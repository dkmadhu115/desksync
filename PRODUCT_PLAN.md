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

### Epic 1.2 — Google sign-in (desktop, browser loopback + PKCE) ✅
- `desksync-backend::oauth`: binds a loopback listener, opens the system browser
  at the backend's `/auth/oauth/google/start?redirect_port&code_challenge`, reads
  the one-time code from the loopback callback, and redeems it with the PKCE
  verifier.
- Backend `auth`: `start` accepts native-client params and records the pending
  flow; the Google callback resolves the account, mints a **one-time grant**, and
  redirects to `http://127.0.0.1:<port>/callback?code=…`; new
  `POST /auth/oauth/desktop/exchange` verifies `S256(verifier)` and issues tokens.
  The Google **client secret never leaves the backend**, and only a user id (never
  a token) sits at rest between the two legs.
- `desksync-agent login` uses the browser by default; `login --password` keeps the
  env-based path for CI/headless.

### Epic 1.3 — Automatic device registration + token-driven runtime ✅
- `desksync-backend::AuthSession` is the single authenticated view of the backend.
  It owns the whole token lifecycle: try → on `401` rotate the refresh token →
  retry once → persist the new pair. A password fallback is used only when
  supplied (CI); otherwise a dead refresh token produces "run `login` again"
  rather than a silent stall.
- `config-ui::agent_auth::bootstrap` resolves credentials in preference order —
  keychain (normal), `DESKSYNC_EMAIL`/`DESKSYNC_PASSWORD` (CI), neither (runs
  signed-out and says so) — then registers this desktop if it has no `device_id`,
  persisting the assigned id to **both** `config.json` and the credential bundle.
- The heartbeat and the session runtime now take an `AuthSession`; neither reads
  the environment or handles tokens. `reauth()` in `session_runtime` is gone.
- `pair` no longer needs environment variables, and `login` registers the desktop
  immediately so it appears in the mobile app without waiting for a daemon start.
- **Acceptance:** fresh install → `login` → device registers and reports presence
  with zero manual steps; token expiry is invisible to every call site.

### Epic 1.4 — Background service ✅ (service/UI split deferred)
- `desksync-core::ServiceManager` installs the agent as a real per-user service:
  writes the platform entry, **activates it immediately** via `launchctl bootstrap`
  (falling back to `load -w`), and reports liveness with `launchctl print`. On
  Linux/Windows the entry is written and `install` honestly reports
  `PendingLogin` instead of claiming the service is up.
- The launchd job now sets `ProcessType=Background` and redirects stdout/stderr to
  `~/Library/Logs/DeskSync/agent.log`, because a login-started service has no
  terminal and "why isn't it working" is always the first question.
- CLI: `service install | status | restart | uninstall`. Unknown arguments are now
  an error — previously any typo silently started the daemon instead.
- The daemon reconciles only the *entry* (`reconcile_entry`), never activation, so
  it cannot bootout the job it is running as; a test pins that the entry it writes
  is byte-identical to the one `install` writes.
- **IPC contract (new `desksync-ipc` crate):** newline-delimited JSON over a Unix
  domain socket in the agent config directory, with a per-install token
  (owner-only file, constant-time compare) and owner-only socket permissions.
  `Ping`, `GetStatus`, and `GetLogPath` are implemented; the service publishes
  live state (sign-in, device id, backend URL, capture settings, whether frames
  are actually being produced, active sessions, last error) and `desksync-agent
  status` renders it. Binding reclaims a socket left by a crashed service but
  refuses to steal one from a live instance. Windows named pipes return
  `Unsupported` rather than pretending to work.
- `DESKSYNC_CONFIG_DIR` relocates the whole state directory, so a second isolated
  instance (own config, identity, lock, and socket) can be run to test a build
  before installing it as the service.
- **Deferred:** a separate `desksync-service` binary and the Tauri `desksync-ui`.
  The IPC contract those need now exists and is tested, so the UI becomes an
  additive client rather than a refactor. The current product direction is
  CLI-on-desktop + app-on-phone, so the window itself is not on the critical path.

### Epic 1.5 — First-run wizard + permissions ✅ (CLI wizard; Tauri window deferred)
- New `desksync-permissions` crate detects real OS state instead of guessing:
  Screen Recording via `CGPreflightScreenCaptureAccess`, Accessibility via
  `AXIsProcessTrusted`, both behind the `native` feature. Every permission carries
  its user-facing label, whether it is *required*, the consequence of missing it,
  and the System Settings deep link. Non-macOS and non-native builds report
  `Unknown` — never a denial they cannot prove.
- `desksync-agent setup` walks sign-in → permissions → registration → background
  service in dependency order, re-checking after each grant, and ends with what is
  still missing rather than a generic "done". Screen Recording additionally tells
  the user to restart the agent, because macOS applies capture consent only at
  process start.
- `desksync-agent permissions` prints the same verdict without prompting, and
  `status` now reports permissions over IPC — the state that matters is the
  *service* binary's, since macOS grants consent per executable.
- Unattended (`setup` with no TTY) degrades to a read-only report: it will not
  install a launchd job, open System Settings, or launch a browser without a
  person agreeing. The readiness decision is a pure, unit-tested function, so
  "optional permission denied" stays ready and `Unknown` never blocks.
- **Startup no longer blocks on the keychain.** The credential read is a blocking
  OS call that took **54 s** on a rebuilt dev binary (macOS asks for keychain
  access whenever the signature changes). It now runs on a blocking thread, warns
  when it is slow, and — critically — IPC starts *before* sign-in, so `status`
  answers in ~0.1 s instead of being dead for the entire window when you most need
  it. `status` distinguishes `signing in…` from a settled `no`.
- `BackendClient` had **no HTTP timeout**: a black-holed backend hung a heartbeat
  or poll forever, silently leaving the device offline with no retry. Now 20 s per
  request, 8 s to connect.
- **Acceptance:** on a clean Mac, `setup` takes a new user to "online + ready",
  and when it cannot, it names the missing piece. Verified end-to-end against the
  Azure backend.
- **Deferred:** the Tauri window. It is an additive IPC client over the contract
  from 1.4 plus this permission report, not a rewrite.

### Epic 1.6 — macOS installer (.pkg) + signing/notarization scaffold ✅
Everything lives in `desktop-agent/packaging/macos/`.

- `build-app.sh` produces **DeskSync.app** — a real bundle, not a loose binary,
  because macOS ties screen-recording and accessibility consent to an executable's
  identity and *shows that identity to the user*. A bare binary appears in System
  Settings as `desksync-agent` with a generic icon and gets a fresh identity every
  time it is replaced; a bundle with a stable id keeps its grants across upgrades.
  `LSUIElement` keeps it out of the Dock, since the agent has no window.
- Universal by default (arm64 + x86_64 via `lipo`), because the alternative is two
  downloads or Rosetta — and Rosetta on a capture/encode hot path costs frames.
  `--host-only` gives a fast dev build.
- `build-pkg.sh` wraps it in a distribution package with a welcome pane and, more
  usefully, a **conclusion pane** naming the one remaining step (`desksync setup`).
  Output: `dist/DeskSync-<version>.pkg`, ~9 MB.
- `postinstall` does exactly two things: links `desksync` onto `PATH`, and on an
  *upgrade* restarts the service — replacing a binary under a running launchd job
  leaves the old process serving, with nothing saying so. It deliberately does not
  start the service on a fresh install: the agent can do nothing before a browser
  sign-in, so that would only produce a service logging "not signed in" and a first
  run needing a restart. `setup` starts it at the right moment.
- `setup` now distinguishes "was already signed in" from "just signed in" and
  restarts an installed service only when this run actually invalidated its
  credentials — a restart drops live sessions, so it is not done gratuitously.
- `sign-and-notarize.sh` is the release path: hardened runtime + timestamp,
  `--entitlements` (network client/server, audio input, Apple events — each one
  justified, none speculative), `productsign`, `notarytool submit --wait`, and
  `stapler staple` so the ticket validates offline. Absent credentials produce a
  precise error listing available identities rather than a fake-signed artifact.
- `uninstall.sh` removes the app, the launchd job (booted out before the entry is
  deleted, or it orphans a live process), the symlink, and the package receipt;
  `--purge` also clears state, logs, and the keychain entry.
- **Acceptance:** `.pkg` builds, installs `/Applications/DeskSync.app` +
  `/usr/local/bin/desksync`, and the conclusion pane points at `desksync setup`,
  which reaches "online + ready". Payload, receipts, bundle, and universal slices
  verified; the install itself needs an admin password, so it is the one step left
  to run by hand.
- **Not signed:** there is no Apple Developer account on this machine
  (`security find-identity` → 0 valid identities), so the artifact is ad-hoc signed
  and Gatekeeper will block it elsewhere. This also means macOS re-asks for
  screen-recording and keychain consent on each rebuild. The scaffold is ready for
  a real Developer ID; CI wiring is Epic 4.1.

### Epic 1.7 — Sessions that stay signed in ✅
Signing in was already good for 30 days (`JWT_REFRESH_TTL`), but a session could be
destroyed within minutes: the theft response to a repeated refresh token revoked
**every** token on the account, and a rotation whose response was lost left the
client holding a token that looked stolen. The result was `session expired` on
every heartbeat until the user signed in again.

- **Token families** (migration `000007`): each sign-in starts a family and
  rotations inherit it. Reuse revokes one family, so a phone with a stale token can
  no longer sign a desktop out.
- **Retry grace** (`JWT_REFRESH_REUSE_GRACE`, 1m): a spent token is honoured again
  while its successor is still unused — that is a client retrying a rotation it
  never got an answer to. Once the successor has been used the client demonstrably
  received it, so an older token is a replay and is refused.
- **`JWT_ACCESS_TTL` 15m → 1h**: fewer rotations, less exposure to all of the
  above. Sign-in length is unchanged; it comes from the refresh token.
- **Agent**: rotates ~1 min *before* expiry instead of waiting for a `401`, and a
  session that truly needs sign-in is reported once and retried each minute rather
  than on every heartbeat.
- **Mobile**: only a refresh the backend *refuses* clears the tokens. A refresh
  that was never delivered (offline, timeout, 5xx) leaves the session intact —
  previously any connectivity blip logged the user out.
- **Acceptance:** the agent runs for days without re-authenticating; a revoked or
  replayed token ends one session, not the account.

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

### Epic 2.4 — Status & diagnostics (partially done)
- ✅ Structured status over the service IPC, surfaced by `desksync-agent status`:
  version, uptime, sign-in, device id, backend URL, capture settings, whether
  frames are being produced, active sessions, and the last error. "Producing
  frames: no" is called out explicitly because on macOS that is the signature of a
  missing Screen Recording grant.
- ✅ Stale-session fix found via this command: the backend served **every** session
  stuck in `connecting`, so an agent restart answered a backlog of zombies (10 on
  the live box), each costing a signaling connection and a WebRTC peer.
  `PendingSessionsForDevice` is now bounded by `PendingSessionMaxAge`.
- Remaining: `frame stream stats` over IPC, and a "copy diagnostics" bundle.

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
