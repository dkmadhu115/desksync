//! Wire model for developer quick-launch actions.
//!
//! These types are the contract between the mobile client and the agent. They
//! are deliberately *closed*: the phone can only ask for actions from a fixed
//! set of editors, terminals, and tool shortcuts, and can only reference
//! workspaces/hosts by **id** (resolved against a validated registry on the
//! agent). There is no field anywhere that carries a raw path, host, or command
//! string, so a compromised or malicious client cannot ask the agent to run
//! arbitrary programs.

use serde::{Deserialize, Serialize};

/// GUI code editors that can be launched.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Editor {
    /// Visual Studio Code (`code` CLI).
    VsCode,
    /// Cursor (`cursor` CLI).
    Cursor,
    /// Claude Desktop.
    Claude,
}

/// Terminal emulators that can be launched.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Terminal {
    /// Warp.
    Warp,
    /// macOS Terminal.app.
    AppleTerminal,
    /// iTerm2.
    ITerm,
    /// PowerShell.
    PowerShell,
    /// Windows Terminal.
    WindowsTerminal,
}

/// Developer CLIs that expose curated shortcuts.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Tool {
    /// Git.
    Git,
    /// Docker / Docker Compose.
    Docker,
    /// Kubernetes CLI.
    Kubectl,
    /// Helm.
    Helm,
}

/// A requested developer action. Each variant is a closed command with only
/// id-references to registered resources.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "action", rename_all = "snake_case")]
pub enum DevActionKind {
    /// Launch an editor, optionally opening a registered workspace.
    LaunchEditor {
        /// Which editor.
        editor: Editor,
        /// Optional registered workspace id to open.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        workspace_id: Option<String>,
    },
    /// Open a terminal, optionally `cd`'d into a registered workspace.
    OpenTerminal {
        /// Which terminal.
        terminal: Terminal,
        /// Optional registered workspace id to open in.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        workspace_id: Option<String>,
    },
    /// Run a built-in shortcut for a tool (e.g. `git status`), optionally in a
    /// registered workspace directory.
    RunShortcut {
        /// Which tool.
        tool: Tool,
        /// Built-in shortcut id (validated against the catalog).
        shortcut_id: String,
        /// Optional registered workspace id used as the working directory.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        workspace_id: Option<String>,
    },
    /// Open an SSH session to a registered host in a terminal.
    SshConnect {
        /// Registered host id (resolved to user@host:port on the agent).
        host_id: String,
        /// Which terminal to open the session in.
        terminal: Terminal,
    },
}

/// A dev-action request carrying a client correlation id.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct DevActionRequest {
    /// Client-generated id used to correlate the [`DevActionResult`].
    pub request_id: String,
    /// The action to perform.
    #[serde(flatten)]
    pub kind: DevActionKind,
}

/// Outcome status of a dev action.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum DevActionStatus {
    /// The action was launched / completed successfully.
    Ok,
    /// The action was rejected or failed.
    Error,
}

/// The result of a dev action, echoed back to the client.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct DevActionResult {
    /// Correlates with [`DevActionRequest::request_id`].
    pub request_id: String,
    /// Whether the action succeeded.
    pub status: DevActionStatus,
    /// Human-readable detail (error reason or a short success note).
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub message: String,
    /// Captured command output for shortcuts (truncated), when applicable.
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub output: String,
}

impl DevActionResult {
    /// Build a success result.
    pub fn ok(request_id: impl Into<String>, message: impl Into<String>) -> Self {
        Self {
            request_id: request_id.into(),
            status: DevActionStatus::Ok,
            message: message.into(),
            output: String::new(),
        }
    }

    /// Build a success result carrying captured output.
    pub fn ok_output(request_id: impl Into<String>, output: impl Into<String>) -> Self {
        Self {
            request_id: request_id.into(),
            status: DevActionStatus::Ok,
            message: String::new(),
            output: output.into(),
        }
    }

    /// Build an error result.
    pub fn error(request_id: impl Into<String>, message: impl Into<String>) -> Self {
        Self {
            request_id: request_id.into(),
            status: DevActionStatus::Error,
            message: message.into(),
            output: String::new(),
        }
    }
}

/// A fully-resolved, shell-free command to execute. Built only by the planner
/// from the closed action model; never constructed from client input directly.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CommandSpec {
    /// Program to execute (looked up on PATH by the OS).
    pub program: String,
    /// Arguments, passed verbatim (no shell interpolation).
    pub args: Vec<String>,
    /// Optional working directory.
    pub cwd: Option<String>,
    /// Whether to capture and return stdout/stderr (true for shortcuts) or run
    /// fire-and-forget (false for GUI launches).
    pub capture_output: bool,
}

impl CommandSpec {
    /// A fire-and-forget GUI launch command.
    pub fn launch(program: impl Into<String>, args: Vec<String>) -> Self {
        Self {
            program: program.into(),
            args,
            cwd: None,
            capture_output: false,
        }
    }

    /// A command whose output is captured (tool shortcuts).
    pub fn captured(program: impl Into<String>, args: Vec<String>, cwd: Option<String>) -> Self {
        Self {
            program: program.into(),
            args,
            cwd,
            capture_output: true,
        }
    }

    /// Attach a working directory.
    pub fn with_cwd(mut self, cwd: Option<String>) -> Self {
        self.cwd = cwd;
        self
    }
}

/// A saved workspace (project directory) the user has explicitly registered.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Workspace {
    /// Stable id referenced by the client.
    pub id: String,
    /// Human-friendly name.
    pub name: String,
    /// Absolute directory path on the host.
    pub path: String,
}

/// A saved SSH host the user has explicitly registered.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SshHost {
    /// Stable id referenced by the client.
    pub id: String,
    /// Human-friendly label.
    pub label: String,
    /// SSH user.
    pub user: String,
    /// Hostname or IP.
    pub host: String,
    /// Port (defaults to 22).
    #[serde(default = "default_ssh_port")]
    pub port: u16,
}

fn default_ssh_port() -> u16 {
    22
}

impl SshHost {
    /// The `user@host` destination string for the ssh CLI.
    pub fn destination(&self) -> String {
        format!("{}@{}", self.user, self.host)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn request_roundtrips_with_flattened_kind() {
        let req = DevActionRequest {
            request_id: "r1".into(),
            kind: DevActionKind::LaunchEditor {
                editor: Editor::VsCode,
                workspace_id: Some("ws1".into()),
            },
        };
        let json = serde_json::to_string(&req).unwrap();
        assert!(json.contains("\"action\":\"launch_editor\""));
        assert!(json.contains("\"editor\":\"vs_code\""));
        let back: DevActionRequest = serde_json::from_str(&json).unwrap();
        assert_eq!(req, back);
    }

    #[test]
    fn shortcut_request_parses() {
        let json = r#"{"request_id":"r2","action":"run_shortcut","tool":"git","shortcut_id":"status"}"#;
        let req: DevActionRequest = serde_json::from_str(json).unwrap();
        assert_eq!(
            req.kind,
            DevActionKind::RunShortcut {
                tool: Tool::Git,
                shortcut_id: "status".into(),
                workspace_id: None,
            }
        );
    }

    #[test]
    fn ssh_host_defaults_port_and_builds_destination() {
        let json = r#"{"id":"h1","label":"prod","user":"deploy","host":"10.0.0.1"}"#;
        let host: SshHost = serde_json::from_str(json).unwrap();
        assert_eq!(host.port, 22);
        assert_eq!(host.destination(), "deploy@10.0.0.1");
    }

    #[test]
    fn result_omits_empty_optional_fields() {
        let json = serde_json::to_string(&DevActionResult::ok("r", "launched")).unwrap();
        assert!(!json.contains("output"));
        assert!(json.contains("\"status\":\"ok\""));
    }
}
