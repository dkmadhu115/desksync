# DeskSync — Productization Plan

Turn DeskSync from a developer-run stack into a **commercial-grade product**: a
non-technical user installs an app, signs in with Google, grants permissions via
a guided wizard, and connects from their phone in **under two minutes** — no
Docker, Cargo, or terminal.

> **Scope guardrail:** the backend architecture (Go microservices), the WebRTC
> media pipeline, and the mobile app are **not** re-architected. All work here is
> about installation, onboarding, updates, device management, and packaging.
>
> **Branching:** `main` stays stable/releasable. **All productization work lands
> on `develop`** and is promoted to `main` per release.

---

## 0. Where we are today (baseline)

| Area | Today | Target |
|------|-------|--------|
| Desktop install | `cargo build --features native` + run binary; creds via `DESKSYNC_EMAIL/PASSWORD` env | Signed native installer per OS; background service + tray UI |
| Auth | Email/password (env vars) | Google OAuth (browser loopback), tokens in OS keychain |
| Device registration | `desksync-agent pair` prints a QR | Automatic on first sign-in; QR optional |
| Trust | QR pairing per device | Same-account auto-discovery + connect-time approval prompt |
| Updates | Manual rebuild | Silent, signed auto-updates |
| Lifecycle | Foreground process, dies with terminal | 24/7 background service, survives UI close |
| Distribution | none | GitHub Releases + stores, update manifest |

Existing building blocks we reuse: `desksync-core` (`AgentConfig`, `AgentStore`,
`DeviceIdentity`, `Autostart`, `SingleInstance`), `desksync-backend` (REST client,
enrollment), `session_runtime` (WebRTC answerer), and the Go `auth` service's
existing Google OAuth support.

---

## 1. Target architecture (desktop)

Split the single foreground binary into a **service** + a **UI**, sharing the
existing crates.

```text
┌──────────────────────────────┐        ┌───────────────────────────┐
│  desksync-service (daemon)    │  IPC   │  desksync-ui (Tauri tray)  │
│  runs 24/7, no window         │◄──────►│  status · settings · setup │
│                               │ local  │  wizard · permissions      │
│  capture · input · clipboard  │ socket │                           │
│  WebRTC · heartbeat · updates │        │  (closing UI ≠ stopping    │
│  device registration          │        │   the service)            │
└──────────────────────────────┘        └───────────────────────────┘
```

- **Service**: everything remote-access-critical. Installed as a
  launchd agent (macOS), Windows Service, or systemd user service (Linux).
- **UI**: Tauri app for login, status, settings, logs, and the first-run wizard.
  Talks to the service over a local IPC channel (UDS / named pipe) with a
  per-install token.
- **IPC contract**: small JSON request/response + event stream
  (`GetStatus`, `Login`, `Logout`, `GetSettings`, `SetSettings`, `GetLogs`,
  `OpenPermission`, events: `StatusChanged`, `ConnectionRequest`).

---

## 2. Phase 1 — User Experience (installable, sign-in, auto-register)

**Goal:** a signed installer that installs a background service, a tray UI with a
first-run wizard, Google sign-in, keychain-stored credentials, and automatic
device registration. macOS first (our live test box), then Windows, then Linux.

### Epic 1.1 — Secure credential storage *(foundation, do first)*
- New `SecretStore` trait in `desksync-core` with two impls:
  - `KeyringSecretStore` (feature `os-keychain`) → macOS Keychain / Windows
    Credential Manager / Linux Secret Service via the `keyring` crate.
  - `FileSecretStore` fallback (0600 file under the config dir) for headless CI
    and Linux boxes without a secret service.
- Store a `TokenBundle { access_token, refresh_token, device_id }`; the X25519
  private key stays in `DeviceIdentity` (already local-only) but its at-rest
  location is documented.
- **Acceptance:** set/get/delete round-trip tested; default (non-native) build
  compiles with the file impl; secrets never written to `config.json` or logs.
- **Status: implemented in the first commit of this plan.**

### Epic 1.2 — Google sign-in (desktop, browser loopback + PKCE)
- Add `desksync-backend::oauth`: start a localhost loopback listener, open the
  system browser to the backend's Google OAuth authorize URL with a PKCE
  challenge + `redirect_uri=http://127.0.0.1:<port>/callback`, receive the code,
  exchange it via the gateway for `{access, refresh, device_id}`.
- Backend: confirm/extend the `auth` service to support the loopback redirect and
  a token exchange for native clients (reuse existing Google OAuth; add a
  desktop client id + allowed loopback redirect).
- Replace `Credentials::from_env()` as the primary path; env remains a fallback
  for CI/dev.
- **Acceptance:** `desksync-agent login` opens the browser, completes sign-in,
  and persists tokens to the keychain; heartbeat/session use stored tokens with
  refresh rotation.

### Epic 1.3 — Automatic device registration
- On first authenticated start with no `device_id`: generate identity (exists),
  register the device (exists), persist `device_id` to config + keychain, start
  heartbeat. No user action, no QR required.
- **Acceptance:** fresh install → sign in → device shows **online** on mobile
  within one heartbeat interval, zero manual steps.

### Epic 1.4 — Service/UI split + background service
- Extract the current `config-ui/main.rs` runtime into a `desksync-service` binary
  (headless) and add a `desksync-ui` Tauri app.
- Service installers register: launchd `~/Library/LaunchAgents/com.desksync.agent.plist`
  (macOS), `systemd --user` unit (Linux), Windows Service (Windows).
- Keep `SingleInstance` guard; add graceful stop + restart hooks.
- **Acceptance:** closing the UI leaves the service running and connectable;
  service auto-starts on login.

### Epic 1.5 — First-run wizard + permissions (macOS first)
- Tauri wizard: Welcome → Continue with Google → grant Screen Recording →
  grant Accessibility → grant Notifications → “Device registered” → Finish.
- Detect permission state and deep-link to the correct System Settings pane;
  re-check on focus.
- **Acceptance:** a new user reaches “online + ready” purely through the wizard.

### Epic 1.6 — macOS installer (.pkg) + signing/notarization scaffold
- Tauri bundler / `pkgbuild` to produce `DeskSync.pkg` that installs the app +
  the launchd service and launches the wizard.
- Wire signing + notarization in CI (Epic 4.1) — secrets via GitHub Actions.
- **Acceptance:** double-click `.pkg` on a clean Mac → installed, service running,
  wizard opens.

**Phase 1 exit criteria:** on macOS, download `.pkg` → install → Google sign-in →
grant permissions via wizard → device auto-registers → connect from phone. All
without a terminal.

---

## 3. Phase 2 — Reliability (updates, trust, permissions, diagnostics)

### Epic 2.1 — Auto-updates
- On launch and on a timer: query an **update manifest** (`/releases/latest.json`
  served by backend or GitHub Releases), compare semver, download, **verify
  signature**, swap the binary, restart the service. Silent by default; UI shows
  “updating”.
- Use the Tauri updater (UI) + a service-side updater for the daemon.
- **Acceptance:** publishing a newer signed build causes clients to self-update
  within one check interval; signature-mismatch aborts safely.

### Epic 2.2 — Device approval / trusted devices
- Connect-time prompt on the desktop: **Allow Once / Always Allow / Reject**.
- Persist “Always Allow” trust per mobile device (keychain + backend trust
  record); auto-accept trusted peers.
- Keep QR pairing as an optional alternative for out-of-band trust.
- Backend: extend `pairing`/`device` to record trust decisions and gate session
  creation on them.
- **Acceptance:** first connect prompts; “Always Allow” makes subsequent connects
  seamless; “Reject” blocks and notifies.

### Epic 2.3 — Cross-OS permission handling
- Windows (Accessibility/UAC, firewall rule, notifications) and Linux
  (PipeWire/X11/Wayland capture consent) flows, mirroring macOS.
- Installer detects missing permissions and offers one-click guidance.

### Epic 2.4 — Status & diagnostics
- UI: connection status, last error, “copy diagnostics” bundle (logs + versions).
- Structured health endpoint on the service IPC; surface `frame stream stats`.

**Phase 2 exit criteria:** self-updating, trusted-device connect without QR, and
clear status/diagnostics on all three desktop OSes.

---

## 4. Phase 3 — Product features & platform breadth

### Epic 3.1 — Windows + Linux installers
- Windows: **WiX** `DeskSyncSetup.exe` (service + tray, code-signed).
- Linux: `.deb`, `.rpm`, and AppImage; systemd user unit.

### Epic 3.2 — Web management portal
- Web dashboard (reuse backend APIs): my devices, rename/remove, status,
  session history, download latest agent. AuthN via the same Google OAuth.

### Epic 3.3 — Feature depth
- File transfer + clipboard sync, remote commands (lock/restart/shutdown),
  connection history, notifications, adaptive quality tuning.

---

## 5. Cross-cutting: CI/CD (spans phases, stand up early)

### Epic 4.1 — Release pipeline (`.github/workflows/release.yml`)
- Matrix build on push of a `v*` tag: macOS (.pkg, sign + notarize),
  Windows (WiX .exe, sign), Linux (.deb/.rpm/AppImage).
- Publish to **GitHub Releases** and emit/refresh the **update manifest**
  consumed by Epic 2.1.
- Secrets (signing certs, notarization creds, Google desktop client id) via
  GitHub Actions encrypted secrets — never in the repo.

---

## 6. Technology choices (locked)

| Component | Choice |
|-----------|--------|
| Desktop service + UI | Rust (existing crates) + Tauri (UI) |
| Windows installer | WiX Toolset, signed |
| macOS installer | `.pkg` + Apple signing + notarization |
| Linux | `.deb`, `.rpm`, AppImage + systemd user unit |
| Auto-updates | Tauri updater (UI) + service-side signed updater |
| Auth | Google OAuth 2.0 (loopback + PKCE) → JWT/refresh |
| Secure storage | OS keychain via `keyring`, file fallback |
| Notifications | FCM/APNs (mobile) + native desktop notifications |
| Device discovery | Existing backend device registry |
| Backend / Mobile | Unchanged |

---

## 7. Sequencing & tracking

1. **1.1 Secure storage** ✅ (landing now) → 1.2 Google sign-in → 1.3 auto-register
   → 1.4 service/UI split → 1.5 wizard → 1.6 macOS installer.
2. Stand up **4.1 CI release** alongside 1.6.
3. Phase 2 (updates → trust → permissions → diagnostics).
4. Phase 3 (Windows/Linux installers → web portal → feature depth).

Each epic ships as its own reviewable commit/PR on `develop`, is production-ready
(tested, no dead code), and is demoed before the next begins — consistent with the
project's phase-by-phase workflow. Operational commands live in
[`RUNBOOK.md`](RUNBOOK.md); system design in [`ARCHITECTURE.md`](ARCHITECTURE.md).
