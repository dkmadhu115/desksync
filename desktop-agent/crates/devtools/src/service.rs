//! The developer-tools service: validate a request, plan a command, run it, and
//! return a structured result. This is what the control-channel router calls
//! for each `dev_action` frame from the phone.

use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;

use tracing::{info, warn};

use crate::model::{DevActionRequest, DevActionResult};
use crate::planner;
use crate::registry::{SshHostRegistry, WorkspaceRegistry};
use crate::runner::CommandRunner;

/// Ties the registries, planner, and runner together.
pub struct DevToolsService {
    workspaces: WorkspaceRegistry,
    hosts: SshHostRegistry,
    runner: Arc<dyn CommandRunner>,
    os: String,
    dispatched: AtomicU64,
    rejected: AtomicU64,
}

impl DevToolsService {
    /// Build a service. `os` is normally `std::env::consts::OS`.
    pub fn new(
        workspaces: WorkspaceRegistry,
        hosts: SshHostRegistry,
        runner: Arc<dyn CommandRunner>,
        os: impl Into<String>,
    ) -> Self {
        Self {
            workspaces,
            hosts,
            runner,
            os: os.into(),
            dispatched: AtomicU64::new(0),
            rejected: AtomicU64::new(0),
        }
    }

    /// Registered workspaces (for advertising to the client).
    pub fn workspaces(&self) -> &WorkspaceRegistry {
        &self.workspaces
    }

    /// Registered SSH hosts.
    pub fn hosts(&self) -> &SshHostRegistry {
        &self.hosts
    }

    /// Number of successfully executed actions.
    pub fn dispatched(&self) -> u64 {
        self.dispatched.load(Ordering::SeqCst)
    }

    /// Number of rejected/failed actions.
    pub fn rejected(&self) -> u64 {
        self.rejected.load(Ordering::SeqCst)
    }

    /// Validate, plan, and execute a request, returning a client-facing result.
    pub async fn handle(&self, req: DevActionRequest) -> DevActionResult {
        let spec = match planner::plan(&req, &self.workspaces, &self.hosts, &self.os) {
            Ok(spec) => spec,
            Err(e) => {
                self.rejected.fetch_add(1, Ordering::SeqCst);
                warn!(request_id = %req.request_id, error = %e, "dev action rejected");
                return DevActionResult::error(req.request_id, e.to_string());
            }
        };

        match self.runner.run(&spec).await {
            Ok(output) => {
                self.dispatched.fetch_add(1, Ordering::SeqCst);
                info!(request_id = %req.request_id, program = %spec.program, "dev action executed");
                if output.is_empty() {
                    DevActionResult::ok(req.request_id, format!("Launched {}", spec.program))
                } else {
                    DevActionResult::ok_output(req.request_id, output)
                }
            }
            Err(e) => {
                self.rejected.fetch_add(1, Ordering::SeqCst);
                warn!(request_id = %req.request_id, error = %e, "dev action failed");
                DevActionResult::error(req.request_id, e.to_string())
            }
        }
    }

    /// Decode a JSON control frame and handle it. Returns `None` (and counts a
    /// rejection) when the frame is not a valid [`DevActionRequest`], so a
    /// malformed frame never disturbs the control loop.
    pub async fn handle_frame(&self, frame: &str) -> Option<DevActionResult> {
        match serde_json::from_str::<DevActionRequest>(frame) {
            Ok(req) => Some(self.handle(req).await),
            Err(e) => {
                self.rejected.fetch_add(1, Ordering::SeqCst);
                warn!(error = %e, "dropping malformed dev-action frame");
                None
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::{DevActionKind, DevActionStatus, Editor, Tool, Workspace};
    use crate::runner::CommandRunner;
    use crate::CommandSpec;
    use async_trait::async_trait;
    use std::sync::Mutex;

    #[derive(Default)]
    struct RecordingRunner {
        specs: Mutex<Vec<CommandSpec>>,
        output: String,
        fail: bool,
    }

    #[async_trait]
    impl CommandRunner for RecordingRunner {
        async fn run(&self, spec: &CommandSpec) -> crate::error::Result<String> {
            self.specs.lock().unwrap().push(spec.clone());
            if self.fail {
                Err(crate::error::DevToolsError::Execution("boom".into()))
            } else {
                Ok(self.output.clone())
            }
        }
    }

    fn workspaces() -> WorkspaceRegistry {
        WorkspaceRegistry::from_items(vec![Workspace {
            id: "ws1".into(),
            name: "App".into(),
            path: "/home/dev/app".into(),
        }])
        .unwrap()
    }

    fn service(runner: Arc<dyn CommandRunner>) -> DevToolsService {
        DevToolsService::new(workspaces(), SshHostRegistry::new(), runner, "linux")
    }

    #[tokio::test]
    async fn launch_reports_ok_and_runs_command() {
        let runner = Arc::new(RecordingRunner::default());
        let svc = service(runner.clone());

        let res = svc
            .handle(DevActionRequest {
                request_id: "r1".into(),
                kind: DevActionKind::LaunchEditor {
                    editor: Editor::VsCode,
                    workspace_id: Some("ws1".into()),
                },
            })
            .await;

        assert_eq!(res.status, DevActionStatus::Ok);
        assert_eq!(svc.dispatched(), 1);
        assert_eq!(runner.specs.lock().unwrap().len(), 1);
        assert_eq!(runner.specs.lock().unwrap()[0].program, "code");
    }

    #[tokio::test]
    async fn shortcut_output_is_returned() {
        let runner = Arc::new(RecordingRunner {
            output: "On branch main".into(),
            ..Default::default()
        });
        let svc = service(runner);

        let res = svc
            .handle(DevActionRequest {
                request_id: "r2".into(),
                kind: DevActionKind::RunShortcut {
                    tool: Tool::Git,
                    shortcut_id: "status".into(),
                    workspace_id: Some("ws1".into()),
                },
            })
            .await;

        assert_eq!(res.status, DevActionStatus::Ok);
        assert_eq!(res.output, "On branch main");
    }

    #[tokio::test]
    async fn invalid_request_is_rejected_without_running() {
        let runner = Arc::new(RecordingRunner::default());
        let svc = service(runner.clone());

        let res = svc
            .handle(DevActionRequest {
                request_id: "r3".into(),
                kind: DevActionKind::LaunchEditor {
                    editor: Editor::VsCode,
                    workspace_id: Some("missing".into()),
                },
            })
            .await;

        assert_eq!(res.status, DevActionStatus::Error);
        assert!(res.message.contains("unknown workspace"));
        assert_eq!(svc.rejected(), 1);
        assert!(runner.specs.lock().unwrap().is_empty());
    }

    #[tokio::test]
    async fn runner_failure_becomes_error_result() {
        let runner = Arc::new(RecordingRunner {
            fail: true,
            ..Default::default()
        });
        let svc = service(runner);

        let res = svc
            .handle(DevActionRequest {
                request_id: "r4".into(),
                kind: DevActionKind::RunShortcut {
                    tool: Tool::Docker,
                    shortcut_id: "ps".into(),
                    workspace_id: None,
                },
            })
            .await;

        assert_eq!(res.status, DevActionStatus::Error);
        assert!(res.message.contains("boom"));
    }

    #[tokio::test]
    async fn malformed_frame_returns_none_and_counts_rejection() {
        let svc = service(Arc::new(RecordingRunner::default()));
        assert!(svc.handle_frame("not-json").await.is_none());
        assert_eq!(svc.rejected(), 1);
    }

    #[tokio::test]
    async fn valid_frame_is_handled() {
        let svc = service(Arc::new(RecordingRunner::default()));
        let frame = r#"{"request_id":"r5","action":"run_shortcut","tool":"docker","shortcut_id":"ps"}"#;
        let res = svc.handle_frame(frame).await.unwrap();
        assert_eq!(res.request_id, "r5");
        assert_eq!(res.status, DevActionStatus::Ok);
    }
}
