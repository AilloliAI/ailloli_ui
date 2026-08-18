//! Declarative GPU capture options for [`App`](crate::App) visual tests and tooling.
//!
//! Prefer [`CaptureOpts`] on [`Window`](crate::Window) and optional [`AppBuilder::on_captured`](crate::AppBuilder::on_captured)
//! over manual [`CaptureHandle`] wiring for the common case.

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};

use ailloli_ui_core::Rect;
use ailloli_ui_render_wgpu::CaptureParams;
use ailloli_ui_winit::{
    CaptureError, CaptureHandle, CaptureRequestId, CaptureResult, CaptureTarget,
};

/// What to capture — the logical window id comes from [`Window::new`](crate::Window::new).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CaptureTargetSpec {
    Window,
    Element { key: String },
}

/// Declarative capture request attached to a [`Window`](crate::Window).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CaptureOpts {
    target: CaptureTargetSpec,
    encode_png: bool,
    exit_after: bool,
    out_path: Option<PathBuf>,
}

impl CaptureOpts {
    /// Capture the full window contents.
    pub fn window() -> Self {
        Self {
            target: CaptureTargetSpec::Window,
            encode_png: true,
            exit_after: false,
            out_path: None,
        }
    }

    /// Capture a cropped region for the element identified by [`View::key`](crate::View::key).
    pub fn element(key: impl Into<String>) -> Self {
        Self {
            target: CaptureTargetSpec::Element { key: key.into() },
            encode_png: true,
            exit_after: false,
            out_path: None,
        }
    }

    /// Writes the PNG to `path` when the capture completes (creates parent directories).
    pub fn file(mut self, path: impl Into<PathBuf>) -> Self {
        self.out_path = Some(path.into());
        self
    }

    /// When `true`, contributes to exiting the event loop after all captures complete.
    pub fn exit_after(mut self, value: bool) -> Self {
        self.exit_after = value;
        self
    }

    /// Controls whether PNG bytes are embedded in the GPU readback result.
    pub fn encode_png(mut self, value: bool) -> Self {
        self.encode_png = value;
        self
    }

    pub(crate) fn target(&self) -> &CaptureTargetSpec {
        &self.target
    }

    pub(crate) fn encode_png_enabled(&self) -> bool {
        self.encode_png
    }

    pub(crate) fn exit_after_enabled(&self) -> bool {
        self.exit_after
    }

    pub(crate) fn out_path(&self) -> Option<&Path> {
        self.out_path.as_deref()
    }
}

/// Completed capture payload delivered to [`AppBuilder::on_captured`](crate::AppBuilder::on_captured).
#[derive(Debug, Clone, PartialEq)]
pub struct CapturedArtifact {
    pub window_id: String,
    pub target: CaptureTargetSpec,
    pub width: u32,
    pub height: u32,
    pub png: Vec<u8>,
    pub rgba: Vec<u8>,
    pub bounds_px: Option<Rect>,
    pub error: Option<CaptureError>,
}

/// Window id + declarative capture declarations (used during [`AppBuilder::run`](crate::AppBuilder::run)).
#[derive(Debug, Clone)]
pub(crate) struct WindowCaptureSource {
    pub window_id: String,
    pub captures: Vec<CaptureOpts>,
}

#[derive(Clone)]
struct DeclarativeCaptureSpec {
    window_id: String,
    target: CaptureTargetSpec,
    out_path: Option<PathBuf>,
}

type UserListener = Arc<dyn Fn(CapturedArtifact) + Send + Sync>;

/// Owns the shared [`CaptureHandle`] and declarative capture wiring for one app run.
#[derive(Clone)]
pub(crate) struct CaptureSession {
    handle: CaptureHandle,
    specs: HashMap<CaptureRequestId, DeclarativeCaptureSpec>,
    listeners: Vec<UserListener>,
    io_errors: Arc<Mutex<Vec<std::io::Error>>>,
    explicit_handle: bool,
}

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

impl CaptureSession {
    pub fn use_explicit_handle(mut self, handle: CaptureHandle) -> Self {
        self.handle = handle;
        self.explicit_handle = true;
        self
    }

    pub fn has_explicit_handle(&self) -> bool {
        self.explicit_handle
    }

    pub fn handle(&self) -> &CaptureHandle {
        &self.handle
    }

    pub fn register_listener<F>(&mut self, listener: F)
    where
        F: Fn(CapturedArtifact) + Send + Sync + 'static,
    {
        self.listeners.push(Arc::new(listener));
    }

    pub fn has_on_captured_listeners(&self) -> bool {
        !self.listeners.is_empty()
    }

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

    pub fn take_first_io_error(&self) -> Option<std::io::Error> {
        self.io_errors.lock().ok()?.pop()
    }
}

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
