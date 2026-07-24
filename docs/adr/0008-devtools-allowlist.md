# 8. Allowlisted developer actions (no remote command execution)

- Status: Accepted
- Date: 2026-07-24

## Context

Phase 8 ("Developer Features / Quick Launch") lets a developer trigger
workstation actions from the phone: launch VS Code / Cursor / Claude, open a
terminal (Warp / Terminal / iTerm / PowerShell / Windows Terminal), open a saved
workspace, run Git/Docker/kubectl/Helm shortcuts, and SSH into a saved host.

The obvious-but-wrong design is a generic "run this command" RPC: the phone sends
a shell string and the agent executes it. That turns the paired phone (and the
signaling path) into a remote code execution vector on the developer's primary
machine. The spec is explicit: *never sacrifice security for simplicity*.

## Decision

The agent exposes a **closed allowlist**, not arbitrary execution. Concretely:

- **Closed wire model.** `DevActionKind` is a fixed enum (`launch_editor`,
  `open_terminal`, `run_shortcut`, `ssh_connect`). Editors, terminals, and tools
  are enums. No field anywhere carries a raw path, host, or command string.
- **Id-only references.** Workspaces and SSH hosts are referenced by **id** and
  resolved on the agent against registries (`WorkspaceRegistry`,
  `SshHostRegistry`) that are populated out-of-band by the user (config files),
  never by the phone. A compromised client cannot introduce a new path or host.
- **Fixed shortcut catalog.** Tool shortcuts come from a compile-time
  `shortcuts::CATALOG` of `(tool, id) → program + fixed args`. The client picks a
  shortcut by id; it can never supply arguments. The catalog is read-mostly (e.g.
  `git status`, `docker ps`, `kubectl get pods`, `helm list`).
- **Shell-free execution.** Commands run via `tokio::process` with an explicit
  argv (`TokioCommandRunner`); there is no shell, so an argument can never be
  reinterpreted as a command. GUI launches are detached and fire-and-forget;
  shortcuts run with a timeout and their (truncated) output is captured.
- **Fail-closed config.** Registry entries are validated (absolute paths;
  no whitespace/control characters in ssh user/host); an invalid registry starts
  empty rather than widening the allowlist.

The single choke point is `planner::plan`, which turns a validated request into a
`CommandSpec`. Every layer is pure and unit-tested; only the runner touches the
OS.

## Transport

Dev actions flow over a dedicated, reliable, ordered **`control`** WebRTC data
channel, separate from the latency-sensitive `input` channel, so control
payloads never block pointer/key frames. On the agent the native peer dispatches
each frame to `DevToolsService::handle_frame`, mirroring the Phase 7
`InputRouter`. The mobile client sends through a `SwitchableControlSink` attached
by the `ViewerController` while a session is connected.

## Consequences

- Adding a new shortcut is a code change (a new catalog entry), not a config
  toggle. This is intentional: the allowlist is auditable and cannot be widened
  from the phone or from a mutable config value.
- Result/registry sync back to the phone (so the UI can list the real workspaces
  and show command output) rides on the native WebRTC peer's `control` channel
  receive path, which lands with the media plane; until then the phone dispatches
  actions fire-and-forget and references ids the user configured.
- SSH is limited to registered hosts; there is deliberately no "ssh to an
  arbitrary host" action.
