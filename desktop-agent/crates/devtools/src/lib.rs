//! Developer quick-launch and tool-shortcut engine for the DeskSync agent.
//!
//! Phase 8 lets a developer trigger workstation actions from the phone: launch
//! an editor (VS Code / Cursor / Claude) or terminal (Warp / Terminal / iTerm /
//! PowerShell / Windows Terminal), open a saved workspace, run curated Git /
//! Docker / kubectl / Helm shortcuts, or SSH into a saved host.
//!
//! ## Security model
//!
//! The engine is built around an **allowlist**, not free-form execution:
//!
//! - The wire model ([`DevActionKind`]) is closed — the phone picks from fixed
//!   enums and references workspaces/hosts by **id** only. No field carries a
//!   raw path, host, or command string.
//! - Ids are resolved against registries ([`WorkspaceRegistry`],
//!   [`SshHostRegistry`]) populated out-of-band by the user, so the phone can
//!   never introduce a new path or host.
//! - Tool shortcuts come from a fixed [`shortcuts::CATALOG`]; arguments are
//!   never taken from the client.
//! - Commands are spawned **without a shell** ([`TokioCommandRunner`]), so an
//!   argument can never be reinterpreted as a command.
//!
//! Every layer is pure and unit-tested; only [`TokioCommandRunner`] touches the
//! OS, and it is covered with POSIX-safe tests.

#![forbid(unsafe_code)]
#![warn(missing_docs)]

pub mod error;
pub mod launch;
pub mod model;
pub mod planner;
pub mod registry;
pub mod runner;
pub mod service;
pub mod shortcuts;

pub use error::{DevToolsError, Result};
pub use model::{
    CommandSpec, DevActionKind, DevActionRequest, DevActionResult, DevActionStatus, Editor, SshHost, Terminal, Tool,
    Workspace,
};
pub use planner::plan;
pub use registry::{SshHostRegistry, WorkspaceRegistry};
pub use runner::{CommandRunner, TokioCommandRunner};
pub use service::DevToolsService;
pub use shortcuts::Shortcut;
