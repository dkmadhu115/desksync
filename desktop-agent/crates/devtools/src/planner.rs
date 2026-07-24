//! Turns a validated [`DevActionRequest`] into a concrete [`CommandSpec`].
//!
//! This is the single choke point where client intent becomes an executable
//! command. It resolves every id reference against the registries and the
//! shortcut catalog, and returns a typed error for anything unknown. Because it
//! only ever emits commands built from the closed model, it cannot produce an
//! arbitrary command.

use crate::error::{DevToolsError, Result};
use crate::launch;
use crate::model::{CommandSpec, DevActionKind, DevActionRequest};
use crate::registry::{SshHostRegistry, WorkspaceRegistry};
use crate::shortcuts;

/// Plan a command for `req` against the given registries and target `os`.
pub fn plan(
    req: &DevActionRequest,
    workspaces: &WorkspaceRegistry,
    hosts: &SshHostRegistry,
    os: &str,
) -> Result<CommandSpec> {
    match &req.kind {
        DevActionKind::LaunchEditor { editor, workspace_id } => {
            let path = resolve_optional(workspaces, workspace_id.as_deref())?;
            launch::resolve_editor(*editor, os, path.as_deref())
        }
        DevActionKind::OpenTerminal { terminal, workspace_id } => {
            let path = resolve_optional(workspaces, workspace_id.as_deref())?;
            launch::resolve_terminal(*terminal, os, path.as_deref())
        }
        DevActionKind::RunShortcut {
            tool,
            shortcut_id,
            workspace_id,
        } => {
            let shortcut = shortcuts::find(*tool, shortcut_id).ok_or_else(|| DevToolsError::UnknownShortcut {
                tool: format!("{tool:?}"),
                shortcut: shortcut_id.clone(),
            })?;

            let cwd = if shortcut.needs_workspace {
                let id = workspace_id.as_deref().ok_or_else(|| {
                    DevToolsError::invalid("shortcut", format!("'{}' requires a workspace", shortcut.id))
                })?;
                Some(workspaces.resolve_path(id)?)
            } else {
                resolve_optional(workspaces, workspace_id.as_deref())?
            };

            Ok(CommandSpec::captured(shortcut.program, shortcut.args_owned(), cwd))
        }
        DevActionKind::SshConnect { host_id, terminal } => {
            let host = hosts.resolve(host_id)?;
            launch::resolve_ssh(&host.destination(), host.port, *terminal, os)
        }
    }
}

fn resolve_optional(workspaces: &WorkspaceRegistry, workspace_id: Option<&str>) -> Result<Option<String>> {
    match workspace_id {
        Some(id) => workspaces.resolve_path(id).map(Some),
        None => Ok(None),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::launch::{LINUX, MACOS};
    use crate::model::{Editor, SshHost, Terminal, Tool, Workspace};

    fn workspaces() -> WorkspaceRegistry {
        WorkspaceRegistry::from_items(vec![Workspace {
            id: "ws1".into(),
            name: "App".into(),
            path: "/home/dev/app".into(),
        }])
        .unwrap()
    }

    fn hosts() -> SshHostRegistry {
        SshHostRegistry::from_items(vec![SshHost {
            id: "prod".into(),
            label: "Prod".into(),
            user: "deploy".into(),
            host: "10.0.0.1".into(),
            port: 22,
        }])
        .unwrap()
    }

    fn req(kind: DevActionKind) -> DevActionRequest {
        DevActionRequest {
            request_id: "r".into(),
            kind,
        }
    }

    #[test]
    fn editor_with_known_workspace_resolves_path() {
        let spec = plan(
            &req(DevActionKind::LaunchEditor {
                editor: Editor::VsCode,
                workspace_id: Some("ws1".into()),
            }),
            &workspaces(),
            &hosts(),
            MACOS,
        )
        .unwrap();
        assert_eq!(spec.program, "code");
        assert_eq!(spec.args, vec!["/home/dev/app"]);
    }

    #[test]
    fn unknown_workspace_is_rejected() {
        let err = plan(
            &req(DevActionKind::OpenTerminal {
                terminal: Terminal::AppleTerminal,
                workspace_id: Some("nope".into()),
            }),
            &workspaces(),
            &hosts(),
            MACOS,
        )
        .unwrap_err();
        assert_eq!(err, DevToolsError::UnknownWorkspace("nope".into()));
    }

    #[test]
    fn shortcut_needing_workspace_requires_id() {
        let err = plan(
            &req(DevActionKind::RunShortcut {
                tool: Tool::Git,
                shortcut_id: "status".into(),
                workspace_id: None,
            }),
            &workspaces(),
            &hosts(),
            LINUX,
        )
        .unwrap_err();
        assert!(matches!(err, DevToolsError::Invalid { kind: "shortcut", .. }));
    }

    #[test]
    fn global_shortcut_runs_without_workspace() {
        let spec = plan(
            &req(DevActionKind::RunShortcut {
                tool: Tool::Docker,
                shortcut_id: "ps".into(),
                workspace_id: None,
            }),
            &workspaces(),
            &hosts(),
            LINUX,
        )
        .unwrap();
        assert_eq!(spec.program, "docker");
        assert_eq!(spec.args, vec!["ps"]);
        assert!(spec.capture_output);
        assert!(spec.cwd.is_none());
    }

    #[test]
    fn unknown_shortcut_is_rejected() {
        let err = plan(
            &req(DevActionKind::RunShortcut {
                tool: Tool::Git,
                shortcut_id: "push".into(),
                workspace_id: Some("ws1".into()),
            }),
            &workspaces(),
            &hosts(),
            LINUX,
        )
        .unwrap_err();
        assert!(matches!(err, DevToolsError::UnknownShortcut { .. }));
    }

    #[test]
    fn ssh_connect_resolves_host() {
        let spec = plan(
            &req(DevActionKind::SshConnect {
                host_id: "prod".into(),
                terminal: Terminal::AppleTerminal,
            }),
            &workspaces(),
            &hosts(),
            LINUX,
        )
        .unwrap();
        assert_eq!(spec.program, "ssh");
        assert_eq!(spec.args, vec!["deploy@10.0.0.1"]);
    }

    #[test]
    fn ssh_connect_unknown_host_rejected() {
        let err = plan(
            &req(DevActionKind::SshConnect {
                host_id: "ghost".into(),
                terminal: Terminal::AppleTerminal,
            }),
            &workspaces(),
            &hosts(),
            LINUX,
        )
        .unwrap_err();
        assert_eq!(err, DevToolsError::UnknownHost("ghost".into()));
    }
}
