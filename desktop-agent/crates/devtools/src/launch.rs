//! OS-aware resolution of editor/terminal launches into [`CommandSpec`]s.
//!
//! Pure functions keyed on the target OS string (`std::env::consts::OS`) so the
//! full launch matrix is unit-tested without spawning anything. Unsupported
//! editor/terminal/OS combinations produce a typed [`DevToolsError::Unsupported`]
//! rather than a broken command.

use crate::error::{DevToolsError, Result};
use crate::model::{CommandSpec, Editor, Terminal};

/// macOS OS string.
pub const MACOS: &str = "macos";
/// Windows OS string.
pub const WINDOWS: &str = "windows";
/// Linux OS string.
pub const LINUX: &str = "linux";

/// Resolve an editor launch for `os`, optionally opening `workspace_path`.
pub fn resolve_editor(editor: Editor, os: &str, workspace_path: Option<&str>) -> Result<CommandSpec> {
    let path_args = || workspace_path.map(|p| vec![p.to_string()]).unwrap_or_default();
    match editor {
        // VS Code and Cursor ship stable CLIs that accept a path and inherit
        // the caller's environment on every platform.
        Editor::VsCode => Ok(CommandSpec::launch("code", path_args())),
        Editor::Cursor => Ok(CommandSpec::launch("cursor", path_args())),
        Editor::Claude => match os {
            MACOS => {
                let mut args = vec!["-a".to_string(), "Claude".to_string()];
                args.extend(path_args());
                Ok(CommandSpec::launch("open", args))
            }
            LINUX => Ok(CommandSpec::launch("claude", Vec::new())),
            WINDOWS => Ok(CommandSpec::launch(
                "cmd",
                vec!["/c".into(), "start".into(), "".into(), "claude".into()],
            )),
            other => Err(DevToolsError::Unsupported(format!("Claude on {other}"))),
        },
    }
}

/// Resolve a terminal launch for `os`, optionally opening `workspace_path`.
pub fn resolve_terminal(terminal: Terminal, os: &str, workspace_path: Option<&str>) -> Result<CommandSpec> {
    match (terminal, os) {
        // macOS: `open -a <App> [dir]` opens a new window rooted at the dir.
        (Terminal::AppleTerminal, MACOS) => Ok(open_app("Terminal", workspace_path)),
        (Terminal::ITerm, MACOS) => Ok(open_app("iTerm", workspace_path)),
        (Terminal::Warp, MACOS) => Ok(open_app("Warp", workspace_path)),

        // Linux.
        (Terminal::Warp, LINUX) => Ok(CommandSpec::launch("warp-terminal", Vec::new()).with_cwd(owned(workspace_path))),
        (Terminal::PowerShell, LINUX) => Ok(CommandSpec::launch("pwsh", Vec::new()).with_cwd(owned(workspace_path))),

        // Windows: Windows Terminal opens in `-d <dir>`; PowerShell hosted in wt.
        (Terminal::WindowsTerminal, WINDOWS) => Ok(CommandSpec::launch("wt", wt_dir_args(workspace_path, None))),
        (Terminal::PowerShell, WINDOWS) => Ok(CommandSpec::launch(
            "wt",
            wt_dir_args(workspace_path, Some("powershell")),
        )),

        (t, os) => Err(DevToolsError::Unsupported(format!("{t:?} on {os}"))),
    }
}

/// Build an SSH launch: open a terminal that runs `ssh user@host [-p port]`.
///
/// The destination and port come from a validated [`crate::model::SshHost`], so
/// they are safe to place in argv (no shell is involved).
pub fn resolve_ssh(destination: &str, port: u16, terminal: Terminal, os: &str) -> Result<CommandSpec> {
    let mut ssh_args = vec![destination.to_string()];
    if port != 22 {
        ssh_args.push("-p".into());
        ssh_args.push(port.to_string());
    }
    match (terminal, os) {
        // macOS Terminal/iTerm can't take an inline command via `open`, so run
        // ssh directly; the OS opens it in the user's default terminal.
        (_, MACOS) => {
            let mut args = vec!["ssh".to_string()];
            args.extend(ssh_args);
            Ok(CommandSpec::launch(
                "open",
                prepend(vec!["-b".into(), "com.apple.Terminal".into()], args),
            ))
        }
        (Terminal::WindowsTerminal | Terminal::PowerShell, WINDOWS) => {
            let mut args = vec!["ssh".to_string()];
            args.extend(ssh_args);
            Ok(CommandSpec::launch("wt", args))
        }
        (_, LINUX) => {
            // Fall back to launching ssh directly (works headless and in most
            // terminal wrappers).
            Ok(CommandSpec::launch("ssh", ssh_args))
        }
        (t, os) => Err(DevToolsError::Unsupported(format!("ssh via {t:?} on {os}"))),
    }
}

fn open_app(app: &str, workspace_path: Option<&str>) -> CommandSpec {
    let mut args = vec!["-a".to_string(), app.to_string()];
    if let Some(p) = workspace_path {
        args.push(p.to_string());
    }
    CommandSpec::launch("open", args)
}

fn wt_dir_args(workspace_path: Option<&str>, trailing: Option<&str>) -> Vec<String> {
    let mut args = Vec::new();
    if let Some(p) = workspace_path {
        args.push("-d".to_string());
        args.push(p.to_string());
    }
    if let Some(t) = trailing {
        args.push(t.to_string());
    }
    args
}

fn owned(s: Option<&str>) -> Option<String> {
    s.map(|s| s.to_string())
}

fn prepend(mut head: Vec<String>, tail: Vec<String>) -> Vec<String> {
    head.extend(tail);
    head
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn vscode_uses_cli_with_optional_path() {
        let none = resolve_editor(Editor::VsCode, MACOS, None).unwrap();
        assert_eq!(none.program, "code");
        assert!(none.args.is_empty());

        let with = resolve_editor(Editor::Cursor, LINUX, Some("/home/dev/p")).unwrap();
        assert_eq!(with.program, "cursor");
        assert_eq!(with.args, vec!["/home/dev/p"]);
    }

    #[test]
    fn claude_is_os_specific() {
        assert_eq!(resolve_editor(Editor::Claude, MACOS, None).unwrap().program, "open");
        assert_eq!(resolve_editor(Editor::Claude, LINUX, None).unwrap().program, "claude");
        assert_eq!(resolve_editor(Editor::Claude, WINDOWS, None).unwrap().program, "cmd");
    }

    #[test]
    fn mac_terminal_opens_app_with_dir() {
        let spec = resolve_terminal(Terminal::AppleTerminal, MACOS, Some("/tmp/x")).unwrap();
        assert_eq!(spec.program, "open");
        assert_eq!(spec.args, vec!["-a", "Terminal", "/tmp/x"]);
        assert!(!spec.capture_output);
    }

    #[test]
    fn windows_terminal_uses_dir_flag() {
        let spec = resolve_terminal(Terminal::WindowsTerminal, WINDOWS, Some("C:\\proj")).unwrap();
        assert_eq!(spec.program, "wt");
        assert_eq!(spec.args, vec!["-d", "C:\\proj"]);
    }

    #[test]
    fn unsupported_combinations_error() {
        assert!(matches!(
            resolve_terminal(Terminal::WindowsTerminal, MACOS, None),
            Err(DevToolsError::Unsupported(_))
        ));
        assert!(matches!(
            resolve_terminal(Terminal::AppleTerminal, WINDOWS, None),
            Err(DevToolsError::Unsupported(_))
        ));
    }

    #[test]
    fn ssh_includes_port_only_when_non_default() {
        let default = resolve_ssh("deploy@10.0.0.1", 22, Terminal::AppleTerminal, LINUX).unwrap();
        assert_eq!(default.program, "ssh");
        assert_eq!(default.args, vec!["deploy@10.0.0.1"]);

        let custom = resolve_ssh("deploy@10.0.0.1", 2222, Terminal::AppleTerminal, LINUX).unwrap();
        assert_eq!(custom.args, vec!["deploy@10.0.0.1", "-p", "2222"]);
    }
}
