//! Guided first-run setup.
//!
//! Getting a fresh machine to "my phone can connect" involves four things that
//! fail in unrelated, mostly silent ways: signing in, granting OS permissions,
//! registering the device, and keeping the agent running. Doing them in the wrong
//! order wastes time — granting screen recording before signing in, for example,
//! means restarting the agent twice.
//!
//! So this walks them in dependency order, checks each one instead of assuming,
//! and ends by telling the user what is still missing.
//!
//! Without a terminal (piped stdin, CI) it degrades to a read-only report: every
//! check still runs and prints its verdict, but nothing that changes the machine —
//! installing a background service, opening System Settings, launching a browser —
//! happens without someone actually agreeing to it.

use std::io::{IsTerminal, Write};

use anyhow::Context;
use desksync_core::identity::DeviceIdentity;
use desksync_core::{load_tokens, AgentConfig, AgentStore};
use desksync_permissions::{Permission, PermissionState};

use crate::agent_auth;

/// Total steps, for the "Step n/N" headers.
const STEPS: usize = 4;

/// Run the guided setup.
pub async fn run(store: &AgentStore, config: &AgentConfig, identity: &DeviceIdentity) -> anyhow::Result<()> {
    println!("DeskSync setup\n");

    step(1, "Sign in");
    let signed_in = ensure_signed_in(store, config, identity).await?;

    step(2, "Screen & input permissions");
    let permissions = ensure_permissions();

    step(3, "Register this computer");
    let device_id = match signed_in {
        true => register(store, config, identity).await,
        false => {
            println!("  Skipped — sign in first, then re-run `desksync-agent setup`.");
            None
        }
    };

    step(4, "Run in the background");
    offer_service();

    summary(signed_in, device_id.as_deref(), &permissions);
    Ok(())
}

/// Print a step header.
fn step(n: usize, title: &str) {
    println!("\nStep {n}/{STEPS}  {title}");
    println!("{}", "-".repeat(40));
}

/// Make sure credentials are stored, offering a browser sign-in if not.
async fn ensure_signed_in(store: &AgentStore, config: &AgentConfig, identity: &DeviceIdentity) -> anyhow::Result<bool> {
    let secrets = desksync_core::default_secret_store(store.dir());
    if load_tokens(secrets.as_ref()).ok().flatten().is_some() {
        println!("  Already signed in. (Run `desksync-agent logout` to switch accounts.)");
        return Ok(true);
    }

    if !consent("  Sign in with Google in your browser now?", true) {
        println!("  Not signed in. Run `desksync-agent login` when you're ready.");
        return Ok(false);
    }

    // Reuse the same path as the standalone command so there is one sign-in
    // implementation to keep correct.
    match crate::run_login(store, config, identity, crate::LoginMode::Browser).await {
        Ok(()) => Ok(true),
        Err(e) => {
            println!("  Sign-in failed: {e:#}");
            println!("  You can retry with `desksync-agent login`.");
            Ok(false)
        }
    }
}

/// Walk each permission, opening the right settings pane for missing ones.
fn ensure_permissions() -> Vec<(Permission, PermissionState)> {
    let mut results = Vec::new();
    for permission in Permission::ALL {
        results.push((permission, resolve_permission(permission)));
    }
    results
}

/// Report one permission, and help the user grant it if it is missing.
fn resolve_permission(permission: Permission) -> PermissionState {
    let state = permission.check();
    match state {
        PermissionState::Granted => {
            println!("  [ok]      {permission}");
            return state;
        }
        PermissionState::Unknown => {
            println!("  [unknown] {permission} — can't check this automatically here");
            if permission.is_required() {
                println!(
                    "            Make sure it is enabled: without it, {}.",
                    permission.consequence()
                );
            }
            return state;
        }
        PermissionState::Denied => {}
    }

    let requirement = if permission.is_required() {
        "required"
    } else {
        "optional"
    };
    println!("  [missing] {permission} ({requirement})");
    println!("            Without it, {}.", permission.consequence());

    if !consent(
        "            Open System Settings to grant it?",
        permission.is_required(),
    ) {
        return state;
    }
    if let Err(e) = desksync_permissions::open_settings(permission) {
        println!("            Could not open System Settings: {e}");
        return state;
    }

    // Re-check after the user says they've granted it. macOS applies capture
    // consent per executable and only at process start, so a fresh grant needs a
    // restart to take effect — say so rather than letting them wonder why the
    // screen is still blank.
    prompt_enter("            Enable it in System Settings, then press Enter to re-check.");
    let rechecked = permission.check();
    match rechecked {
        PermissionState::Granted => println!("  [ok]      {permission} — granted"),
        _ => println!("  [missing] {permission} — still not granted"),
    }
    if permission == Permission::ScreenRecording && rechecked == PermissionState::Granted {
        println!("            Restart the agent for this to take effect.");
    }
    rechecked
}

/// Register the device (idempotent) and report the id.
async fn register(store: &AgentStore, config: &AgentConfig, identity: &DeviceIdentity) -> Option<String> {
    match agent_auth::bootstrap(store, config, identity).await {
        Ok(Some(account)) => {
            println!("  Registered as device {}.", account.device_id);
            Some(account.device_id)
        }
        Ok(None) => {
            println!("  No credentials found — sign in and re-run setup.");
            None
        }
        Err(e) => {
            let detail = format!("{e:#}");
            // A transport failure here is almost always the backend URL, not the
            // account — say which URL was tried so the fix is obvious.
            if detail.contains("http transport error") || detail.contains("error sending request") {
                println!("  Couldn't reach the DeskSync backend at {}.", config.api_url);
                println!("  Check that it is running and reachable, then re-run setup.");
            } else {
                println!("  Registration failed: {detail}");
            }
            None
        }
    }
}

/// Offer to install the background service.
fn offer_service() {
    if !consent("  Start DeskSync automatically in the background?", true) {
        println!("  Not installed. Run `desksync-agent` in a terminal, or");
        println!("  `desksync-agent service install` to have it always running.");
        return;
    }
    if let Err(e) = crate::run_service(Some("install")) {
        println!("  Could not install the service: {e:#}");
    }
}

/// Print what works and what is still missing.
fn summary(signed_in: bool, device_id: Option<&str>, permissions: &[(Permission, PermissionState)]) {
    println!("\n{}", "=".repeat(40));
    println!("Setup summary");
    println!("{}", "=".repeat(40));
    println!("  Signed in:  {}", yes_no(signed_in));
    println!("  Registered: {}", device_id.unwrap_or("no"));
    for (permission, state) in permissions {
        println!("  {permission}: {state}");
    }

    println!();
    match next_step(signed_in, device_id.is_some(), permissions) {
        NextStep::SignIn => println!("Next: run `desksync-agent login`."),
        NextStep::Register => {
            println!("Next: this computer isn't registered yet, so it won't appear on your phone.");
            println!("      Fix the problem above and re-run `desksync-agent setup`.");
        }
        NextStep::Grant(missing) => {
            println!(
                "Next: grant {} — until then your phone sees a blank screen.",
                missing.join(", ")
            );
        }
        NextStep::Ready => {
            println!("Ready. Open the DeskSync app on your phone and connect to this computer.");
            println!("Check on it any time with `desksync-agent status`.");
        }
    }
}

/// The one thing standing between this machine and a working connection.
#[derive(Debug, PartialEq, Eq)]
enum NextStep {
    SignIn,
    Register,
    /// Required permissions the OS is refusing, by label.
    Grant(Vec<String>),
    Ready,
}

/// Decide what the user must fix next.
///
/// The order matters and is not cosmetic: registration needs credentials, and
/// pointing someone at System Settings before they are signed in makes them do
/// the work twice. Only *required* permissions gate readiness, and only when the
/// OS says no — an `Unknown` state (a platform we cannot query) must not be
/// reported as a blocker we cannot prove.
fn next_step(signed_in: bool, registered: bool, permissions: &[(Permission, PermissionState)]) -> NextStep {
    if !signed_in {
        return NextStep::SignIn;
    }
    if !registered {
        return NextStep::Register;
    }
    let missing: Vec<String> = permissions
        .iter()
        .filter(|(p, s)| p.is_required() && s.blocks_readiness())
        .map(|(p, _)| p.label().to_string())
        .collect();
    if missing.is_empty() {
        NextStep::Ready
    } else {
        NextStep::Grant(missing)
    }
}

fn yes_no(value: bool) -> &'static str {
    if value {
        "yes"
    } else {
        "no"
    }
}

/// Ask before doing something that changes the machine.
///
/// Unattended, the answer is always no: a setup run from a script or CI must not
/// install a launchd job, pop system UI, or open a browser just because that is
/// the interactive default. Callers print the manual command instead.
fn consent(question: &str, default: bool) -> bool {
    if !std::io::stdin().is_terminal() {
        return false;
    }
    confirm(question, default)
}

/// Ask a yes/no question, defaulting to `default` on empty input.
fn confirm(question: &str, default: bool) -> bool {
    if !std::io::stdin().is_terminal() {
        return default;
    }
    let hint = if default { "[Y/n]" } else { "[y/N]" };
    print!("{question} {hint} ");
    let _ = std::io::stdout().flush();

    let mut answer = String::new();
    if std::io::stdin().read_line(&mut answer).is_err() {
        return default;
    }
    match answer.trim().to_ascii_lowercase().as_str() {
        "" => default,
        "y" | "yes" => true,
        _ => false,
    }
}

/// Wait for the user to press Enter, skipping the wait without a terminal.
fn prompt_enter(message: &str) {
    if !std::io::stdin().is_terminal() {
        println!("{message}");
        return;
    }
    print!("{message} ");
    let _ = std::io::stdout().flush();
    let mut discard = String::new();
    let _ = std::io::stdin().read_line(&mut discard);
}

/// Print the current permission state without prompting, for `desksync-agent
/// permissions`.
pub fn print_permissions() -> anyhow::Result<()> {
    println!(
        "OS permissions for {}",
        std::env::current_exe().context("resolving executable")?.display()
    );
    println!();
    for check in desksync_permissions::check_all() {
        let requirement = if check.permission.is_required() {
            "required"
        } else {
            "optional"
        };
        println!("  {:<34} {} ({requirement})", check.permission.label(), check.state);
        if check.state != PermissionState::Granted {
            println!("  {:<34} without it, {}", "", check.permission.consequence());
        }
    }
    if !desksync_permissions::ready_to_serve() {
        println!("\nRun `desksync-agent setup` for help granting these.");
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn all_granted() -> Vec<(Permission, PermissionState)> {
        Permission::ALL.iter().map(|p| (*p, PermissionState::Granted)).collect()
    }

    #[test]
    fn everything_done_is_ready() {
        assert_eq!(next_step(true, true, &all_granted()), NextStep::Ready);
    }

    #[test]
    fn sign_in_comes_before_anything_else() {
        // Even with permissions denied, sign-in is the first thing to fix.
        let denied = vec![(Permission::ScreenRecording, PermissionState::Denied)];
        assert_eq!(next_step(false, false, &denied), NextStep::SignIn);
    }

    #[test]
    fn registration_is_reported_before_permissions() {
        let denied = vec![(Permission::ScreenRecording, PermissionState::Denied)];
        assert_eq!(next_step(true, false, &denied), NextStep::Register);
    }

    #[test]
    fn a_denied_required_permission_blocks_readiness() {
        let denied = vec![(Permission::ScreenRecording, PermissionState::Denied)];
        assert_eq!(
            next_step(true, true, &denied),
            NextStep::Grant(vec![Permission::ScreenRecording.label().to_string()])
        );
    }

    #[test]
    fn a_denied_optional_permission_does_not_block_readiness() {
        // View-only is a degraded but working product, so a missing input grant
        // must not be presented as a blocker.
        let denied = vec![
            (Permission::ScreenRecording, PermissionState::Granted),
            (Permission::InputControl, PermissionState::Denied),
        ];
        assert_eq!(next_step(true, true, &denied), NextStep::Ready);
    }

    #[test]
    fn an_unknown_permission_state_is_not_treated_as_a_blocker() {
        let unknown = vec![(Permission::ScreenRecording, PermissionState::Unknown)];
        assert_eq!(next_step(true, true, &unknown), NextStep::Ready);
    }
}
