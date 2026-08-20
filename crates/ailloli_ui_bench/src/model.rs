use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};

/// Version of the JSONL wire schema emitted by this crate.
pub const SCHEMA_VERSION: u32 = 1;

/// Identifier of one benchmark run.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(transparent)]
pub struct RunId(String);

impl RunId {
    /// Creates a run identifier from a caller-controlled stable value.
    pub fn new(value: impl Into<String>) -> Self {
        Self(value.into())
    }

    /// Returns the serialized identifier.
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

/// Monotonic identifier of an event within one run.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(transparent)]
pub struct EventId(u64);

impl EventId {
    /// Creates an event identifier.
    pub const fn new(value: u64) -> Self {
        Self(value)
    }

    /// Returns the numeric identifier.
    pub const fn get(self) -> u64 {
        self.0
    }
}

/// Identifier of a rendered frame within one run.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(transparent)]
pub struct FrameId(u64);

impl FrameId {
    /// Creates a frame identifier.
    pub const fn new(value: u64) -> Self {
        Self(value)
    }

    /// Returns the numeric identifier.
    pub const fn get(self) -> u64 {
        self.0
    }
}

/// Stable logical window identifier used by benchmark correlation.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(transparent)]
pub struct BenchWindowId(String);

impl BenchWindowId {
    /// Creates a logical window identifier.
    pub fn new(value: impl Into<String>) -> Self {
        Self(value.into())
    }

    /// Returns the serialized identifier.
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

/// Stable logical surface identifier used by benchmark correlation.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(transparent)]
pub struct BenchSurfaceId(String);

impl BenchSurfaceId {
    /// Creates a logical surface identifier.
    pub fn new(value: impl Into<String>) -> Self {
        Self(value.into())
    }

    /// Returns the serialized identifier.
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

/// Origin used by elapsed-time measurements in a run.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum TimeOrigin {
    /// Initialization happened at the start of the child process.
    ProcessMain,
    /// Initialization happened when the application entered its run path.
    #[default]
    AppRun,
}

/// Reproducibility metadata for one benchmark run.
///
/// Unknown JSON fields are intentionally tolerated by readers. Use [`Default`]
/// and mutate the public fields instead of constructing this non-exhaustive
/// structure as a literal.
#[non_exhaustive]
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct RunMetadata {
    pub scenario: Option<String>,
    pub phase: Option<String>,
    pub git_revision: Option<String>,
    pub dirty_diff_hash: Option<String>,
    pub profile: Option<String>,
    /// Stable identity of the executable or harness which produced the run.
    pub harness: Option<String>,
    /// Compilation target used for the measured executable, when known.
    pub target: Option<String>,
    /// Stable caller-provided machine identity, when available.
    ///
    /// Producers should use an opaque lab identifier rather than a hostname or
    /// another value which could disclose user information.
    pub machine: Option<String>,
    pub operating_system: Option<String>,
    pub winit_version: Option<String>,
    /// Legacy backend field retained for version-one artifacts.
    ///
    /// New producers should use [`Self::window_backend`] for the effective
    /// winit backend and [`Self::renderer_backend`] for the graphics backend.
    pub backend: Option<String>,
    /// Effective window-system backend observed by the winit adapter.
    pub window_backend: Option<String>,
    /// Effective graphics backend reported by the renderer adapter.
    pub renderer_backend: Option<String>,
    pub gpu: Option<String>,
    pub driver: Option<String>,
    pub window_width: Option<u32>,
    pub window_height: Option<u32>,
    pub scale_factor: Option<f64>,
    /// Device-pixel ratio read from the live native window.
    ///
    /// This is deliberately separate from [`Self::scale_factor`], which is a
    /// requested/configured value in version-one benchmark artifacts.
    pub observed_scale_factor: Option<f64>,
    pub warmup_samples: Option<u32>,
    pub measured_samples: Option<u32>,
    pub time_origin: TimeOrigin,
    /// Provider-specific diagnostics which do not warrant a schema revision.
    pub extensions: BTreeMap<String, serde_json::Value>,
}

impl RunMetadata {
    /// Overlays non-empty values from `update` onto this metadata set.
    pub(crate) fn apply_update(&mut self, update: &Self) {
        macro_rules! apply_option {
            ($field:ident) => {
                if update.$field.is_some() {
                    self.$field.clone_from(&update.$field);
                }
            };
        }

        apply_option!(scenario);
        apply_option!(phase);
        apply_option!(git_revision);
        apply_option!(dirty_diff_hash);
        apply_option!(profile);
        apply_option!(harness);
        apply_option!(target);
        apply_option!(machine);
        apply_option!(operating_system);
        apply_option!(winit_version);
        // Older renderer integrations published their WGPU backend through
        // `backend` together with GPU/driver metadata. Preserve the original
        // window backend and migrate that late update into the dedicated
        // renderer field instead of overwriting Wayland/X11.
        let inferred_renderer_backend = update.renderer_backend.clone().or_else(|| {
            if update.gpu.is_some() || update.driver.is_some() {
                update.backend.clone()
            } else {
                None
            }
        });
        if update.backend.is_some() && inferred_renderer_backend.is_none() {
            self.backend.clone_from(&update.backend);
        }
        apply_option!(window_backend);
        if inferred_renderer_backend.is_some() {
            self.renderer_backend = inferred_renderer_backend;
        }
        apply_option!(gpu);
        apply_option!(driver);
        apply_option!(window_width);
        apply_option!(window_height);
        apply_option!(scale_factor);
        apply_option!(observed_scale_factor);
        apply_option!(warmup_samples);
        apply_option!(measured_samples);
        self.extensions.extend(update.extensions.clone());
    }
}

/// Classification of a sample for deterministic warmup exclusion.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SamplePhase {
    /// Sample used only to warm caches and pipelines.
    Warmup,
    /// Sample included in regression statistics.
    #[default]
    Measured,
}

/// Regression role carried by an explicit numeric metric.
///
/// Legacy metrics and provider events without a role are diagnostics. This
/// keeps existing JSONL artifacts readable without accidentally promoting
/// incidental counters into release gates.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum MetricRole {
    /// Requires at least 30 measured samples and gates median plus p95.
    GatingSteady,
    /// Requires at least five independent measured processes and gates median.
    GatingColdStart,
    /// Informational metric requiring only one sample and never blocking.
    #[default]
    Diagnostic,
    /// Correctness counter whose candidate samples must all equal zero.
    Correctness,
}

/// Correlation attached to a recorded event.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct EventContext {
    pub frame_id: Option<FrameId>,
    pub logical_window_id: Option<BenchWindowId>,
    pub surface_id: Option<BenchSurfaceId>,
    pub presentation_generation: Option<u64>,
    pub cause_event_ids: Vec<EventId>,
    pub sample_phase: SamplePhase,
}

impl EventContext {
    /// Associates the event with a frame.
    pub fn with_frame(mut self, frame_id: FrameId) -> Self {
        self.frame_id = Some(frame_id);
        self
    }

    /// Associates the event with a logical window.
    pub fn with_window(mut self, window_id: BenchWindowId) -> Self {
        self.logical_window_id = Some(window_id);
        self
    }

    /// Associates the event with a logical surface and presentation generation.
    pub fn with_surface(
        mut self,
        surface_id: BenchSurfaceId,
        presentation_generation: u64,
    ) -> Self {
        self.surface_id = Some(surface_id);
        self.presentation_generation = Some(presentation_generation);
        self
    }

    /// Adds a causal predecessor.
    pub fn caused_by(mut self, event_id: EventId) -> Self {
        self.cause_event_ids.push(event_id);
        self
    }

    /// Marks whether this event is a warmup or measured sample.
    pub fn with_sample_phase(mut self, sample_phase: SamplePhase) -> Self {
        self.sample_phase = sample_phase;
        self
    }
}

/// One JSONL event payload.
#[non_exhaustive]
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum Event {
    /// Named checkpoint (including span end markers).
    Marker { ts_ms: u128, name: String },

    /// Window resize queued before GPU apply.
    ResizePending { ts_ms: u128, w: u32, h: u32 },
    /// Surface resize applied on the GPU path.
    ResizeApply {
        ts_ms: u128,
        w: u32,
        h: u32,
        dur_us: u128,
    },
    /// GPU surface configuration.
    SurfaceConfigure {
        ts_ms: u128,
        w: u32,
        h: u32,
        dur_us: u128,
    },

    /// Full frame presented to the swapchain.
    RenderFrame { ts_ms: u128, dur_us: u128 },
    /// Failed to acquire the current swapchain texture.
    GetCurrentTextureErr { ts_ms: u128, err: String },

    /// Window maximize state toggled.
    MaximizeToggle { ts_ms: u128, to: bool },
    /// Event loop about to wait; may schedule redraw after resize.
    AboutToWaitRedraw { ts_ms: u128, awaiting_resize: bool },
    /// Sampled inner window size.
    WindowInnerSizeSample { ts_ms: u128, w: u32, h: u32 },

    /// Structured numeric sample.
    Metric {
        ts_ms: u128,
        name: String,
        value: f64,
        /// Explicit comparison behavior. Missing roles in older artifacts
        /// deserialize as [`MetricRole::Diagnostic`].
        #[serde(default)]
        role: MetricRole,
    },

    /// One text pipeline frame (layout + paint + render).
    TextPipelineFrame {
        ts_ms: u128,
        layout_us: u128,
        paint_us: u128,
        render_us: u128,
        draw_text_cmds: u32,
    },

    /// Glyph atlas cache statistics for one frame.
    TextAtlasFrame {
        ts_ms: u128,
        hits: u32,
        misses: u32,
        rasterized: u32,
        resets: u32,
        evictions_blocked: u32,
        glyphs_skipped: u32,
        pages_active: u32,
    },

    /// Isolated compositor metrics for one surface-backed frame.
    IsolatedCompositorFrame {
        ts_ms: u128,
        scenario: String,
        isolated_pass_count: u32,
        isolated_pixels_total: u64,
        blur_pixels_total: u64,
        offscreen_peak_bytes: u64,
        pool_reuse_hits: u32,
        pool_allocs: u32,
        pool_reuse_ratio: f64,
        blur_pass_count: u32,
        stencil_offscreen_count: u32,
        downgrade_count: u32,
        downgrade_blur_clamped: u32,
        downgrade_surface_clamped: u32,
        downgrade_bytes_skipped: u32,
        backdrop_capture_count: u32,
        backdrop_pixels_total: u64,
        backdrop_blur_pass_count: u32,
        downgrade_backdrop_skipped: u32,
        blend_capture_count: u32,
        blend_composite_count: u32,
        downgrade_blend_skipped: u32,
    },
}

/// Parsed `run_start` line.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RunStartRecord {
    pub schema_version: u32,
    pub run_id: RunId,
    pub started_unix_ms: u128,
    pub metadata: RunMetadata,
}

/// Parsed `metadata_update` line.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MetadataUpdateRecord {
    pub schema_version: u32,
    pub run_id: RunId,
    pub elapsed_us: u128,
    pub metadata: RunMetadata,
}

/// Parsed correlated event line. The payload remains a JSON value so readers
/// can retain events introduced by future schema revisions.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BenchEventRecord {
    pub schema_version: u32,
    pub run_id: RunId,
    pub event_id: EventId,
    pub elapsed_us: u128,
    #[serde(flatten)]
    pub context: EventContext,
    pub event: serde_json::Value,
}

/// Parsed `run_end` line.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RunEndRecord {
    pub schema_version: u32,
    pub run_id: RunId,
    pub elapsed_us: u128,
    pub valid: bool,
    pub dropped_records: u64,
    pub records_written: u64,
}

#[derive(Debug, Serialize)]
#[serde(tag = "record_type", rename_all = "snake_case")]
pub(crate) enum WireRecord {
    RunStart(RunStartRecord),
    MetadataUpdate(MetadataUpdateRecord),
    Event(BenchEventRecord),
    RunEnd(RunEndRecord),
}
