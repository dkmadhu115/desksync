# DeskSync — Operations Runbook

Practical guide to **run, deploy, and resume** DeskSync (remote desktop: view & control
your Mac from an Android phone). Read this first when picking the project back up.

> Secrets (account passwords, TURN credentials, OAuth client secrets, SSH passwords)
> are **not** stored in this repo. Keep them in your password manager. Placeholders
> below look like `<...>`.

---

## 1. What DeskSync is

```
   PHONE (controller / viewer)                 MAC / any host (agent)
   ─ Flutter app                    ───▶       ─ Rust desktop agent
   ─ sees the remote screen                    ─ captures screen (JPEG)
   ─ sends taps / keys / scroll                ─ injects input, streams frames
                         │                                  ▲
                         └──────── Backend (Go, on k8s) ────┘
                            auth · devices · pairing · sessions
                            signaling (WebSocket) · TURN relay (coturn)
```

- **Host = the machine being controlled** (your Mac). It runs the **agent**, which
  registers itself as a device and answers incoming sessions. It is the WebRTC
  *answerer*.
- **Controller = the phone.** It logs in, sees the host as an online device, taps
  **Connect**, and is the WebRTC *offerer*.
- **Media path:** screen frames are JPEG-encoded on the host and pushed to the phone
  over a WebRTC **data channel** (`frames`); input flows back over the `input` /
  `control` data channels. Connectivity uses STUN + a **coturn TURN relay**.

### Repo layout

| Path | What |
|------|------|
| `backend/` | Go microservices (gateway, auth, device, pairing, session, signaling, relay, notification, monitoring) |
| `desktop-agent/` | Rust agent — `crates/{core,backend,capture,media,input,devtools,config-ui}` |
| `mobile/` | Flutter app (Riverpod, GoRouter, flutter_webrtc) |
| `helm/desksync/` | Helm chart + `values.yaml`, `values-azure.yaml`, `values-vps.yaml` |
| `docker/` | local docker-compose stack |

---

## 2. Current live environment (Azure)

Everything is deployed on an Azure VM with **k3s** in the `desksync` namespace.

| Item | Value |
|------|-------|
| VM SSH | `apollo@20.109.60.233` (password in your vault) |
| API base (gateway) | `http://20.109.60.233:8080` |
| Signaling (WS) | `ws://20.109.60.233:8085/api/v1/signaling` |
| TURN relay | `20.109.60.233:3478` (coturn, relay ports `50000-50100`) |
| k8s namespace | `desksync` (Helm release `desksync`) |
| Test account | `dkmadhu2109@gmail.com` (password in your vault) |

> **Azure NSG:** inbound rules must be open for `8080/tcp`, `8085/tcp`,
> `3478/udp+tcp`, and `50000-50100/udp` (TURN relay). If ICE fails but signaling
> works, check these first.

Toolchain already installed on the VM: `/opt/flutter`, `/opt/android-sdk`,
project at `/home/apollo/DeskSync`, `/tmp/cloudflared`, nginx serving APKs from
`/var/www/apk` on ports 80 & 8000.

---

## 3. Run the Mac agent (host side)

The "app on the Mac" is the **agent** — a background program, not a GUI window.

### One-time: macOS Screen Recording permission

The agent captures the screen, so the app that launches it needs permission:

1.  → **System Settings → Privacy & Security → Screen & System Audio Recording**
2. Enable the launching app (e.g. **Cursor**, or **Terminal**, or a packaged
   `DeskSync.app`). Click **Quit & Reopen** when prompted.

Without this, capture produces **blank** frames.

### Build & run

```bash
cd desktop-agent
cargo build -p desksync-agent --features native

./target/debug/desksync-agent setup    # once: sign in, permissions, registration
./target/debug/desksync-agent          # run the agent
```

`setup` is the guided path: it signs you in, checks Screen Recording and
Accessibility (opening the right System Settings pane for anything missing),
registers this Mac, and offers to install the background service. It ends by
naming whatever is still missing, so there is no guessing. `login` alone still
works if you only want credentials.

`login` stores credentials in the keychain and registers this Mac as a device, so
running the agent needs **no environment variables**. `DESKSYNC_EMAIL` /
`DESKSYNC_PASSWORD` still work (`login --password`, and as an automatic fallback
if a stored refresh token is ever rejected) — that path exists for CI.

Healthy startup logs:

```
using stored credentials from the OS keychain
desktop registered automatically  device_id=...   ← first run only
agent running; press Ctrl-C to stop
reporting presence  device_id=... interval_secs=15
session runtime ready; watching for incoming sessions
capture pipeline produced first frame  width=... height=...
```

For verbose troubleshooting: `DESKSYNC_LOG="info,desksync_media=trace,webrtc_ice=warn"`.
The variable is `DESKSYNC_LOG`, **not** `RUST_LOG` — setting `RUST_LOG` is silently
ignored and you get default `info` output, which looks like the logging you asked
for simply not existing.

### Ask the running agent what it's doing

```bash
./target/debug/desksync-agent status
```

```text
DeskSync service v0.1.0
  Signed in:       yes
  Device id:       b1328bbf-f595-45ec-8d85-a7a5b081f502
  Backend:         http://20.109.60.233:8080
  Capture:         max 720p at 20 fps — producing frames
  Active sessions: 0
  Uptime:          15s
  Last error:      none
  Permissions:
    Screen & System Audio Recording    granted
    Accessibility                      granted
    Notifications                      unknown
```

This works whether the agent runs in the foreground or as the background service —
it asks over a Unix socket in the config directory (owner-only, plus a per-install
token). Read it as follows:

| Line | What it tells you |
|------|-------------------|
| `Signed in: no` | run `login`; the device will show offline |
| `Signed in: signing in…` | still reading credentials — a slow keychain read, not a failure |
| `NO frames captured` | Screen Recording permission missing → blank frames |
| `Permissions` | as seen by *this* binary; macOS grants consent per executable |
| `Active sessions` | how many phones are connected right now |
| `Last error` | the most recent failure, cleared on recovery |

### Permissions

```bash
./target/debug/desksync-agent permissions
```

Reports what the OS actually grants this binary, what breaks without each one, and
nothing it cannot verify (non-macOS builds say `unknown` rather than guessing).

Three macOS behaviours cause most confusion:

- **Consent is per executable.** Rebuilding the agent, or moving it, can require a
  fresh grant — and after `service install`, the grant that matters is the one for
  the installed path.
- **Capture consent is read at process start.** Granting Screen Recording while the
  agent is running does nothing until you restart it.
- **A binary launched from Terminal inherits Terminal's grant.** macOS attributes
  screen access to the *responsible* process, so `desksync-agent permissions` run
  in a terminal can report `granted` on the strength of Terminal's own permission,
  while the same binary started by launchd has none. That is why `status` reports
  the permissions the **service** process sees — trust that one.

A rebuilt binary also changes its signature, so macOS re-asks for **keychain**
access on the next start. That read blocks until you answer (54 s was observed
while a dialog waited); the agent logs `reading stored credentials was slow` when
this happens. Choose *Always Allow* to stop the pause recurring. `status` works
throughout, reporting `signing in…`.

To try a build without disturbing the installed service, run it against a throwaway
state directory — its own config, identity, instance lock, and socket:

```bash
DESKSYNC_CONFIG_DIR=/tmp/desksync-test ./target/debug/desksync-agent &
DESKSYNC_CONFIG_DIR=/tmp/desksync-test ./target/debug/desksync-agent status
```

### Run it in the background (survives closing the terminal)

```bash
./target/debug/desksync-agent service install    # starts now + at every login
./target/debug/desksync-agent service status
tail -f ~/Library/Logs/DeskSync/agent.log
./desksync-agent service restart                 # after rebuilding the binary
./desksync-agent service uninstall
```

`install` writes `~/Library/LaunchAgents/com.desksync.agent.plist` pointing at
**the executable you ran it from**, and launchd restarts the agent if it exits.
Two consequences on macOS:

- Re-run `service install` after rebuilding or moving the binary.
- Screen Recording consent is tied to that binary, so grant it to the agent
  itself once installed (rather than to Cursor/Terminal). Replacing the binary
  changes its signature and re-prompts.

Stop the foreground agent before installing the service — the single-instance
lock allows only one.

When a phone connects you should see:

```
frames data channel established
frame stream stats  sent:30  dropped:0   ← frames flowing (dropped≈0 is good)
```

### Signing in (Google, via the browser)

```bash
./target/debug/desksync-agent login              # opens the browser for Google sign-in
./target/debug/desksync-agent login --password   # email/password from env (CI/headless)
./target/debug/desksync-agent logout             # clears stored credentials
```

`login` stores the access token, refresh token, and device id in the **OS
keychain** (macOS Keychain / Windows Credential Manager / Linux Secret Service) on
`--features native` builds, or an owner-only `secrets.json` otherwise. Nothing
sensitive is written to `config.json`. It then registers this desktop, so the
device appears in the mobile app right away.

### How long a sign-in lasts

**30 days**, and every day of use extends it. `login` is needed again only after a
month of the agent never running, or if the session is explicitly revoked.

Access tokens are separate and short (`JWT_ACCESS_TTL`, 1h): the agent rotates
them behind the scenes, shortly before they expire and again on any `401`, and
writes each new pair back to the keychain. That is why `JWT_ACCESS_TTL` can stay
short — it bounds how long a revoked account keeps working, not how long a user
stays logged in.

Rotation is deliberately careful, because a refresh token must never be presented
twice: the backend treats a repeat as possible theft.

- Only **one** rotation is ever in flight, so the heartbeat and session poller
  can't refresh with the same token when they expire together.
- A token the backend has refused is never sent again.
- If another DeskSync process (`setup`, `login`, a second agent) rotated the
  shared keychain entry, this one adopts what is stored rather than reporting an
  expiry.
- The backend allows a spent token to be retried for `JWT_REFRESH_REUSE_GRACE`
  (1m), so a rotation whose response was lost to a network drop can be repeated
  instead of costing the session.

If the refresh token really is rejected, the agent logs `session expired; run
\`desksync-agent login\`` **once** and retries on a one-minute cadence — running
`login` in another terminal is picked up without restarting the service.

> **Google Cloud Console setup (one-time).** The desktop never holds the client
> secret — it signs in *through* the backend. So the only authorized redirect URI
> Google needs is the **backend callback**:
>
> ```text
> http://20.109.60.233:8080/api/v1/auth/oauth/google/callback     # Azure
> http://localhost:8080/api/v1/auth/oauth/google/callback         # local dev
> ```
>
> Set `GOOGLE_OAUTH_CLIENT_ID` / `GOOGLE_OAUTH_CLIENT_SECRET` / `GOOGLE_OAUTH_REDIRECT_URL`
> in the backend's environment (local: the gitignored `.env`; Kubernetes: the
> chart's secret values). Never commit them to `.env.example`.

### Agent config

`~/Library/Application Support/desksync/config.json` — points the agent at the
backend and controls capture quality:

```json
{
  "device_id": "<uuid or 'unregistered' to force re-pair>",
  "backend_url": "ws://20.109.60.233:8085/api/v1/signaling",
  "api_url": "http://20.109.60.233:8080",
  "codec": "vp9",
  "target_fps": 20,
  "max_height": 720,
  "autostart": false,
  "heartbeat_secs": 15
}
```

To re-point at a different backend, edit `backend_url` + `api_url`, set
`device_id` to `unregistered`, and restart — the agent re-registers automatically
on the next authenticated start, no `pair` needed. `autostart: true` keeps the
service entry installed; the running daemon never starts/stops itself.

### Build the installer (`.pkg`)

```bash
cd desktop-agent/packaging/macos
./build-pkg.sh                 # universal (arm64 + x86_64) — ~3 min
./build-pkg.sh --host-only     # current arch only, for a quick dev build
```

Output is `desktop-agent/dist/DeskSync-<version>.pkg` (~9 MB). It installs
`/Applications/DeskSync.app` and links `desksync` into `/usr/local/bin`, then its
last pane tells the user to run `desksync setup`.

Install it locally:

```bash
sudo installer -pkg ../../dist/DeskSync-0.1.0.pkg -target /
desksync setup
```

Remove it again:

```bash
./uninstall.sh            # keeps your sign-in and device identity
./uninstall.sh --purge    # also clears state, logs, and the keychain entry
```

The app is a **bundle**, not a bare binary, on purpose: macOS shows the user the
identity it is granting screen access to, and a bundle with a stable id shows up as
"DeskSync" and keeps its grants across upgrades. Upgrades are handled in
`postinstall` — replacing the binary under a running launchd job would otherwise
leave the old process serving with nothing to indicate it.

### Signing and notarization

Builds from this repo are **ad-hoc signed only** — there is no Apple Developer
identity on this machine (`security find-identity -v -p codesigning` → 0 found).
Consequences worth knowing before sharing a build:

- Gatekeeper blocks it on any other Mac (locally: right-click → Open, or use
  `sudo installer`).
- macOS re-asks for screen recording and keychain access after every rebuild,
  because an unsigned binary's identity changes each time.

With a Developer ID, one command produces a distributable installer:

```bash
export DESKSYNC_SIGN_IDENTITY="Developer ID Application: You (TEAMID)"
export DESKSYNC_INSTALLER_IDENTITY="Developer ID Installer: You (TEAMID)"
export DESKSYNC_NOTARY_PROFILE="desksync-notary"   # notarytool store-credentials
./sign-and-notarize.sh
```

It signs with the hardened runtime and a secure timestamp, signs the installer,
submits to Apple, waits, and staples the ticket so it validates offline. Missing
credentials stop it with a specific error instead of emitting something that only
looks signed. `DESKSYNC_SKIP_NOTARIZE=1` signs without notarizing for internal
test builds.

---

## 4. Mobile app (controller side)

### Build the APK (on the Azure VM — has the Android SDK)

```bash
ssh apollo@20.109.60.233
export PATH=$PATH:/opt/flutter/bin ANDROID_HOME=/opt/android-sdk ANDROID_SDK_ROOT=/opt/android-sdk
cd /home/apollo/DeskSync/mobile
flutter build apk --release --split-per-abi \
  --dart-define=DESKSYNC_API_BASE_URL=http://20.109.60.233:8080 \
  --dart-define=DESKSYNC_SIGNALING_URL=ws://20.109.60.233:8085/api/v1/signaling
```

Outputs in `build/app/outputs/flutter-apk/`:
- `app-arm64-v8a-release.apk`  (~38 MB) — most phones
- `app-armeabi-v7a-release.apk` (~28 MB) — older 32-bit phones

Bump `mobile/pubspec.yaml` `version:` before each build so installs update in place.

### Sync local code → VM before building

```bash
sshpass -p '<vm-pass>' rsync -az mobile/ apollo@20.109.60.233:/home/apollo/DeskSync/mobile/
```

### Serve the APK for phone download

nginx serves `/var/www/apk`; a Cloudflare quick tunnel gives an HTTPS URL that
works around mobile carrier/proxy issues with plain HTTP:

```bash
# publish (on VM)
sudo cp build/app/outputs/flutter-apk/app-arm64-v8a-release.apk /var/www/apk/desksync.apk
# expose over HTTPS (on VM); prints a https://<random>.trycloudflare.com URL
/tmp/cloudflared tunnel --url http://localhost:8000
```

Then download `https://<tunnel-host>/desksync.apk` on the phone and install.

### Use it

1. Open app → log in (`dkmadhu2109@gmail.com`).
2. The Mac appears as an **online device** (agent must be running).
3. Tap **Connect** → the Mac's screen streams to the phone; touches control it.

---

## 5. Deploy / redeploy the backend (k3s on Azure)

```bash
ssh apollo@20.109.60.233
cd /home/apollo/DeskSync

# Build service images and import into k3s' containerd (k3s does NOT use dockerd):
#   docker build ... -t desksync/<svc>:local
#   docker save desksync/<svc>:local | sudo k3s ctr images import -

# Install / upgrade the release:
sudo helm upgrade --install desksync ./helm/desksync \
  -n desksync --create-namespace \
  -f ./helm/desksync/values-azure.yaml

# Check:
sudo k3s kubectl get pods -n desksync
sudo k3s kubectl get svc  -n desksync
```

`values-azure.yaml` highlights: images pulled from local containerd, ingress off
(uses ServiceLB), gateway + signaling exposed via `LoadBalancer`, and coturn with
`externalIP: 20.109.60.233/10.2.0.4` (1:1 NAT) and relay ports `50000-50100`.

---

## 6. Local development (docker-compose)

```bash
cp .env.example .env      # fill in JWT/OAuth values
docker compose -f docker/docker-compose.yml up --build
```

Backend on `localhost:8080`; run the agent and mobile against it via the
matching `api_url` / `--dart-define` values.

---

## 7. Fixes applied in this work session (context for future me)

All of these are committed. They took DeskSync from "connects then crashes / no
video" to "connects and streams."

1. **Mobile crash on connect — null `sdpMid` (root cause).**
   `RTCIceCandidate` was built with `sdpMid = null`; the device's `org.webrtc`
   build NPEs in `JniHelper.getStringBytes` and Android force-crashed the app
   ~1.5 s after connecting. Fix: never pass a null mid — fall back to the m-line
   index string; also plumb `sdp_mid` through the signaling payload.
   *(`mobile/.../signal_envelope.dart`, `webrtc_session.dart`)*

2. **No video — frames exceeded the data channel max message size.**
   Whole JPEGs (~100 KB) are larger than the SCTP max message size, so every
   send failed (`outbound packet larger than maximum message size`). Fix:
   **chunk** each frame into 16 KiB pieces with an 8-byte header
   `[frame_id u32][chunk_index u16][chunk_count u16]` on the agent, and
   **reassemble** on the phone (chunks copied out of the reused buffer).
   *(`desktop-agent/crates/media/src/rtc.rs`, `mobile/.../webrtc_session.dart`)*

3. **Terrible throughput (~0.5 fps) — CPU storm from stale sessions.**
   The agent answered many stale sessions and JPEG-encoded the screen for each
   one at full frame rate before discarding it. Fix: `frames_open()` check skips
   the encode entirely for sessions with no viewer attached.
   *(`desktop-agent/crates/media/src/rtc.rs`, `crates/config-ui/src/session_runtime.rs`)*

4. **Answer-loop churn.** The agent re-answered the same session every poll,
   causing peer leave/rejoin that tore down the call. Fix: answer each
   `session_id` at most once (bounded set).
   *(`crates/config-ui/src/session_runtime.rs`)*

5. **Mobile frame-render memory blow-up.** `Image.memory` routed every frame
   through Flutter's `ImageCache` and exhausted memory. Fix: decode to a
   `ui.Image`, hold only the latest, dispose the previous after paint, render
   with a `CustomPainter`. *(`webrtc_session.dart`, `desktop_viewer_screen.dart`)*

6. **Signaling robustness.** Serialized signal handling, buffered remote ICE
   until the answer is applied, guarded duplicate offers/answers.

### Diagnostics left in place
- Agent logs `frame stream stats {sent, dropped, last_frame_bytes}` every 30
  frames (set `DESKSYNC_LOG=...desksync_media=trace` for per-drop reasons).
- To read a phone crash/log: `adb logcat -b crash -b main -v threadtime`.

---

## 8. Troubleshooting quick table

| Symptom | Likely cause | Check / fix |
|---------|--------------|-------------|
| Device shows **offline** on phone | agent not running / not authenticated | `desksync status` — it names the reason |
| Works a while, then **`session expired`** every tick | (fixed) a repeat refresh — from a second task, a retry after a dropped response, or another device — was read as theft, and the response revoked *every* token on the account | fixed by single-flight rotation, per-session token families, and a 1m retry grace; needs migration 000007 deployed. On an old build: `login` again |
| App **closes** right after Connect | (old bug) null `sdpMid` | ensure app ≥ v1.0.3 |
| Connected but **blank screen** | frames not sent | agent log: `frame stream stats sent:>0`? if `sent:0`, check `frames_open`/channel |
| Blank screen, `sent` climbing | macOS **Screen Recording** off → black frames | grant permission, restart agent |
| Very **low fps** | CPU storm / stale sessions | confirm `frames_open` skip is deployed; restart agent |
| ICE fails, signaling OK | **NSG / TURN** ports closed | open `3478` + `50000-50100/udp` in Azure NSG |
| APK download stalls on phone | plain HTTP via carrier | use the Cloudflare HTTPS tunnel URL |
| "app not installed" | same versionCode already installed | bump `pubspec.yaml` `version:` and rebuild |

---

## 9. Resume checklist (fast path)

1. **Backend up?** `ssh apollo@20.109.60.233` → `sudo k3s kubectl get pods -n desksync` (all `Running`).
2. **Agent up on Mac?** `cd desktop-agent && cargo build -p desksync-agent --features native`, run it, then `desksync-agent status` — one command answers signed-in, device id, permissions, frames, and last error.
3. **Anything missing?** `desksync-agent setup` fixes it in order and says what it could not.
4. **Phone:** install latest APK (§4), log in, tap Connect.
5. Watch agent `frame stream stats sent:… dropped:…` to confirm streaming.
