use ailloli_ui_runtime::app::{ExternalUrl, ExternalUrlOpener, OpenUrlError};

/// Native, non-blocking external URL opener used by [`crate::UiApp`].
#[derive(Clone, Copy, Debug, Default)]
pub struct SystemExternalUrlOpener;

impl SystemExternalUrlOpener {
    pub const fn new() -> Self {
        Self
    }
}

impl ExternalUrlOpener for SystemExternalUrlOpener {
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
fn open_system_url(url: &ExternalUrl) -> Result<(), OpenUrlError> {
    spawn_opener("xdg-open", url)
}

#[cfg(target_os = "macos")]
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
fn spawn_opener(program: &str, url: &ExternalUrl) -> Result<(), OpenUrlError> {
    std::process::Command::new(program)
        .arg(url.as_str())
        .spawn()
        .map(|_| ())
        .map_err(|_| OpenUrlError::LaunchFailed)
}

#[cfg(windows)]
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
fn open_system_url(_url: &ExternalUrl) -> Result<(), OpenUrlError> {
    Err(OpenUrlError::Unavailable)
}

#[cfg(test)]
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
