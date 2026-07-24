//! The built-in, allowlisted catalog of tool shortcuts.
//!
//! Every runnable shortcut is defined here as a fixed program + argument list.
//! The client can only pick a shortcut by (tool, id); it can never supply
//! arguments. This is the allowlist that makes "run a dev command from your
//! phone" safe: the set of commands is closed and read-mostly, and none of them
//! take free-form input.

use crate::model::Tool;

/// A single catalog entry: a fixed command for a tool.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Shortcut {
    /// Stable id referenced by the client.
    pub id: &'static str,
    /// The tool this shortcut belongs to.
    pub tool: Tool,
    /// Program to run.
    pub program: &'static str,
    /// Fixed arguments.
    pub args: &'static [&'static str],
    /// Whether the shortcut must run inside a registered workspace directory
    /// (e.g. git commands need a repo).
    pub needs_workspace: bool,
    /// Short human description for the UI.
    pub description: &'static str,
}

impl Shortcut {
    /// The arguments as owned strings.
    pub fn args_owned(&self) -> Vec<String> {
        self.args.iter().map(|s| s.to_string()).collect()
    }
}

/// The complete built-in catalog. Ordered for stable UI listing.
pub const CATALOG: &[Shortcut] = &[
    // Git — all operate on a repository, so they require a workspace.
    Shortcut {
        id: "status",
        tool: Tool::Git,
        program: "git",
        args: &["status", "--short", "--branch"],
        needs_workspace: true,
        description: "Working tree status",
    },
    Shortcut {
        id: "fetch",
        tool: Tool::Git,
        program: "git",
        args: &["fetch", "--all", "--prune"],
        needs_workspace: true,
        description: "Fetch all remotes",
    },
    Shortcut {
        id: "pull",
        tool: Tool::Git,
        program: "git",
        args: &["pull", "--ff-only"],
        needs_workspace: true,
        description: "Fast-forward pull",
    },
    Shortcut {
        id: "log",
        tool: Tool::Git,
        program: "git",
        args: &["log", "--oneline", "-n", "20"],
        needs_workspace: true,
        description: "Last 20 commits",
    },
    Shortcut {
        id: "branches",
        tool: Tool::Git,
        program: "git",
        args: &["branch", "-vv"],
        needs_workspace: true,
        description: "List branches",
    },
    // Docker — ps/images are global; compose commands need the project dir.
    Shortcut {
        id: "ps",
        tool: Tool::Docker,
        program: "docker",
        args: &["ps"],
        needs_workspace: false,
        description: "Running containers",
    },
    Shortcut {
        id: "images",
        tool: Tool::Docker,
        program: "docker",
        args: &["images"],
        needs_workspace: false,
        description: "Local images",
    },
    Shortcut {
        id: "compose_up",
        tool: Tool::Docker,
        program: "docker",
        args: &["compose", "up", "-d"],
        needs_workspace: true,
        description: "Compose up (detached)",
    },
    Shortcut {
        id: "compose_down",
        tool: Tool::Docker,
        program: "docker",
        args: &["compose", "down"],
        needs_workspace: true,
        description: "Compose down",
    },
    Shortcut {
        id: "compose_ps",
        tool: Tool::Docker,
        program: "docker",
        args: &["compose", "ps"],
        needs_workspace: true,
        description: "Compose status",
    },
    // Kubectl — read-only cluster views.
    Shortcut {
        id: "pods",
        tool: Tool::Kubectl,
        program: "kubectl",
        args: &["get", "pods", "-A"],
        needs_workspace: false,
        description: "Pods (all namespaces)",
    },
    Shortcut {
        id: "services",
        tool: Tool::Kubectl,
        program: "kubectl",
        args: &["get", "svc", "-A"],
        needs_workspace: false,
        description: "Services (all namespaces)",
    },
    Shortcut {
        id: "nodes",
        tool: Tool::Kubectl,
        program: "kubectl",
        args: &["get", "nodes"],
        needs_workspace: false,
        description: "Cluster nodes",
    },
    Shortcut {
        id: "contexts",
        tool: Tool::Kubectl,
        program: "kubectl",
        args: &["config", "get-contexts"],
        needs_workspace: false,
        description: "Configured contexts",
    },
    // Helm.
    Shortcut {
        id: "list",
        tool: Tool::Helm,
        program: "helm",
        args: &["list", "--all-namespaces"],
        needs_workspace: false,
        description: "Releases (all namespaces)",
    },
];

/// Look up a shortcut by tool + id.
pub fn find(tool: Tool, id: &str) -> Option<&'static Shortcut> {
    CATALOG.iter().find(|s| s.tool == tool && s.id == id)
}

/// All shortcuts for a tool.
pub fn for_tool(tool: Tool) -> impl Iterator<Item = &'static Shortcut> {
    CATALOG.iter().filter(move |s| s.tool == tool)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn finds_known_shortcut() {
        let s = find(Tool::Git, "status").unwrap();
        assert_eq!(s.program, "git");
        assert!(s.needs_workspace);
        assert_eq!(s.args_owned(), vec!["status", "--short", "--branch"]);
    }

    #[test]
    fn unknown_shortcut_is_none() {
        assert!(find(Tool::Git, "push").is_none());
        assert!(find(Tool::Helm, "install").is_none());
    }

    #[test]
    fn ids_are_unique_per_tool() {
        for entry in CATALOG {
            let count = CATALOG
                .iter()
                .filter(|s| s.tool == entry.tool && s.id == entry.id)
                .count();
            assert_eq!(count, 1, "duplicate {:?}/{}", entry.tool, entry.id);
        }
    }

    #[test]
    fn for_tool_lists_only_that_tool() {
        assert!(for_tool(Tool::Kubectl).all(|s| s.tool == Tool::Kubectl));
        assert!(for_tool(Tool::Helm).count() >= 1);
    }
}
