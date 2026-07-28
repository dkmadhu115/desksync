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

export DESKSYNC_EMAIL="dkmadhu2109@gmail.com"
export DESKSYNC_PASSWORD="<account-password>"
export RUST_LOG="info,desksync_media=trace,desksync_agent=info,webrtc_ice=warn"

./target/debug/desksync-agent
```

Healthy startup logs:

```
agent running; press Ctrl-C to stop
heartbeat: authenticated; reporting presence
session runtime ready; watching for incoming sessions
capture pipeline produced first frame  width=... height=...
```

When a phone connects you should see:

```
frames data channel established
frame stream stats  sent:30  dropped:0   ← frames flowing (dropped≈0 is good)
```

### Signing in (Google, via the browser)

```bash
./target/debug/desksync-agent login       # opens the browser for Google sign-in
./target/debug/desksync-agent login --password   # email/password from env (CI/headless)
./target/debug/desksync-agent logout      # clears stored credentials
```

`login` stores the access token, refresh token, and device id in the **OS
keychain** (macOS Keychain / Windows Credential Manager / Linux Secret Service) on
`--features native` builds, or an owner-only `secrets.json` otherwise. Nothing
sensitive is written to `config.json`.

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
`device_id` to `unregistered`, and restart (it re-registers on next auth).

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
  frames (set `RUST_LOG=...desksync_media=trace` for per-drop reasons).
- To read a phone crash/log: `adb logcat -b crash -b main -v threadtime`.

---

## 8. Troubleshooting quick table

| Symptom | Likely cause | Check / fix |
|---------|--------------|-------------|
| Device shows **offline** on phone | agent not running / not authenticated | start agent; look for `heartbeat: authenticated` |
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
2. **Agent up on Mac?** `cd desktop-agent && cargo build -p desksync-agent --features native` then run with env vars (§3). Confirm `authenticated`.
3. **macOS Screen Recording** enabled for the launching app.
4. **Phone:** install latest APK (§4), log in, tap Connect.
5. Watch agent `frame stream stats sent:… dropped:…` to confirm streaming.
