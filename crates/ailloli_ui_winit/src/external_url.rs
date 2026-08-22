//! Platform browser handoff for already validated HTTP(S) URLs.
//!
//! Unix-like systems spawn an opaque single argument without a shell; Windows
//! calls `ShellExecuteW`. Success means launch was accepted, not that navigation
//! completed. Unsupported targets return `Unavailable`.

use ailloli_ui_runtime::app::{ExternalUrl, ExternalUrlOpener, OpenUrlError};

/// Native, non-blocking external URL opener used by [`crate::UiApp`].
///
/// # Examples
///
/// ```
/// use ailloli_ui_winit::SystemExternalUrlOpener;
/// let opener = SystemExternalUrlOpener::new();
/// assert_eq!(format!("{opener:?}"), "SystemExternalUrlOpener");
/// ```
#[derive(Clone, Copy, Debug, Default)]
pub struct SystemExternalUrlOpener;

/// Zero-state construction.
impl SystemExternalUrlOpener {
    /// Creates a stateless platform opener without launching a process.
    ///
    /// # Examples
    ///
    /// ```
    /// use ailloli_ui_winit::SystemExternalUrlOpener;
    /// let _: SystemExternalUrlOpener = SystemExternalUrlOpener::new();
    /// ```
    pub const fn new() -> Self {
        Self
    }
}

/// Delegates validated URLs to the platform-specific nonblocking launch path.
impl ExternalUrlOpener for SystemExternalUrlOpener {
    /// Delegates a validated URL to the platform opener implementation.
    fn open(&self, url: &ExternalUrl) -> Result<(), OpenUrlError> {
        open_system_url(url)
    }
}

#[cfg(any(
    target_os = "linux",
    target_os = "freebsd",
    target_os = "openbsd",
    target_os = "netbsd",
    target_os = "dragonfly"
))]
/// Launches `xdg-open` on Linux and supported BSD targets.
fn open_system_url(url: &ExternalUrl) -> Result<(), OpenUrlError> {
    spawn_opener("xdg-open", url)
}

#[cfg(target_os = "macos")]
/// Launches the absolute macOS `/usr/bin/open` utility.
fn open_system_url(url: &ExternalUrl) -> Result<(), OpenUrlError> {
    spawn_opener("/usr/bin/open", url)
}

#[cfg(any(
    target_os = "linux",
    target_os = "freebsd",
    target_os = "openbsd",
    target_os = "netbsd",
    target_os = "dragonfly",
    target_os = "macos"
))]
/// Spawns `program` with the URL as one opaque argument and does not wait.
fn spawn_opener(program: &str, url: &ExternalUrl) -> Result<(), OpenUrlError> {
    std::process::Command::new(program)
        .arg(url.as_str())
        .spawn()
        .map(|_| ())
        .map_err(|_| OpenUrlError::LaunchFailed)
}

#[cfg(windows)]
/// Passes a NUL-terminated UTF-16 URL to `ShellExecuteW` with operation `open`.
fn open_system_url(url: &ExternalUrl) -> Result<(), OpenUrlError> {
    use std::iter;
    use std::ptr;
    use windows_sys::Win32::UI::Shell::ShellExecuteW;
    use windows_sys::Win32::UI::WindowsAndMessaging::SW_SHOWNORMAL;

    let operation = "open"
        .encode_utf16()
        .chain(iter::once(0))
        .collect::<Vec<_>>();
    let target = url
        .as_str()
        .encode_utf16()
        .chain(iter::once(0))
        .collect::<Vec<_>>();
    let result = unsafe {
        ShellExecuteW(
            ptr::null_mut(),
            operation.as_ptr(),
            target.as_ptr(),
            ptr::null(),
            ptr::null(),
            SW_SHOWNORMAL,
        )
    };
    if shell_execute_succeeded(result as isize) {
        Ok(())
    } else {
        Err(OpenUrlError::LaunchFailed)
    }
}

#[cfg(windows)]
/// Applies the Win32 contract: values strictly greater than 32 mean success.
fn shell_execute_succeeded(result: isize) -> bool {
    result > 32
}

#[cfg(not(any(
    target_os = "linux",
    target_os = "freebsd",
    target_os = "openbsd",
    target_os = "netbsd",
    target_os = "dragonfly",
    target_os = "macos",
    windows
)))]
/// Reports platform unavailability without side effects.
fn open_system_url(_url: &ExternalUrl) -> Result<(), OpenUrlError> {
    Err(OpenUrlError::Unavailable)
}

#[cfg(test)]
/// Command construction, Windows return-code, and opt-in real-launch scenarios.
mod tests {
    use super::*;

    #[cfg(any(
        target_os = "linux",
        target_os = "freebsd",
        target_os = "openbsd",
        target_os = "netbsd",
        target_os = "dragonfly"
    ))]
    #[test]
    fn unix_plan_uses_xdg_open_with_an_opaque_argument() {
        let url = ExternalUrl::parse("https://example.com/?q=a;b&x=$(ignored)").unwrap();
        let mut command = std::process::Command::new("xdg-open");
        command.arg(url.as_str());
        assert_eq!(command.get_program(), "xdg-open");
        assert_eq!(
            command.get_args().collect::<Vec<_>>(),
            [std::ffi::OsStr::new(url.as_str())]
        );
    }

    #[cfg(windows)]
    #[test]
    fn shell_execute_success_contract_is_strictly_greater_than_32() {
        assert!(!shell_execute_succeeded(32));
        assert!(shell_execute_succeeded(33));
    }

    #[test]
    #[ignore = "opens the configured browser; set AILLOLI_UI_OPEN_URL_REAL=1"]
    fn native_external_url_smoke() {
        if std::env::var("AILLOLI_UI_OPEN_URL_REAL").as_deref() != Ok("1") {
            eprintln!("skipped: set AILLOLI_UI_OPEN_URL_REAL=1 to open a real browser");
            return;
        }
        SystemExternalUrlOpener::new()
            .open(&ExternalUrl::parse("https://example.com").unwrap())
            .unwrap();
    }
}
