# Desktop agent design

The desktop agent is a headless background daemon (with a Tauri config UI added
later) that captures the screen, injects input from a paired mobile client,
shares the clipboard, and streams over WebRTC (Phase 5). It is a Cargo
workspace of focused crates so platform-specific code is isolated behind traits
and the runtime stays testable.

## Crate layout

| Crate | Responsibility |
|-------|----------------|
| `desksync-core` | Runtime (`Agent`), `Subsystem` trait + health model, config, unified error, **device identity (X25519)**, **on-disk persistence**, **single-instance lock**, **autostart** |
| `desksync-capture` | `ScreenCapturer` trait, `Frame`/`Monitor` model, pure frame-scaling utils, the `CaptureLoop` service, and the native `XcapCapturer` |
| `desksync-input` | `InputInjector` trait + event model, pure coordinate/keycode `mapping`, `Clipboard` abstraction, and the native `EnigoInjector` |
| `desksync-transport` | Signaling envelope + `SignalingTransport` trait, `ReplayGuard` (WebRTC lands in Phase 5) |
| `desksync-agent` (config-ui) | Process entrypoint: tracing, single-instance, config+identity, subsystem wiring, graceful shutdown |

## Subsystem model

Capture, input, and the capture loop implement `Subsystem` (`start`/`stop`/
`health`). The `Agent` starts them in order (rolling back on failure) and stops
them in reverse. Subsystems are injected as trait objects (`Arc<dyn …>`), so the
real backends and the no-op test doubles are interchangeable.

## Native vs default backends (the `native` feature)

Real OS access is provided by mature cross-platform crates:

- **Capture** — [`xcap`] (ScreenCaptureKit / DXGI / PipeWire+X11) → `XcapCapturer`
- **Input** — [`enigo`] (SendInput / CGEvent / uinput·XTest) → `EnigoInjector`
- **Clipboard** — [`arboard`] (NSPasteboard / Win clipboard / X11·Wayland) → `ArboardClipboard`

These are **optional dependencies behind the `native` cargo feature (off by
default)**. Rationale and trade-offs are recorded in
[ADR 0005](../adr/0005-desktop-agent-native-backends.md). In short:

- Default build = pure Rust, no system libraries or display/permissions needed.
  It builds and unit-tests everywhere, including headless Linux CI.
- `--features native` = real backends, built and clippy-linted on a macOS CI
  runner and shipped in desktop builds.

```bash
make agent-build            # portable, no-op backends
make agent-build-native     # real backends (this OS)
make agent-run-native       # run with real backends (asks for OS permissions)
```

Because `enigo::Enigo` and `arboard::Clipboard` are `!Send`, input injection
runs on a dedicated OS thread that owns the `Enigo` instance and consumes events
from a channel; the async `InputInjector` forwards to it. `xcap` capture calls
run on the Tokio blocking pool.

## Capture pipeline

`CaptureLoop` is a `Subsystem` that ticks at `target_fps`, captures a frame from
the selected monitor (primary by default), downscales it so its height fits
`max_height` (aspect-preserving, even dimensions for encoders), and publishes
the latest frame on a `watch` channel. `watch` gives lossy/coalescing delivery —
the correct semantics for live video, where a slow consumer skips stale frames
rather than lagging. Transient capture errors (e.g. display sleep) are logged
and retried on the next tick rather than killing the loop.

## Input mapping

Events from the mobile client are resolution-independent: pointer coordinates in
`[0,1]` and keys as **USB HID usage codes**. Pure functions
(`normalized_to_pixel`, `map_hid_key`) convert these to pixel coordinates
(clamped so a bad event can never drive the cursor off-screen) and a
backend-neutral `PhysicalKey`; the native backend only translates `PhysicalKey`
into its own enum. This keeps the arithmetic and key table fully unit-tested
without any OS.

## Security-relevant state

- **Device identity**: a long-lived X25519 key pair generated on first run. The
  **private key never leaves the device** — it is written once to
  `identity.key` with owner-only (`0600`) permissions and used only for the
  pairing/session ECDH; only the public key is shared. See
  [security design](security.md).
- **Persistence**: `config.json` + `identity.key` under the platform config dir
  (`~/Library/Application Support/desksync`, `~/.config/desksync`,
  `%APPDATA%\desksync`). Writes are atomic (temp file + rename).
- **Single instance**: an advisory exclusive lock (`agent.lock`) ensures only
  one agent drives capture/input; a second instance exits cleanly.
- **Autostart**: launch-at-login via a LaunchAgent plist (macOS), XDG
  `.desktop` entry (Linux), or Startup launcher (Windows), reconciled from the
  `autostart` config flag.

[`xcap`]: https://crates.io/crates/xcap
[`enigo`]: https://crates.io/crates/enigo
[`arboard`]: https://crates.io/crates/arboard
