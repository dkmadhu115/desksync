//! The OS permissions the agent needs, and their current state.
//!
//! Missing permissions are the single most confusing failure mode of a remote
//! desktop tool, because nothing *errors*: macOS hands back a screenshot of the
//! wallpaper with no windows, and silently drops injected keystrokes. The agent
//! looks healthy and the phone shows a blank screen.
//!
//! So permissions are treated as first-class state: each one knows what breaks
//! without it, how to check it, and which System Settings pane grants it.
//!
//! Detection is only compiled with the `native` feature; otherwise every check
//! reports [`PermissionState::Unknown`], which callers must render as "can't
//! tell" rather than "denied". Guessing here would be worse than admitting
//! ignorance: a false "denied" sends users to toggle a setting that was already
//! on.

#![forbid(unsafe_code)]
#![warn(missing_docs)]

use std::fmt;

/// An OS permission the agent depends on.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Permission {
    /// Capturing the screen. Without it, frames contain no windows.
    ScreenRecording,
    /// Injecting keyboard and mouse events (macOS "Accessibility").
    /// Without it, the stream works but the phone cannot control anything.
    InputControl,
    /// Showing native notifications (e.g. a connection request).
    Notifications,
}

/// Whether a permission has been granted.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PermissionState {
    /// Confirmed granted.
    Granted,
    /// Confirmed not granted.
    Denied,
    /// Could not be determined on this build or platform.
    Unknown,
}

impl PermissionState {
    /// Whether this state should block "you're ready to connect".
    ///
    /// `Unknown` does not block: on a platform we cannot query, refusing to
    /// proceed would be a dead end.
    pub fn blocks_readiness(self) -> bool {
        matches!(self, PermissionState::Denied)
    }
}

impl fmt::Display for PermissionState {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let s = match self {
            PermissionState::Granted => "granted",
            PermissionState::Denied => "not granted",
            PermissionState::Unknown => "unknown",
        };
        f.write_str(s)
    }
}

/// A permission together with its current state.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PermissionCheck {
    /// Which permission this describes.
    pub permission: Permission,
    /// Its state right now.
    pub state: PermissionState,
}

impl Permission {
    /// Every permission the agent cares about, in the order a first-run wizard
    /// should present them: screen first (nothing works without it), then input,
    /// then the optional extras.
    pub const ALL: [Permission; 3] = [
        Permission::ScreenRecording,
        Permission::InputControl,
        Permission::Notifications,
    ];

    /// Short human name, matching what the OS calls it.
    pub fn label(&self) -> &'static str {
        match self {
            Permission::ScreenRecording => {
                if cfg!(target_os = "macos") {
                    "Screen & System Audio Recording"
                } else {
                    "Screen capture"
                }
            }
            Permission::InputControl => {
                if cfg!(target_os = "macos") {
                    "Accessibility"
                } else {
                    "Input control"
                }
            }
            Permission::Notifications => "Notifications",
        }
    }

    /// What the user loses without this permission. Phrased as a consequence,
    /// because "grant Accessibility" means nothing to most people.
    pub fn consequence(&self) -> &'static str {
        match self {
            Permission::ScreenRecording => "your phone will show a blank desktop with no windows",
            Permission::InputControl => "you can watch the screen but not control it from your phone",
            Permission::Notifications => "you won't be alerted when a phone asks to connect",
        }
    }

    /// Whether the agent is unusable without it.
    ///
    /// Only screen capture is truly required: without input the product still
    /// does something useful (view-only), and notifications are a convenience.
    pub fn is_required(&self) -> bool {
        matches!(self, Permission::ScreenRecording)
    }

    /// Deep link to the System Settings pane that grants this permission.
    pub fn settings_url(&self) -> Option<&'static str> {
        #[cfg(target_os = "macos")]
        {
            Some(match self {
                Permission::ScreenRecording => {
                    "x-apple.systempreferences:com.apple.preference.security?Privacy_ScreenCapture"
                }
                Permission::InputControl => {
                    "x-apple.systempreferences:com.apple.preference.security?Privacy_Accessibility"
                }
                Permission::Notifications => "x-apple.systempreferences:com.apple.preference.notifications",
            })
        }
        #[cfg(not(target_os = "macos"))]
        {
            None
        }
    }

    /// Current state of this permission.
    pub fn check(&self) -> PermissionState {
        check(*self)
    }
}

impl fmt::Display for Permission {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.label())
    }
}

/// Errors from acting on a permission.
#[derive(Debug, thiserror::Error)]
pub enum PermissionError {
    /// There is no known settings pane for this permission on this platform.
    #[error("no settings page is known for {0} on this platform")]
    NoSettingsPage(Permission),

    /// Launching the settings pane failed.
    #[error("could not open system settings: {0}")]
    Launch(#[from] std::io::Error),
}

/// Check one permission.
pub fn check(permission: Permission) -> PermissionState {
    #[cfg(all(feature = "native", target_os = "macos"))]
    {
        match permission {
            Permission::ScreenRecording => {
                if core_graphics::access::ScreenCaptureAccess.preflight() {
                    PermissionState::Granted
                } else {
                    PermissionState::Denied
                }
            }
            Permission::InputControl => {
                if macos_accessibility_client::accessibility::application_is_trusted() {
                    PermissionState::Granted
                } else {
                    PermissionState::Denied
                }
            }
            // Notification authorization is per-app and only readable through an
            // async ObjC callback; the wizard deep-links instead of guessing.
            Permission::Notifications => PermissionState::Unknown,
        }
    }
    #[cfg(not(all(feature = "native", target_os = "macos")))]
    {
        let _ = permission;
        PermissionState::Unknown
    }
}

/// Check every permission, in wizard order.
pub fn check_all() -> Vec<PermissionCheck> {
    Permission::ALL
        .iter()
        .map(|&permission| PermissionCheck {
            permission,
            state: check(permission),
        })
        .collect()
}

/// Whether everything strictly required to serve a session is granted.
///
/// A permission we cannot query does not block readiness; see
/// [`PermissionState::blocks_readiness`].
pub fn ready_to_serve() -> bool {
    !Permission::ALL
        .iter()
        .filter(|p| p.is_required())
        .any(|p| check(*p).blocks_readiness())
}

/// Ask the OS to prompt for a permission, where that is possible.
///
/// Returns the state after prompting. macOS only shows its screen-recording
/// prompt once per binary, so a `Denied` result here means the user must go to
/// System Settings — which is why [`open_settings`] exists as the fallback.
pub fn request(permission: Permission) -> PermissionState {
    #[cfg(all(feature = "native", target_os = "macos"))]
    {
        if permission == Permission::ScreenRecording {
            let granted = core_graphics::access::ScreenCaptureAccess.request();
            return if granted {
                PermissionState::Granted
            } else {
                PermissionState::Denied
            };
        }
    }
    check(permission)
}

/// Open the System Settings pane that grants a permission.
pub fn open_settings(permission: Permission) -> Result<(), PermissionError> {
    let url = permission
        .settings_url()
        .ok_or(PermissionError::NoSettingsPage(permission))?;
    open_url(url)
}

/// Hand a URL to the platform opener.
#[cfg(target_os = "macos")]
fn open_url(url: &str) -> Result<(), PermissionError> {
    std::process::Command::new("open").arg(url).status()?;
    Ok(())
}

#[cfg(target_os = "linux")]
fn open_url(url: &str) -> Result<(), PermissionError> {
    std::process::Command::new("xdg-open").arg(url).status()?;
    Ok(())
}

#[cfg(target_os = "windows")]
fn open_url(url: &str) -> Result<(), PermissionError> {
    std::process::Command::new("cmd")
        .args(["/C", "start", "", url])
        .status()?;
    Ok(())
}

#[cfg(not(any(target_os = "macos", target_os = "linux", target_os = "windows")))]
fn open_url(_url: &str) -> Result<(), PermissionError> {
    Err(PermissionError::NoSettingsPage(Permission::ScreenRecording))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_permission_explains_what_breaks_without_it() {
        // These strings are what a non-technical user reads in the wizard, so an
        // empty or jargon-only one is a real defect.
        for permission in Permission::ALL {
            assert!(!permission.label().is_empty());
            let consequence = permission.consequence();
            assert!(consequence.len() > 20, "{permission} needs a real explanation");
            assert!(
                !consequence.contains("permission"),
                "{permission} should describe the consequence, not restate the permission"
            );
        }
    }

    #[test]
    fn only_screen_capture_blocks_using_the_product() {
        assert!(Permission::ScreenRecording.is_required());
        // View-only is still useful, and notifications are a convenience, so
        // neither should stop a user from finishing setup.
        assert!(!Permission::InputControl.is_required());
        assert!(!Permission::Notifications.is_required());
    }

    #[test]
    fn unknown_state_does_not_block_readiness() {
        // On a platform we cannot query, blocking would be a dead end.
        assert!(!PermissionState::Unknown.blocks_readiness());
        assert!(!PermissionState::Granted.blocks_readiness());
        assert!(PermissionState::Denied.blocks_readiness());
    }

    #[test]
    fn check_all_covers_every_permission_in_wizard_order() {
        let checks = check_all();
        assert_eq!(checks.len(), Permission::ALL.len());
        // Screen capture first: nothing else matters if the screen is blank.
        assert_eq!(checks[0].permission, Permission::ScreenRecording);
        assert_eq!(
            checks.iter().map(|c| c.permission).collect::<Vec<_>>(),
            Permission::ALL.to_vec()
        );
    }

    #[cfg(target_os = "macos")]
    #[test]
    fn macos_settings_links_target_the_right_panes() {
        assert!(Permission::ScreenRecording
            .settings_url()
            .unwrap()
            .contains("Privacy_ScreenCapture"));
        assert!(Permission::InputControl
            .settings_url()
            .unwrap()
            .contains("Privacy_Accessibility"));
        for permission in Permission::ALL {
            let url = permission.settings_url().expect("macOS has a pane for each");
            assert!(
                url.starts_with("x-apple.systempreferences:"),
                "not a settings deep link: {url}"
            );
        }
    }

    #[cfg(not(feature = "native"))]
    #[test]
    fn without_the_native_feature_state_is_unknown_not_denied() {
        // A false "denied" would send users to toggle an already-enabled setting.
        for permission in Permission::ALL {
            assert_eq!(check(permission), PermissionState::Unknown);
        }
        assert!(ready_to_serve(), "an unqueryable build must not report itself broken");
    }
}
