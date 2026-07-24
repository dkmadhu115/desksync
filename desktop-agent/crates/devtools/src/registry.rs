//! Registries of user-approved workspaces and SSH hosts.
//!
//! These are the *only* source of paths and hosts the engine will ever act on.
//! The client references entries by id; the agent resolves them here. Entries
//! are added out-of-band (the config UI / agent config), never by the phone, so
//! the phone cannot introduce a new path or host.

use crate::error::{DevToolsError, Result};
use crate::model::{SshHost, Workspace};
use std::path::Path;

/// A registry of saved workspaces (project directories).
#[derive(Debug, Clone, Default)]
pub struct WorkspaceRegistry {
    items: Vec<Workspace>,
}

impl WorkspaceRegistry {
    /// An empty registry.
    pub fn new() -> Self {
        Self::default()
    }

    /// Build from persisted items, validating each. Invalid entries are
    /// rejected (the whole load fails) so a corrupt config cannot smuggle in a
    /// bad path.
    pub fn from_items(items: Vec<Workspace>) -> Result<Self> {
        let mut reg = Self::new();
        for w in items {
            reg.upsert(w)?;
        }
        Ok(reg)
    }

    /// All registered workspaces.
    pub fn list(&self) -> &[Workspace] {
        &self.items
    }

    /// Look up a workspace by id.
    pub fn get(&self, id: &str) -> Option<&Workspace> {
        self.items.iter().find(|w| w.id == id)
    }

    /// Resolve a workspace path by id, or a typed error.
    pub fn resolve_path(&self, id: &str) -> Result<String> {
        self.get(id)
            .map(|w| w.path.clone())
            .ok_or_else(|| DevToolsError::UnknownWorkspace(id.to_string()))
    }

    /// Insert or replace a workspace after validating it. Validation checks
    /// structure only (non-empty fields, absolute path); filesystem existence
    /// is checked separately by the caller that has disk access.
    pub fn upsert(&mut self, ws: Workspace) -> Result<()> {
        validate_workspace(&ws)?;
        if let Some(existing) = self.items.iter_mut().find(|w| w.id == ws.id) {
            *existing = ws;
        } else {
            self.items.push(ws);
        }
        Ok(())
    }

    /// Remove a workspace by id; returns whether it existed.
    pub fn remove(&mut self, id: &str) -> bool {
        let before = self.items.len();
        self.items.retain(|w| w.id != id);
        self.items.len() != before
    }
}

/// A registry of saved SSH hosts.
#[derive(Debug, Clone, Default)]
pub struct SshHostRegistry {
    items: Vec<SshHost>,
}

impl SshHostRegistry {
    /// An empty registry.
    pub fn new() -> Self {
        Self::default()
    }

    /// Build from persisted items, validating each.
    pub fn from_items(items: Vec<SshHost>) -> Result<Self> {
        let mut reg = Self::new();
        for h in items {
            reg.upsert(h)?;
        }
        Ok(reg)
    }

    /// All registered hosts.
    pub fn list(&self) -> &[SshHost] {
        &self.items
    }

    /// Look up a host by id.
    pub fn get(&self, id: &str) -> Option<&SshHost> {
        self.items.iter().find(|h| h.id == id)
    }

    /// Resolve a host by id, or a typed error.
    pub fn resolve(&self, id: &str) -> Result<SshHost> {
        self.get(id)
            .cloned()
            .ok_or_else(|| DevToolsError::UnknownHost(id.to_string()))
    }

    /// Insert or replace a host after validating it.
    pub fn upsert(&mut self, host: SshHost) -> Result<()> {
        validate_host(&host)?;
        if let Some(existing) = self.items.iter_mut().find(|h| h.id == host.id) {
            *existing = host;
        } else {
            self.items.push(host);
        }
        Ok(())
    }

    /// Remove a host by id; returns whether it existed.
    pub fn remove(&mut self, id: &str) -> bool {
        let before = self.items.len();
        self.items.retain(|h| h.id != id);
        self.items.len() != before
    }
}

fn validate_workspace(w: &Workspace) -> Result<()> {
    if w.id.trim().is_empty() {
        return Err(DevToolsError::invalid("workspace", "id must not be empty"));
    }
    if w.name.trim().is_empty() {
        return Err(DevToolsError::invalid("workspace", "name must not be empty"));
    }
    if w.path.trim().is_empty() || !Path::new(&w.path).is_absolute() {
        return Err(DevToolsError::invalid("workspace", "path must be absolute"));
    }
    Ok(())
}

fn validate_host(h: &SshHost) -> Result<()> {
    if h.id.trim().is_empty() {
        return Err(DevToolsError::invalid("ssh host", "id must not be empty"));
    }
    if h.label.trim().is_empty() {
        return Err(DevToolsError::invalid("ssh host", "label must not be empty"));
    }
    // The user/host feed argv directly; reject whitespace/control characters so
    // a single field cannot smuggle extra tokens even though we never use a
    // shell.
    for (field, value) in [("user", &h.user), ("host", &h.host)] {
        if value.trim().is_empty() {
            return Err(DevToolsError::invalid("ssh host", format!("{field} must not be empty")));
        }
        if value.chars().any(|c| c.is_whitespace() || c.is_control()) {
            return Err(DevToolsError::invalid(
                "ssh host",
                format!("{field} must not contain whitespace or control characters"),
            ));
        }
    }
    if h.port == 0 {
        return Err(DevToolsError::invalid("ssh host", "port must be non-zero"));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ws(id: &str) -> Workspace {
        Workspace {
            id: id.into(),
            name: "Project".into(),
            path: "/home/dev/project".into(),
        }
    }

    fn host(id: &str) -> SshHost {
        SshHost {
            id: id.into(),
            label: "Prod".into(),
            user: "deploy".into(),
            host: "10.0.0.1".into(),
            port: 22,
        }
    }

    #[test]
    fn workspace_upsert_get_remove() {
        let mut reg = WorkspaceRegistry::new();
        reg.upsert(ws("a")).unwrap();
        assert_eq!(reg.resolve_path("a").unwrap(), "/home/dev/project");
        assert!(reg.remove("a"));
        assert!(!reg.remove("a"));
        assert!(matches!(reg.resolve_path("a"), Err(DevToolsError::UnknownWorkspace(_))));
    }

    #[test]
    fn workspace_rejects_relative_or_empty_path() {
        let mut reg = WorkspaceRegistry::new();
        let mut bad = ws("a");
        bad.path = "relative/dir".into();
        assert!(reg.upsert(bad).is_err());
        let mut empty = ws("b");
        empty.name = "".into();
        assert!(reg.upsert(empty).is_err());
    }

    #[test]
    fn host_rejects_injection_characters() {
        let mut reg = SshHostRegistry::new();
        let mut bad = host("h");
        bad.host = "10.0.0.1 rm -rf".into();
        assert!(reg.upsert(bad).is_err());
    }

    #[test]
    fn host_resolves_by_id() {
        let mut reg = SshHostRegistry::new();
        reg.upsert(host("h")).unwrap();
        assert_eq!(reg.resolve("h").unwrap().destination(), "deploy@10.0.0.1");
        assert!(matches!(reg.resolve("x"), Err(DevToolsError::UnknownHost(_))));
    }

    #[test]
    fn from_items_rejects_invalid_batch() {
        let mut bad = ws("a");
        bad.path = "nope".into();
        assert!(WorkspaceRegistry::from_items(vec![bad]).is_err());
    }
}
