//! Command execution backends.
//!
//! [`CommandRunner`] abstracts *how* a planned [`CommandSpec`] runs so the
//! service is testable with a recording fake. [`TokioCommandRunner`] is the real
//! backend: it spawns the program **directly** (never through a shell), so the
//! argument vector cannot be reinterpreted. GUI launches are detached and
//! fire-and-forget; tool shortcuts are run to completion with a timeout and
//! their (truncated) output captured.

use std::process::Stdio;
use std::time::Duration;

use async_trait::async_trait;
use tokio::process::Command;

use crate::error::{DevToolsError, Result};
use crate::model::CommandSpec;

/// Maximum bytes of captured output returned to the client.
pub const MAX_OUTPUT_BYTES: usize = 16 * 1024;

/// Executes planned commands.
#[async_trait]
pub trait CommandRunner: Send + Sync {
    /// Run `spec`. Returns captured output for `capture_output` specs, or an
    /// empty string for fire-and-forget launches.
    async fn run(&self, spec: &CommandSpec) -> Result<String>;
}

/// Runs commands with `tokio::process`, without a shell.
#[derive(Debug, Clone)]
pub struct TokioCommandRunner {
    timeout: Duration,
}

impl TokioCommandRunner {
    /// Create a runner with the given per-command timeout for captured
    /// shortcuts.
    pub fn new(timeout: Duration) -> Self {
        Self { timeout }
    }
}

impl Default for TokioCommandRunner {
    fn default() -> Self {
        Self::new(Duration::from_secs(20))
    }
}

#[async_trait]
impl CommandRunner for TokioCommandRunner {
    async fn run(&self, spec: &CommandSpec) -> Result<String> {
        let mut cmd = Command::new(&spec.program);
        cmd.args(&spec.args);
        if let Some(cwd) = &spec.cwd {
            cmd.current_dir(cwd);
        }
        // The agent runs unattended; never inherit a controlling TTY.
        cmd.stdin(Stdio::null());

        if !spec.capture_output {
            // Detached GUI launch: silence pipes and do not wait.
            cmd.stdout(Stdio::null()).stderr(Stdio::null());
            cmd.spawn()
                .map_err(|e| DevToolsError::Execution(format!("failed to launch {}: {e}", spec.program)))?;
            return Ok(String::new());
        }

        cmd.stdout(Stdio::piped()).stderr(Stdio::piped());
        let output = tokio::time::timeout(self.timeout, cmd.output())
            .await
            .map_err(|_| DevToolsError::Execution(format!("{} timed out", spec.program)))?
            .map_err(|e| DevToolsError::Execution(format!("failed to run {}: {e}", spec.program)))?;

        let mut combined = String::new();
        combined.push_str(&String::from_utf8_lossy(&output.stdout));
        if !output.stderr.is_empty() {
            if !combined.is_empty() {
                combined.push('\n');
            }
            combined.push_str(&String::from_utf8_lossy(&output.stderr));
        }
        let combined = truncate(combined);

        if output.status.success() {
            Ok(combined)
        } else {
            let code = output
                .status
                .code()
                .map(|c| c.to_string())
                .unwrap_or_else(|| "signal".into());
            Err(DevToolsError::Execution(format!(
                "{} exited with {code}: {combined}",
                spec.program
            )))
        }
    }
}

fn truncate(mut s: String) -> String {
    if s.len() > MAX_OUTPUT_BYTES {
        // Truncate on a char boundary at or below the limit.
        let mut end = MAX_OUTPUT_BYTES;
        while end > 0 && !s.is_char_boundary(end) {
            end -= 1;
        }
        s.truncate(end);
        s.push_str("\n… (truncated)");
    }
    s
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn captures_output_of_a_real_command() {
        let runner = TokioCommandRunner::default();
        // `echo` (or cmd on Windows) is universally available; keep the test
        // POSIX-only to avoid shelling out on CI Windows runners.
        if cfg!(windows) {
            return;
        }
        let spec = CommandSpec::captured("echo", vec!["hello-devtools".into()], None);
        let out = runner.run(&spec).await.unwrap();
        assert!(out.contains("hello-devtools"));
    }

    #[tokio::test]
    async fn nonzero_exit_is_an_error() {
        if cfg!(windows) {
            return;
        }
        let runner = TokioCommandRunner::default();
        let spec = CommandSpec::captured("false", vec![], None);
        assert!(matches!(runner.run(&spec).await, Err(DevToolsError::Execution(_))));
    }

    #[tokio::test]
    async fn missing_program_is_an_error() {
        let runner = TokioCommandRunner::default();
        let spec = CommandSpec::captured("desksync-no-such-binary-xyz", vec![], None);
        assert!(matches!(runner.run(&spec).await, Err(DevToolsError::Execution(_))));
    }

    #[test]
    fn truncate_caps_length_on_char_boundary() {
        let big = "a".repeat(MAX_OUTPUT_BYTES + 100);
        let out = truncate(big);
        assert!(out.len() <= MAX_OUTPUT_BYTES + 32);
        assert!(out.ends_with("(truncated)"));
    }
}
