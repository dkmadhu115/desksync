# 5. Desktop agent native backends via cross-platform crates behind a feature

- Status: Accepted
- Date: 2026-07-23

## Context

Phase 3 implements the desktop agent's real subsystems: screen capture,
keyboard/mouse injection, and clipboard. The specification names the underlying
OS APIs (ScreenCaptureKit on macOS, DXGI Desktop Duplication on Windows,
PipeWire/X11 on Linux for capture; SendInput/CGEvent/uinput for input;
NSPasteboard/Win clipboard/X11 for clipboard).

Two implementation strategies were considered:

1. **Hand-rolled per-OS FFI** — bind directly to each platform API. Maximum
   control, but a very large `unsafe` surface, three separate implementations to
   maintain, and hard to build/test on a single developer machine or in CI.
2. **Mature cross-platform crates** — `xcap` (capture), `enigo` (input), and
   `arboard` (clipboard). These wrap exactly the platform APIs above behind a
   single safe Rust API.

We also need the workspace to build and be unit-tested on headless Linux CI,
where no display or input/screen-recording permissions exist, and where pulling
in GUI system libraries makes CI brittle.

## Decision

- Use `xcap`, `enigo`, and `arboard` as the native backends.
- Gate them behind an **optional `native` cargo feature**, off by default. The
  default build (and the Linux CI job) compiles only pure Rust: the subsystem
  traits, the `NoopCapturer`/`NoopInjector`/`NoopClipboard`, the frame-scaling
  utilities, the coordinate/keycode mapping, the capture loop, device identity,
  and persistence — all fully unit-tested.
- The real backends (`XcapCapturer`, `EnigoInjector`, `ArboardClipboard`) are
  compiled and linted on a **macOS CI job** with `--features native`, and are
  what ship in desktop builds.
- Keep `#![forbid(unsafe_code)]` in the agent crates; all `unsafe` lives inside
  the vetted backend crates.
- Because `enigo::Enigo` and `arboard::Clipboard` are `!Send`, input injection
  runs on a dedicated OS thread that owns the `Enigo` instance and receives
  events over a channel; the async API forwards to it. `xcap` calls run on the
  Tokio blocking pool.
- The device's X25519 private key is generated locally and persisted with
  owner-only file permissions; only the public key is shared. (A future
  hardening ADR may move it into the OS keychain.)

## Consequences

- The default `cargo build`/`cargo test` stays dependency-free, fast, and green
  on any platform, including headless CI — no `apt-get` of GUI libraries.
- Native code cannot regress silently: it is compiled + clippy-linted on macOS
  CI every run. Windows/Linux native builds follow the same feature flag.
- Behaviour that requires a real display or OS permissions is validated
  manually on developer machines, not in CI. Only pure logic is unit-tested.
- We accept a dependency on the `xcap`/`enigo`/`arboard` maintenance cadence in
  exchange for one safe implementation instead of three `unsafe` ones. The
  trait abstraction means a backend can be swapped without touching the runtime.
