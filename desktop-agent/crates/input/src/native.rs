//! Native input-injection backend built on `enigo` (SendInput on Windows,
//! CGEvent on macOS, uinput/XTest on Linux).
//!
//! Compiled only with the `native` feature. `enigo::Enigo` is `!Send`, so it is
//! owned by a single dedicated OS thread; the async [`InputInjector`] forwards
//! events to that thread over a channel. This keeps all platform handles on one
//! thread (a requirement on macOS/X11) while presenting an async API.

use crate::mapping::{map_hid_key, normalized_to_pixel, NamedKey, PhysicalKey};
use crate::{InputEvent, Modifiers, MouseButton};
use async_trait::async_trait;
use desksync_core::error::{AgentError, Result};
use desksync_core::subsystem::{HealthStatus, Subsystem};
use enigo::{Axis, Button, Coordinate, Direction, Enigo, Key, Keyboard, Mouse, Settings};
use std::sync::mpsc::{self, Sender};
use std::sync::Mutex;
use std::thread::JoinHandle;

enum Command {
    Event(InputEvent),
    Shutdown,
}

/// Input injector backed by the OS via `enigo`, driven from a dedicated thread.
#[derive(Default)]
pub struct EnigoInjector {
    tx: Mutex<Option<Sender<Command>>>,
    thread: Mutex<Option<JoinHandle<()>>>,
}

impl EnigoInjector {
    /// Create a new (not-yet-started) injector.
    pub fn new() -> Self {
        Self::default()
    }
}

fn direction(pressed: bool) -> Direction {
    if pressed {
        Direction::Press
    } else {
        Direction::Release
    }
}

fn modifier_keys(m: Modifiers) -> Vec<Key> {
    let mut keys = Vec::new();
    if m.ctrl {
        keys.push(Key::Control);
    }
    if m.alt {
        keys.push(Key::Alt);
    }
    if m.shift {
        keys.push(Key::Shift);
    }
    if m.meta {
        keys.push(Key::Meta);
    }
    keys
}

fn named_to_enigo(n: NamedKey) -> Option<Key> {
    Some(match n {
        NamedKey::Enter => Key::Return,
        NamedKey::Escape => Key::Escape,
        NamedKey::Backspace => Key::Backspace,
        NamedKey::Tab => Key::Tab,
        NamedKey::Space => Key::Space,
        NamedKey::CapsLock => Key::CapsLock,
        NamedKey::Delete => Key::Delete,
        NamedKey::Home => Key::Home,
        NamedKey::End => Key::End,
        NamedKey::PageUp => Key::PageUp,
        NamedKey::PageDown => Key::PageDown,
        NamedKey::ArrowRight => Key::RightArrow,
        NamedKey::ArrowLeft => Key::LeftArrow,
        NamedKey::ArrowDown => Key::DownArrow,
        NamedKey::ArrowUp => Key::UpArrow,
        NamedKey::F(n) => match n {
            1 => Key::F1,
            2 => Key::F2,
            3 => Key::F3,
            4 => Key::F4,
            5 => Key::F5,
            6 => Key::F6,
            7 => Key::F7,
            8 => Key::F8,
            9 => Key::F9,
            10 => Key::F10,
            11 => Key::F11,
            12 => Key::F12,
            _ => return None,
        },
    })
}

fn physical_to_enigo(key: PhysicalKey) -> Option<Key> {
    match key {
        PhysicalKey::Char(c) => Some(Key::Unicode(c)),
        PhysicalKey::Named(n) => named_to_enigo(n),
    }
}

fn to_button(b: MouseButton) -> Button {
    match b {
        MouseButton::Left => Button::Left,
        MouseButton::Right => Button::Right,
        MouseButton::Middle => Button::Middle,
    }
}

/// Execute one event on the OS. `dims` is the primary display size used to map
/// normalized pointer coordinates.
fn execute(enigo: &mut Enigo, dims: (i32, i32), event: InputEvent) -> Result<()> {
    let map = |e: enigo::InputError| AgentError::subsystem("input", format!("{e}"));
    match event {
        InputEvent::MouseMove { x, y } => {
            let (px, py) = normalized_to_pixel(x, y, dims.0.max(1) as u32, dims.1.max(1) as u32);
            enigo.move_mouse(px, py, Coordinate::Abs).map_err(map)?;
        }
        InputEvent::MouseButton {
            button,
            pressed,
            modifiers,
        } => {
            let mods = modifier_keys(modifiers);
            if pressed {
                for k in &mods {
                    enigo.key(*k, Direction::Press).map_err(map)?;
                }
                enigo.button(to_button(button), Direction::Press).map_err(map)?;
            } else {
                enigo.button(to_button(button), Direction::Release).map_err(map)?;
                for k in mods.iter().rev() {
                    enigo.key(*k, Direction::Release).map_err(map)?;
                }
            }
        }
        InputEvent::Scroll { dx, dy } => {
            if dy != 0.0 {
                enigo.scroll(-dy as i32, Axis::Vertical).map_err(map)?;
            }
            if dx != 0.0 {
                enigo.scroll(dx as i32, Axis::Horizontal).map_err(map)?;
            }
        }
        InputEvent::Key {
            code,
            pressed,
            modifiers,
        } => {
            let Some(key) = map_hid_key(code).and_then(physical_to_enigo) else {
                return Err(AgentError::subsystem("input", format!("unmapped key code {code}")));
            };
            let mods = modifier_keys(modifiers);
            if pressed {
                for k in &mods {
                    enigo.key(*k, Direction::Press).map_err(map)?;
                }
                enigo.key(key, direction(true)).map_err(map)?;
            } else {
                enigo.key(key, direction(false)).map_err(map)?;
                for k in mods.iter().rev() {
                    enigo.key(*k, Direction::Release).map_err(map)?;
                }
            }
        }
        InputEvent::ClipboardText { text } => {
            // Set the host clipboard synchronously on this thread (arboard is
            // `!Send`, so it must be created and used here, never held across a
            // thread boundary).
            let mut cb =
                arboard::Clipboard::new().map_err(|e| AgentError::subsystem("clipboard", format!("open: {e}")))?;
            cb.set_text(text)
                .map_err(|e| AgentError::subsystem("clipboard", format!("write: {e}")))?;
        }
    }
    Ok(())
}

#[async_trait]
impl Subsystem for EnigoInjector {
    fn name(&self) -> &'static str {
        "input"
    }

    async fn start(&self) -> Result<()> {
        let mut guard = self.tx.lock().expect("input tx mutex");
        if guard.is_some() {
            return Ok(()); // already started (idempotent)
        }

        let (tx, rx) = mpsc::channel::<Command>();
        let (ready_tx, ready_rx) = mpsc::channel::<std::result::Result<(), String>>();

        let handle = std::thread::Builder::new()
            .name("desksync-input".into())
            .spawn(move || {
                let mut enigo = match Enigo::new(&Settings::default()) {
                    Ok(e) => {
                        let _ = ready_tx.send(Ok(()));
                        e
                    }
                    Err(e) => {
                        let _ = ready_tx.send(Err(format!(
                            "failed to initialize input backend (accessibility permission?): {e}"
                        )));
                        return;
                    }
                };
                let dims = enigo.main_display().unwrap_or((1920, 1080));
                while let Ok(cmd) = rx.recv() {
                    match cmd {
                        Command::Shutdown => break,
                        Command::Event(event) => {
                            if let Err(e) = execute(&mut enigo, dims, event) {
                                tracing::warn!(error = %e, "input injection failed");
                            }
                        }
                    }
                }
            })
            .map_err(|e| AgentError::subsystem("input", format!("spawn thread: {e}")))?;

        // Wait for the backend to report initialization success/failure.
        match ready_rx.recv() {
            Ok(Ok(())) => {}
            Ok(Err(msg)) => {
                let _ = handle.join();
                return Err(AgentError::subsystem("input", msg));
            }
            Err(e) => {
                return Err(AgentError::subsystem(
                    "input",
                    format!("backend thread died during init: {e}"),
                ))
            }
        }

        *guard = Some(tx);
        *self.thread.lock().expect("input thread mutex") = Some(handle);
        Ok(())
    }

    async fn stop(&self) -> Result<()> {
        if let Some(tx) = self.tx.lock().expect("input tx mutex").take() {
            let _ = tx.send(Command::Shutdown);
        }
        if let Some(handle) = self.thread.lock().expect("input thread mutex").take() {
            let _ = handle.join();
        }
        Ok(())
    }

    async fn health(&self) -> HealthStatus {
        if self.tx.lock().expect("input tx mutex").is_some() {
            HealthStatus::Healthy
        } else {
            HealthStatus::Stopped
        }
    }
}

#[async_trait]
impl crate::InputInjector for EnigoInjector {
    async fn inject(&self, event: InputEvent) -> Result<()> {
        let guard = self.tx.lock().expect("input tx mutex");
        let tx = guard
            .as_ref()
            .ok_or_else(|| AgentError::subsystem("input", "injector not started"))?;
        tx.send(Command::Event(event))
            .map_err(|e| AgentError::subsystem("input", format!("send: {e}")))
    }
}
