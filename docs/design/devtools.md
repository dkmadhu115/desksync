# Developer features (Quick Launch & tool shortcuts)

Phase 8 lets a developer drive their workstation from the phone: launch editors
and terminals, open saved workspaces, run curated tool shortcuts, and SSH into
saved hosts. This document covers the agent engine (`desksync-devtools`), the
control transport, and the mobile Quick Launch feature.

See also [ADR 0008](../adr/0008-devtools-allowlist.md) for the security
rationale (why this is an allowlist and not remote command execution).

## Threat model in one line

The phone (and anything that can reach the signaling path) must **never** be able
to run an arbitrary command on the developer's machine. Every design choice below
follows from that.

## Agent engine — `desksync-devtools`

Pure, layered, and unit-tested; only the runner touches the OS.

| Module | Responsibility |
|--------|----------------|
| `model` | Closed wire types: `Editor`/`Terminal`/`Tool` enums, `DevActionKind` (tagged), `DevActionRequest`/`Result`, `CommandSpec`, `Workspace`, `SshHost`. |
| `registry` | `WorkspaceRegistry` + `SshHostRegistry`: id → path/host, validated, fail-closed. |
| `shortcuts` | Compile-time `CATALOG` of `(tool, id) → program + fixed args`. |
| `launch` | OS-aware resolution of editor/terminal/ssh launches into `CommandSpec`s. |
| `planner` | The single choke point: validated request → `CommandSpec`. |
| `runner` | `CommandRunner` trait + `TokioCommandRunner` (shell-free spawn). |
| `service` | `DevToolsService`: validate → plan → run → structured result; metrics. |

### The closed model

The phone can only pick from fixed enums and reference workspaces/hosts by **id**:

```jsonc
// launch VS Code in a saved workspace
{ "request_id": "…", "action": "launch_editor", "editor": "vs_code", "workspace_id": "api" }
// run a curated shortcut
{ "request_id": "…", "action": "run_shortcut", "tool": "git", "shortcut_id": "status", "workspace_id": "api" }
// ssh into a saved host, in a terminal
{ "request_id": "…", "action": "ssh_connect", "host_id": "prod", "terminal": "apple_terminal" }
```

There is no field for a raw path, host, or command string anywhere in the model.

### Registries (the only source of paths/hosts)

`WorkspaceRegistry` and `SshHostRegistry` are populated out-of-band from
`<config-dir>/workspaces.json` and `<config-dir>/ssh_hosts.json` — never by the
phone. Entries are validated (workspace paths must be absolute; ssh `user`/`host`
must not contain whitespace or control characters) and an invalid file fails
closed to an empty registry, so a corrupt config can never widen the allowlist.

### Shortcut catalog

Tool shortcuts are compile-time constants — `(tool, id) → program + fixed args` —
so the client supplies no arguments. The set is read-mostly:

- **Git** (need a workspace): `status`, `fetch`, `pull`, `log`, `branches`.
- **Docker**: `ps`, `images` (global); `compose_up`, `compose_down`, `compose_ps`
  (need a workspace).
- **kubectl**: `pods`, `services`, `nodes`, `contexts`.
- **Helm**: `list`.

### Execution

`TokioCommandRunner` spawns the program directly with an explicit argv — no shell,
so arguments cannot be reinterpreted. GUI launches are detached and
fire-and-forget; shortcuts run to completion under a timeout (default 20s) and
their combined stdout/stderr is captured and truncated (16 KiB) into the result.
Non-zero exits become a structured error. Malformed control frames are counted
and dropped, never disturbing the loop.

## Transport — the `control` data channel

Dev actions ride a dedicated, reliable, ordered **`control`** WebRTC data channel,
separate from the latency-sensitive `input` channel so control payloads never
block pointer/key frames. On the agent the native peer dispatches each frame to
`DevToolsService::handle_frame` (mirroring the Phase 7 `InputRouter`).

## Mobile — Quick Launch

- `domain/dev_action.dart` mirrors the wire contract (flattened `action`
  discriminator, snake_case enums, id-only references).
- `domain/dev_catalog.dart` mirrors the shortcut catalog to render the UI; the
  agent remains the source of truth and re-validates every request.
- `application/control_sink.dart` provides `SwitchableControlSink` (same pattern
  as the input pipeline): the UI always dispatches through a stable sink, and the
  live data-channel target is attached by the `ViewerController` on connect and
  detached on teardown.
- `application/dev_action_controller.dart` assigns a correlation id, serializes,
  and sends; state is the count of dispatched actions.
- `presentation/quick_launch_screen.dart` offers editors, terminals, tool
  shortcuts (grouped, disabled when they need a workspace and none is set), and an
  SSH connect form. Reached from the viewer app bar's **Quick Launch** button so
  it operates within the connected session.

## Not yet wired (native peer follow-up)

The return path — advertising the agent's real workspace/host registries to the
phone and streaming shortcut output back — rides on the native WebRTC peer's
`control` receive channel, which lands with the media plane. Until then the phone
dispatches fire-and-forget and references ids the user configured.
