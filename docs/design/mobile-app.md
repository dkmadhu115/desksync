# Mobile app design (Flutter)

The mobile client lets a developer control their laptop. It is a Flutter app
using Riverpod for state, GoRouter for navigation, and Dio for HTTP. Phase 4
delivers authentication, the device list, pairing, and the remote viewer with
full touch/keyboard controls. Phase 5 adds the WebRTC connection plane (below).
Phase 6 adds device self-registration and QR-code pairing (the trust handshake).
Phase 7 wires the live remote desktop into the viewer: it resolves the device's
active pairing, creates a session, drives the `WebRtcSession`, renders the live
video (`RTCVideoView`), and routes touch/keyboard/clipboard input to the desktop
over the data channel.

## WebRTC connection plane (Phase 5)

The controller (mobile) is the WebRTC **offerer**. `session/` creates a session
via `POST /api/v1/sessions` and receives the signaling URL + ticket and ICE
servers (`SessionCreated`). `signaling/` holds the wire protocol
(`SignalEnvelope` mirroring the backend and Rust agent) and a `SignalingClient`
over `dart:io` `WebSocket` (monotonic nonce, heartbeats, decoded message
stream). `viewer/application/webrtc_session.dart` orchestrates the
`RTCPeerConnection`: it opens a reliable `input` data channel, adds a
`recvonly` video transceiver, waits for the agent's `peer_joined`, then
exchanges the offer/answer and trickled ICE. Outgoing input flows through a
`DataChannelInputSink` (the same `InputSink` interface the viewer already
uses); an `AdaptiveBitrateController` (loss-based AIMD, mirroring the agent)
guides quality. The pure pieces — session/signaling models, the input sink, the
signaling client, and adaptive bitrate — are unit-tested; the `flutter_webrtc`
peer itself is exercised in device/end-to-end testing.

## Architecture

Feature-first, with a clean layering inside each feature so the UI never talks
to HTTP directly:

```
lib/
  core/
    config/env.dart              # --dart-define configuration
    network/
      dio_client.dart            # Dio provider + session-expiry signal
      auth_interceptor.dart      # bearer attach + rotation-safe refresh
      api_exception.dart         # typed, user-presentable errors
    storage/secure_storage.dart  # SecureStore interface + Keychain impl
    util/uuid.dart
  features/<feature>/
    domain/        # models (Device, Pairing, TokenPair, InputEvent, ...)
    data/          # <feature>_api.dart (HTTP) + <feature>_repository.dart
    application/   # Riverpod controllers (Notifier / AsyncNotifier)
    presentation/  # screens + widgets
  app/             # App root, theme, router (+ auth redirect), splash
```

Dependencies flow **presentation → application → data → domain**. Every layer
is behind a provider, so tests override the API/sink/storage with fakes and
exercise the real controllers and repositories.

## Authentication & session lifecycle

- Email/password login and registration call the auth service and persist the
  `TokenPair` in `flutter_secure_storage` (Keychain / Keystore).
- `AuthInterceptor` attaches the bearer token and, on `401`, performs a
  **single-flight, rotation-safe refresh**: concurrent 401s share one refresh
  future, and a request whose token was already rotated simply retries with the
  new token instead of refreshing again (which would trip the backend's refresh
  reuse/theft detection). On refresh failure it clears tokens and signals
  session expiry, which routes the user back to login.
- On launch the app bootstraps auth from stored tokens; the router shows a
  splash until the status is known, then gates every route on `AuthStatus`.

## Devices & pairing

- The device list is an `AsyncNotifier` with loading/error/data states,
  pull-to-refresh, and optimistic revoke (rolls back on failure). Only online
  desktops are tappable (they open the viewer).
- Pairing supports both **QR scanning** (`mobile_scanner`) and manual-code entry
  against the backend contract. The scanner parses the `desksync://pair?...` deep
  link (`PairingLink`, a pure/tested parser) into `pairing_id` + `code` and
  confirms immediately.
- `DeviceIdentity` registers this phone as a `mobile` device on first pairing
  (generating and persisting a device key), then caches the server-assigned id.
  That id is the `mobile_device_id` sent on confirm, so the pairing satisfies the
  backend's device foreign keys. The uploaded public key is a placeholder until
  the real X25519 identity lands with E2E encryption (Phase 9).

## Touch controls (the input pipeline)

The viewer converts gestures/keystrokes into `InputEvent`s whose JSON **exactly
matches the Rust agent's `serde` wire format** (`type` discriminator in
snake_case, lowercase button names, HID usage codes for keys). This contract is
locked by unit tests (`test/input_event_test.dart`).

- `domain/touch_mapping.dart` — pure geometry: normalizes a touch position
  within the surface to `[0,1]` (clamped), and builds click/move/drag/scroll
  event sequences. Fully unit-tested without the widget layer.
- `domain/key_codes.dart` — maps characters to HID usage codes (with Shift),
  matching the agent's decode table.
- `application/input_controller.dart` — a `Notifier` that dispatches events to
  the `InputSink` and counts them.
- `application/input_sink.dart` — the `InputSink` interface plus a
  `SwitchableInputSink`: the pipeline always dispatches through one stable sink
  whose destination is swapped at runtime. The viewer **attaches** the live
  data-channel sink on connect and **detaches** on teardown; until then events
  fall back to logging and are counted as dropped. This decouples
  `InputController`/widgets from the WebRTC lifecycle (unit-tested).
- The viewer offers pointer vs scroll gesture modes, left/right-click taps,
  long-press right-click, a keyboard capture bar (diffing typed text into key
  events, plus Enter/Backspace), and a **send-clipboard** action that pushes the
  phone's clipboard to the desktop as a `clipboard_text` event.

## Viewer connection lifecycle (Phase 7)

`viewer/application/viewer_controller.dart` (`ViewerController`, a
`ChangeNotifier`) orchestrates one connection for a device:

1. **resolve** the device's active pairing — `PairingApi.list()` +
   `selectActivePairing` (pure, unit-tested; prefers the newest `active`
   pairing). No pairing → a "pair this device" prompt.
2. **create** a session for that pairing (`SessionApi.create`).
3. **connect** — build the `WebRtcSession`, `start()` it, then attach its
   data-channel sink to the shared `SwitchableInputSink`.

The UI renders per phase (`resolving`/`connecting`/`connected`/`noPairing`/
`failed`/`closed`): a status overlay with retry while establishing, and the live
`RTCVideoView` once connected. Pairing resolution, session creation, and the
failure/no-pairing branches are unit-tested via injected callbacks; the
`flutter_webrtc` peer is exercised on real devices.

## Developer Quick Launch (Phase 8)

`features/devtools/` lets the user trigger workstation actions on the connected
desktop — launch editors (VS Code/Cursor/Claude) or terminals, run curated
Git/Docker/kubectl/Helm shortcuts, and SSH into saved hosts.

- `domain/dev_action.dart` mirrors the agent's closed wire contract (flattened
  `action` discriminator, snake_case enums, **id-only** references to workspaces
  and hosts — never raw paths or commands). `domain/dev_catalog.dart` mirrors the
  agent's shortcut catalog for the UI.
- Actions flow over a second, reliable **`control`** data channel (opened by
  `WebRtcSession` alongside `input`) so they never block latency-sensitive input.
  A `SwitchableControlSink` (same pattern as the input sink) is attached by the
  `ViewerController` on connect and detached on teardown.
- `DevActionController` assigns a correlation id, serializes, and dispatches;
  `presentation/quick_launch_screen.dart` (reached from the viewer app bar)
  renders the editors/terminals/shortcuts/SSH form.

Shortcuts that need a workspace are disabled until a workspace id is entered. The
agent re-validates every request against its allowlist; advertising the real
registries and streaming command output back rides on the native peer's control
receive path. Unit tests cover the wire serialization, the switchable control
sink, and the controller dispatch/count.

## Security hardening (Phase 9)

- `features/security/secure_channel.dart` (`SecureChannel`) is the end-to-end
  crypto layer: X25519 ECDH → HKDF-SHA256 → AES-256-GCM with per-direction keys
  and counter-based replay protection. It is the byte-exact mirror of the
  agent's `desksync-crypto` crate — a shared interop vector (fixed shared
  secret/session id/public keys) asserts identical derived keys and an identical
  sealed frame on both sides, and the Dart tests also open a Rust-produced
  frame. See [ADR 0009](../adr/0009-e2e-secure-channel.md).
- `core/network/certificate_pinning.dart` (`CertificatePinner`) adds fail-closed
  TLS leaf-certificate pinning to the Dio client (configured via
  `DESKSYNC_CERT_PINS`), using the same base64(SHA-256(DER)) pin format as the
  agent's `CertPinner`.

## Testing

`flutter analyze` is clean and `flutter test` covers: auth flows (login,
register validation, bootstrap, refresh-expiry, logout), the device controller
(load/error/optimistic-remove), the input-event wire format, touch mapping, the
HID key table, the input controller dispatch, and an app-boot widget smoke test.
Fakes live in `test/support/fakes.dart` (in-memory storage, fake APIs, recording
input sink).
