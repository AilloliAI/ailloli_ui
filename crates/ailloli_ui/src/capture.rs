//! Declarative GPU capture options for [`App`](crate::App) visual tests and tooling.
//!
//! Prefer [`CaptureOpts`] on [`Window`](crate::Window) and optional [`AppBuilder::on_captured`](crate::AppBuilder::on_captured)
//! over manual [`CaptureHandle`] wiring for the common case.
//!
//! Capture requests are issued once during native startup and completed by the
//! WGPU readback path. Listener callbacks run synchronously on completion;
//! optional PNG file errors are retained and reported when the application exits.

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};

use ailloli_ui_core::Rect;
use ailloli_ui_render_wgpu::CaptureParams;
use ailloli_ui_winit::{
    CaptureError, CaptureHandle, CaptureRequestId, CaptureResult, CaptureTarget,
};

/// What to capture — the logical window id comes from [`Window::new`](crate::Window::new).
///
/// # Examples
///
/// ```
/// use ailloli_ui::CaptureTargetSpec;
/// let target = CaptureTargetSpec::Element { key: "chart".to_string() };
/// assert_eq!(target, CaptureTargetSpec::Element { key: "chart".to_string() });
/// ```
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CaptureTargetSpec {
    /// Entire drawable area of the selected logical window.
    Window,
    /// Bounds of the retained element carrying the exact view key.
    Element {
        /// Opaque retained-view key; empty strings are preserved and usually fail lookup.
        key: String,
    },
}

/// Declarative capture request attached to a [`Window`](crate::Window).
///
/// Defaults from both constructors are PNG encoding enabled, automatic exit
/// disabled, and no output file.
///
/// # Examples
///
/// ```
/// use ailloli_ui::CaptureOpts;
/// let opts = CaptureOpts::element("preview").encode_png(false).exit_after(true);
/// let debug = format!("{opts:?}");
/// assert!(debug.contains("preview"));
/// assert!(debug.contains("exit_after: true"));
/// ```
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CaptureOpts {
    /// Full-window or keyed-element target.
    target: CaptureTargetSpec,
    /// Whether WGPU should encode PNG bytes in addition to returning raw RGBA.
    encode_png: bool,
    /// Whether this request contributes to automatic event-loop exit.
    exit_after: bool,
    /// Optional PNG output path; `None` performs no file write.
    out_path: Option<PathBuf>,
}

/// Builder methods and crate-internal inspection for a declarative capture.
impl CaptureOpts {
    /// Capture the full window contents.
    ///
    /// # Examples
    ///
    /// ```
    /// use ailloli_ui::CaptureOpts;
    /// assert!(format!("{:?}", CaptureOpts::window()).contains("Window"));
    /// ```
    pub fn window() -> Self {
        Self {
            target: CaptureTargetSpec::Window,
            encode_png: true,
            exit_after: false,
            out_path: None,
        }
    }

    /// Capture a cropped region for the element identified by [`View::key`](crate::View::key).
    ///
    /// Key lookup occurs against the retained tree for the selected logical
    /// window. The string is stored verbatim.
    ///
    /// # Examples
    ///
    /// ```
    /// use ailloli_ui::CaptureOpts;
    /// assert!(format!("{:?}", CaptureOpts::element("canvas")).contains("canvas"));
    /// ```
    pub fn element(key: impl Into<String>) -> Self {
        Self {
            target: CaptureTargetSpec::Element { key: key.into() },
            encode_png: true,
            exit_after: false,
            out_path: None,
        }
    }

    /// Writes the PNG to `path` when the capture completes (creates parent directories).
    ///
    /// This implicitly enables automatic exit after all capture requests, but
    /// does not force PNG encoding back on if [`Self::encode_png(false)`] was set;
    /// that combination reports an empty-PNG I/O error at application exit.
    ///
    /// # Examples
    ///
    /// ```
    /// use ailloli_ui::CaptureOpts;
    /// let opts = CaptureOpts::window().file("artifacts/main.png");
    /// assert!(format!("{opts:?}").contains("artifacts/main.png"));
    /// ```
    pub fn file(mut self, path: impl Into<PathBuf>) -> Self {
        self.out_path = Some(path.into());
        self
    }

    /// When `true`, contributes to exiting the event loop after all captures complete.
    ///
    /// A `false` value does not override the implicit exit policy of another
    /// request with an output file or `exit_after(true)`.
    ///
    /// # Examples
    ///
    /// ```
    /// use ailloli_ui::CaptureOpts;
    /// assert!(format!("{:?}", CaptureOpts::window().exit_after(true)).contains("exit_after: true"));
    /// ```
    pub fn exit_after(mut self, value: bool) -> Self {
        self.exit_after = value;
        self
    }

    /// Controls whether PNG bytes are embedded in the GPU readback result.
    ///
    /// Raw RGBA bytes remain available either way.
    ///
    /// # Examples
    ///
    /// ```
    /// use ailloli_ui::CaptureOpts;
    /// assert!(format!("{:?}", CaptureOpts::window().encode_png(false)).contains("encode_png: false"));
    /// ```
    pub fn encode_png(mut self, value: bool) -> Self {
        self.encode_png = value;
        self
    }

    /// Borrows the full-window or keyed-element target.
    ///
    /// # Examples
    ///
    /// ```
    /// use ailloli_ui::{CaptureOpts, CaptureTargetSpec};
    /// let expected = CaptureTargetSpec::Window;
    /// assert!(format!("{:?}", CaptureOpts::window()).contains(&format!("{expected:?}")));
    /// ```
    pub(crate) fn target(&self) -> &CaptureTargetSpec {
        &self.target
    }

    /// Returns whether PNG encoding is requested; constructors default to `true`.
    ///
    /// # Examples
    ///
    /// ```
    /// use ailloli_ui::CaptureOpts;
    /// assert!(format!("{:?}", CaptureOpts::window()).contains("encode_png: true"));
    /// ```
    pub(crate) fn encode_png_enabled(&self) -> bool {
        self.encode_png
    }

    /// Returns the explicit exit flag, excluding file-output implicit policy.
    ///
    /// # Examples
    ///
    /// ```
    /// use ailloli_ui::CaptureOpts;
    /// assert!(format!("{:?}", CaptureOpts::window()).contains("exit_after: false"));
    /// ```
    pub(crate) fn exit_after_enabled(&self) -> bool {
        self.exit_after
    }

    /// Borrows the configured output path, or `None` when no file was requested.
    ///
    /// # Examples
    ///
    /// ```
    /// use ailloli_ui::CaptureOpts;
    /// let opts = CaptureOpts::window().file("capture.png");
    /// assert!(format!("{opts:?}").contains("capture.png"));
    /// ```
    pub(crate) fn out_path(&self) -> Option<&Path> {
        self.out_path.as_deref()
    }
}

/// Completed capture payload delivered to [`AppBuilder::on_captured`](crate::AppBuilder::on_captured).
///
/// Success has dimensions in physical pixels, raw RGBA length normally
/// `width * height * 4`, optional PNG bytes, optional physical-pixel crop bounds,
/// and no error. Failure uses zero dimensions, empty byte buffers, no bounds,
/// and `Some(error)`.
///
/// # Examples
///
/// ```
/// use ailloli_ui::{CapturedArtifact, CaptureTargetSpec};
/// let artifact = CapturedArtifact {
///     window_id: "main".into(), target: CaptureTargetSpec::Window,
///     width: 1, height: 1, png: Vec::new(), rgba: vec![0, 0, 0, 255],
///     bounds_px: None, error: None,
/// };
/// assert_eq!(artifact.rgba.len(), 4);
/// ```
#[derive(Debug, Clone, PartialEq)]
pub struct CapturedArtifact {
    /// Logical window ID associated with the request.
    pub window_id: String,
    /// Declarative full-window or keyed-element target.
    pub target: CaptureTargetSpec,
    /// Captured width in physical pixels, or zero on failure.
    pub width: u32,
    /// Captured height in physical pixels, or zero on failure.
    pub height: u32,
    /// Encoded PNG bytes, empty when disabled or on failure.
    pub png: Vec<u8>,
    /// Tight RGBA8 pixels in row-major order, empty on failure.
    pub rgba: Vec<u8>,
    /// Element crop bounds in physical pixels, or `None` for full-window/failure.
    pub bounds_px: Option<Rect>,
    /// Capture failure, or `None` for a successful readback.
    pub error: Option<CaptureError>,
}

/// Window id + declarative capture declarations (used during [`AppBuilder::run`](crate::AppBuilder::run)).
///
/// # Examples
///
/// ```
/// use ailloli_ui::{CaptureOpts, Window};
/// let window = Window::<()>::new("main").capture(CaptureOpts::window());
/// assert_eq!(window.id(), "main");
/// ```
#[derive(Debug, Clone)]
pub(crate) struct WindowCaptureSource {
    /// Logical window ID copied from the public declaration.
    pub window_id: String,
    /// Ordered capture options attached to that window.
    pub captures: Vec<CaptureOpts>,
}

/// Internal request metadata used to map a native completion back to public output.
#[derive(Clone)]
struct DeclarativeCaptureSpec {
    /// Logical window ID.
    window_id: String,
    /// Original declarative target.
    target: CaptureTargetSpec,
    /// Optional PNG output path.
    out_path: Option<PathBuf>,
}

/// Thread-safe callback invoked with an owned capture artifact.
type UserListener = Arc<dyn Fn(CapturedArtifact) + Send + Sync>;

/// Owns the shared [`CaptureHandle`] and declarative capture wiring for one app run.
///
/// Collections are unbounded. The I/O error queue is mutex-protected; poisoned
/// capture-completion writes panic, while final error retrieval treats poison as
/// no available error.
///
/// # Examples
///
/// ```
/// use ailloli_ui::CaptureHandle;
/// let handle = CaptureHandle::new();
/// assert!(!handle.exit_after_all_captures());
/// ```
#[derive(Clone)]
pub(crate) struct CaptureSession {
    /// Shared native request/completion handle.
    handle: CaptureHandle,
    /// Request ID to declarative metadata for pending or completed requests.
    specs: HashMap<CaptureRequestId, DeclarativeCaptureSpec>,
    /// User callbacks retained in registration order.
    listeners: Vec<UserListener>,
    /// Deferred file-output errors shared with the completion callback.
    io_errors: Arc<Mutex<Vec<std::io::Error>>>,
    /// Whether the handle came from the caller rather than the default constructor.
    explicit_handle: bool,
}

/// Creates an empty session with a new handle and no automatic-exit policy.
impl Default for CaptureSession {
    fn default() -> Self {
        Self {
            handle: CaptureHandle::new(),
            specs: HashMap::new(),
            listeners: Vec::new(),
            io_errors: Arc::new(Mutex::new(Vec::new())),
            explicit_handle: false,
        }
    }
}

/// Session assembly, listener wiring, completion, and deferred-error handling.
impl CaptureSession {
    /// Replaces the session handle and marks it as explicitly supplied.
    ///
    /// Existing declarative specs and listeners remain, but any requests already
    /// issued through the old handle are not transferred.
    ///
    /// # Examples
    ///
    /// ```
    /// use ailloli_ui::CaptureHandle;
    /// let handle = CaptureHandle::new();
    /// assert!(!handle.exit_after_all_captures());
    /// ```
    pub fn use_explicit_handle(mut self, handle: CaptureHandle) -> Self {
        self.handle = handle;
        self.explicit_handle = true;
        self
    }

    /// Returns `true` only after [`Self::use_explicit_handle`].
    ///
    /// # Examples
    ///
    /// ```
    /// use ailloli_ui::CaptureHandle;
    /// let handle = CaptureHandle::new();
    /// assert_eq!(handle.exit_after_all_captures(), false);
    /// ```
    pub fn has_explicit_handle(&self) -> bool {
        self.explicit_handle
    }

    /// Borrows the shared native capture handle.
    ///
    /// # Examples
    ///
    /// ```
    /// use ailloli_ui::CaptureHandle;
    /// let handle: CaptureHandle = CaptureHandle::new();
    /// assert!(!handle.has_pending());
    /// ```
    pub fn handle(&self) -> &CaptureHandle {
        &self.handle
    }

    /// Appends one user completion listener.
    ///
    /// The listener is retained for the entire app run and invoked once per
    /// recognized declarative result. Panics inside a listener propagate through
    /// the native completion callback.
    ///
    /// # Examples
    ///
    /// ```
    /// use ailloli_ui::App;
    /// let _builder = App::new().state(()).on_captured(|artifact| {
    ///     let _: u32 = artifact.width;
    /// });
    /// ```
    pub fn register_listener<F>(&mut self, listener: F)
    where
        F: Fn(CapturedArtifact) + Send + Sync + 'static,
    {
        self.listeners.push(Arc::new(listener));
    }

    /// Returns whether at least one user listener is registered.
    ///
    /// # Examples
    ///
    /// ```
    /// let listeners: Vec<Box<dyn Fn()>> = Vec::new();
    /// assert!(listeners.is_empty());
    /// ```
    pub fn has_on_captured_listeners(&self) -> bool {
        !self.listeners.is_empty()
    }

    /// Enforces that listeners have at least one declarative capture source.
    ///
    /// # Errors
    ///
    /// Returns [`std::io::ErrorKind::InvalidInput`] when listeners exist but all
    /// source capture lists are empty.
    ///
    /// # Examples
    ///
    /// ```
    /// use ailloli_ui::{CaptureOpts, Window};
    /// let window = Window::<()>::new("main").capture(CaptureOpts::window());
    /// assert_eq!(window.id(), "main");
    /// ```
    pub fn validate_on_captured(
        &self,
        sources: &[WindowCaptureSource],
    ) -> Result<(), std::io::Error> {
        if !self.has_on_captured_listeners() {
            return Ok(());
        }
        let has_declarative = sources.iter().any(|source| !source.captures.is_empty());
        if !has_declarative {
            return Err(std::io::Error::new(
                std::io::ErrorKind::InvalidInput,
                "`on_captured` requires at least one `Window::capture(CaptureOpts::...)`",
            ));
        }
        Ok(())
    }

    /// Issues each window declaration through the handle and records its request ID.
    ///
    /// Repeated calls append new native requests and replace metadata only if the
    /// handle unexpectedly reuses an ID. No explicit queue bound is added here;
    /// native handle limits apply.
    ///
    /// # Examples
    ///
    /// ```
    /// use ailloli_ui::{CaptureOpts, Window};
    /// let captures = [CaptureOpts::window(), CaptureOpts::element("chart")];
    /// let window = Window::<()>::new("main").capture(captures[0].clone()).capture(captures[1].clone());
    /// assert_eq!(window.id(), "main");
    /// ```
    pub fn assemble_from_windows(&mut self, sources: &[WindowCaptureSource]) {
        for source in sources {
            for opts in &source.captures {
                let params = CaptureParams {
                    encode_png: opts.encode_png_enabled(),
                };
                let target = match opts.target() {
                    CaptureTargetSpec::Window => CaptureTarget::window(&source.window_id),
                    CaptureTargetSpec::Element { key } => {
                        CaptureTarget::element(&source.window_id, key.clone())
                    }
                };
                let id = self.handle.request(target, params);
                self.specs.insert(
                    id,
                    DeclarativeCaptureSpec {
                        window_id: source.window_id.clone(),
                        target: opts.target().clone(),
                        out_path: opts.out_path().map(Path::to_path_buf),
                    },
                );
            }
        }
    }

    /// Enables handle auto-exit when any request asks for it or writes a file.
    ///
    /// The method only enables the policy and never resets an already enabled
    /// handle when `sources` has no matching request.
    ///
    /// # Examples
    ///
    /// ```
    /// use ailloli_ui::CaptureOpts;
    /// let opts = CaptureOpts::window().file("capture.png");
    /// assert!(format!("{opts:?}").contains("capture.png"));
    /// ```
    pub fn apply_exit_policy(&self, sources: &[WindowCaptureSource]) {
        let should_exit = sources.iter().any(|source| {
            source
                .captures
                .iter()
                .any(|opts| opts.exit_after_enabled() || opts.out_path().is_some())
        });
        if should_exit {
            self.handle.set_exit_after_all_captures(true);
        }
    }

    /// Installs the completion callback that writes files and invokes listeners.
    ///
    /// Empty sessions install nothing. Unknown request IDs are ignored. File I/O
    /// errors are queued while listeners still receive the artifact.
    ///
    /// # Examples
    ///
    /// ```
    /// use ailloli_ui::CaptureHandle;
    /// let handle = CaptureHandle::new();
    /// assert!(!handle.has_pending());
    /// ```
    pub fn attach_completion_dispatch(&self) {
        if self.specs.is_empty() && self.listeners.is_empty() {
            return;
        }

        let specs = Arc::new(self.specs.clone());
        let listeners = self.listeners.clone();
        let io_errors = self.io_errors.clone();

        self.handle.on_complete(Arc::new(move |id, result| {
            let Some(spec) = specs.get(&id) else {
                return;
            };
            let artifact = build_artifact(spec, result);

            if let Some(path) = &spec.out_path {
                if let Err(err) = write_artifact_file(path, &artifact) {
                    io_errors.lock().expect("capture io_errors lock").push(err);
                }
            }

            for listener in &listeners {
                listener(artifact.clone());
            }
        }));
    }

    /// Removes and returns the most recently recorded capture file error.
    ///
    /// Despite the method name, storage is a vector and therefore uses LIFO
    /// order. Returns `None` for an empty or poisoned queue.
    ///
    /// # Examples
    ///
    /// ```
    /// let mut errors: Vec<std::io::Error> = Vec::new();
    /// assert!(errors.pop().is_none());
    /// ```
    pub fn take_first_io_error(&self) -> Option<std::io::Error> {
        self.io_errors.lock().ok()?.pop()
    }
}

/// Converts a native result into the stable public artifact shape.
///
/// Missing optional PNG data becomes an empty vector. Failures deliberately
/// discard dimensions, bytes, and bounds while preserving the cloned error.
fn build_artifact(
    spec: &DeclarativeCaptureSpec,
    result: &Result<CaptureResult, CaptureError>,
) -> CapturedArtifact {
    match result {
        Ok(res) => CapturedArtifact {
            window_id: spec.window_id.clone(),
            target: spec.target.clone(),
            width: res.frame.width,
            height: res.frame.height,
            png: res.frame.png_data.clone().unwrap_or_default(),
            rgba: res.frame.rgba.clone(),
            bounds_px: res.bounds_px,
            error: None,
        },
        Err(err) => CapturedArtifact {
            window_id: spec.window_id.clone(),
            target: spec.target.clone(),
            width: 0,
            height: 0,
            png: Vec::new(),
            rgba: Vec::new(),
            bounds_px: None,
            error: Some(err.clone()),
        },
    }
}

/// Writes a successful nonempty PNG, creating nonempty parent paths as needed.
///
/// # Errors
///
/// Returns the capture error as [`std::io::ErrorKind::Other`], rejects empty PNG
/// data as [`std::io::ErrorKind::InvalidData`], and propagates directory/file I/O.
fn write_artifact_file(path: &Path, artifact: &CapturedArtifact) -> std::io::Result<()> {
    if let Some(err) = &artifact.error {
        return Err(std::io::Error::other(err.to_string()));
    }
    if artifact.png.is_empty() {
        return Err(std::io::Error::new(
            std::io::ErrorKind::InvalidData,
            format!("capture PNG is empty for `{}`", path.display()),
        ));
    }
    if let Some(parent) = path.parent() {
        if !parent.as_os_str().is_empty() {
            std::fs::create_dir_all(parent)?;
        }
    }
    std::fs::write(path, &artifact.png)
}

#[cfg(test)]
/// Covers request assembly, aggregate exit policy, and result/error mapping.
mod tests {
    use super::*;

    #[test]
    fn assemble_from_windows_registers_window_and_element_requests() {
        let mut session = CaptureSession::default();
        session.assemble_from_windows(&[WindowCaptureSource {
            window_id: "main".to_string(),
            captures: vec![CaptureOpts::window(), CaptureOpts::element("hello-text")],
        }]);

        assert!(session.handle.has_pending_for_window("main"));
        assert_eq!(session.specs.len(), 2);
    }

    #[test]
    fn apply_exit_policy_enables_auto_exit_for_file_or_flag() {
        let session = CaptureSession::default();
        session.apply_exit_policy(&[WindowCaptureSource {
            window_id: "main".to_string(),
            captures: vec![CaptureOpts::window().file("out.png")],
        }]);
        assert!(session.handle.exit_after_all_captures());

        let session = CaptureSession::default();
        session.apply_exit_policy(&[WindowCaptureSource {
            window_id: "main".to_string(),
            captures: vec![CaptureOpts::window().exit_after(true)],
        }]);
        assert!(session.handle.exit_after_all_captures());

        let session = CaptureSession::default();
        session.apply_exit_policy(&[WindowCaptureSource {
            window_id: "main".to_string(),
            captures: vec![CaptureOpts::window()],
        }]);
        assert!(!session.handle.exit_after_all_captures());
    }

    #[test]
    fn build_artifact_maps_success_and_failure() {
        let spec = DeclarativeCaptureSpec {
            window_id: "main".to_string(),
            target: CaptureTargetSpec::Window,
            out_path: None,
        };
        let ok = build_artifact(
            &spec,
            &Ok(CaptureResult {
                request: ailloli_ui_winit::CaptureRequest {
                    id: 1,
                    target: CaptureTarget::window("main"),
                    params: CaptureParams::default(),
                },
                frame: ailloli_ui_render_wgpu::CapturedFrame {
                    width: 2,
                    height: 2,
                    format: ailloli_ui_render_wgpu::CapturedFrameFormat::Rgba8,
                    rgba: vec![0; 16],
                    png_data: Some(vec![1, 2, 3]),
                },
                bounds_px: None,
            }),
        );
        assert_eq!(ok.width, 2);
        assert_eq!(ok.png, vec![1, 2, 3]);
        assert!(ok.error.is_none());

        let err = build_artifact(
            &spec,
            &Err(CaptureError::WindowNotFound {
                window_id: "missing".to_string(),
            }),
        );
        assert!(err.error.is_some());
        assert!(err.png.is_empty());
    }
}
