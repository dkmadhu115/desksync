# Mobile app design (Flutter)

The mobile client lets a developer control their laptop. It is a Flutter app
using Riverpod for state, GoRouter for navigation, and Dio for HTTP. Phase 4
delivers authentication, the device list, pairing (manual-code), and the remote
viewer with full touch/keyboard controls. The live video and data channel arrive
in Phase 5; QR scanning and the trust handshake in Phase 6.

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
- Pairing implements manual-code confirmation against the backend contract
  (`pairing_id` + 8-digit `code` + a persistent local `mobile_device_id`). QR
  scanning is a visible entry point wired in Phase 6.

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
- `application/input_sink.dart` — `InputSink` interface. Today a
  `LoggingInputSink`; in Phase 5 the WebRTC data-channel sink replaces the
  provider override with no other code change.
- The viewer offers pointer vs scroll gesture modes, left/right-click taps,
  long-press right-click, and a keyboard capture bar (diffing typed text into
  key events, plus Enter/Backspace).

## Testing

`flutter analyze` is clean and `flutter test` covers: auth flows (login,
register validation, bootstrap, refresh-expiry, logout), the device controller
(load/error/optimistic-remove), the input-event wire format, touch mapping, the
HID key table, the input controller dispatch, and an app-boot widget smoke test.
Fakes live in `test/support/fakes.dart` (in-memory storage, fake APIs, recording
input sink).
