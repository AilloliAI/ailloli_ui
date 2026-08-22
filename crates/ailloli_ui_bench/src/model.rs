//! Versioned JSONL wire types, stable identifiers, and event correlation.
//!
//! Schema version one tolerates unknown JSON fields and event payloads so newer
//! producers remain inspectable by older readers. IDs are run-local unless a
//! type explicitly describes a logical host identity.

use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};

/// Version of the JSONL wire schema emitted by this crate.
///
/// # Examples
///
/// ```
/// assert_eq!(ailloli_ui_bench::SCHEMA_VERSION, 1);
/// ```
pub const SCHEMA_VERSION: u32 = 1;

/// Identifier of one benchmark run.
///
/// The value is opaque and serialized as a JSON string; this type does not
/// reject empty or duplicate caller-controlled identifiers.
///
/// # Examples
///
/// ```
/// use ailloli_ui_bench::RunId;
///
/// let id = RunId::new("run-42");
/// assert_eq!(id.as_str(), "run-42");
/// ```
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(transparent)]
pub struct RunId(String);

impl RunId {
    /// Creates a run identifier from a caller-controlled stable value.
    ///
    /// # Examples
    ///
    /// ```
    /// use ailloli_ui_bench::RunId;
    ///
    /// assert_eq!(RunId::new(String::from("candidate")).as_str(), "candidate");
    /// ```
    pub fn new(value: impl Into<String>) -> Self {
        Self(value.into())
    }

    /// Returns the serialized identifier.
    ///
    /// # Examples
    ///
    /// ```
    /// use ailloli_ui_bench::RunId;
    ///
    /// assert_eq!(RunId::new("baseline").as_str(), "baseline");
    /// ```
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

/// Monotonic identifier of an event within one run.
///
/// Zero is valid, although [`crate::BenchSession`] allocates IDs starting at
/// one and saturates instead of wrapping at `u64::MAX`.
///
/// # Examples
///
/// ```
/// use ailloli_ui_bench::EventId;
///
/// assert_eq!(EventId::new(7).get(), 7);
/// ```
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(transparent)]
pub struct EventId(u64);

impl EventId {
    /// Creates an event identifier.
    ///
    /// # Examples
    ///
    /// ```
    /// use ailloli_ui_bench::EventId;
    /// assert_eq!(EventId::new(0).get(), 0);
    /// ```
    pub const fn new(value: u64) -> Self {
        Self(value)
    }

    /// Returns the numeric identifier.
    ///
    /// # Examples
    ///
    /// ```
    /// use ailloli_ui_bench::EventId;
    /// assert_eq!(EventId::new(u64::MAX).get(), u64::MAX);
    /// ```
    pub const fn get(self) -> u64 {
        self.0
    }
}

/// Identifier of a rendered frame within one run.
///
/// # Examples
///
/// ```
/// use ailloli_ui_bench::FrameId;
/// assert_eq!(FrameId::new(12).get(), 12);
/// ```
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(transparent)]
pub struct FrameId(u64);

impl FrameId {
    /// Creates a frame identifier.
    ///
    /// # Examples
    ///
    /// ```
    /// use ailloli_ui_bench::FrameId;
    /// assert_eq!(FrameId::new(1).get(), 1);
    /// ```
    pub const fn new(value: u64) -> Self {
        Self(value)
    }

    /// Returns the numeric identifier.
    ///
    /// # Examples
    ///
    /// ```
    /// use ailloli_ui_bench::FrameId;
    /// assert_eq!(FrameId::new(99).get(), 99);
    /// ```
    pub const fn get(self) -> u64 {
        self.0
    }
}

/// Stable logical window identifier used by benchmark correlation.
///
/// # Examples
///
/// ```
/// use ailloli_ui_bench::BenchWindowId;
/// assert_eq!(BenchWindowId::new("main").as_str(), "main");
/// ```
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(transparent)]
pub struct BenchWindowId(String);

impl BenchWindowId {
    /// Creates a logical window identifier.
    ///
    /// # Examples
    ///
    /// ```
    /// use ailloli_ui_bench::BenchWindowId;
    /// assert_eq!(BenchWindowId::new(String::from("popup")).as_str(), "popup");
    /// ```
    pub fn new(value: impl Into<String>) -> Self {
        Self(value.into())
    }

    /// Returns the serialized identifier.
    ///
    /// # Examples
    ///
    /// ```
    /// use ailloli_ui_bench::BenchWindowId;
    /// assert_eq!(BenchWindowId::new("main").as_str(), "main");
    /// ```
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

/// Stable logical surface identifier used by benchmark correlation.
///
/// # Examples
///
/// ```
/// use ailloli_ui_bench::BenchSurfaceId;
/// assert_eq!(BenchSurfaceId::new("main-surface").as_str(), "main-surface");
/// ```
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(transparent)]
pub struct BenchSurfaceId(String);

impl BenchSurfaceId {
    /// Creates a logical surface identifier.
    ///
    /// # Examples
    ///
    /// ```
    /// use ailloli_ui_bench::BenchSurfaceId;
    /// assert_eq!(BenchSurfaceId::new(String::from("surface-2")).as_str(), "surface-2");
    /// ```
    pub fn new(value: impl Into<String>) -> Self {
        Self(value.into())
    }

    /// Returns the serialized identifier.
    ///
    /// # Examples
    ///
    /// ```
    /// use ailloli_ui_bench::BenchSurfaceId;
    /// assert_eq!(BenchSurfaceId::new("surface").as_str(), "surface");
    /// ```
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

/// Origin used by elapsed-time measurements in a run.
///
/// # Examples
///
/// ```
/// use ailloli_ui_bench::TimeOrigin;
/// assert_eq!(TimeOrigin::default(), TimeOrigin::AppRun);
/// ```
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
///
/// Optional values are absent when unknown; empty strings are not normalized.
/// Width and height are physical window pixels. Scale factors must be finite
/// and positive when validated by a session or comparison workflow.
///
/// # Examples
///
/// ```
/// use ailloli_ui_bench::RunMetadata;
///
/// let mut metadata = RunMetadata::default();
/// metadata.scenario = Some("scroll-tree".into());
/// metadata.window_width = Some(1280);
/// assert_eq!(metadata.scenario.as_deref(), Some("scroll-tree"));
/// ```
#[non_exhaustive]
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct RunMetadata {
    /// Stable benchmark scenario name, or `None` when unspecified.
    pub scenario: Option<String>,
    /// Project phase or experiment label, or `None` when unspecified.
    pub phase: Option<String>,
    /// Source revision measured by this run.
    pub git_revision: Option<String>,
    /// Digest of uncommitted source changes, when present.
    pub dirty_diff_hash: Option<String>,
    /// Build profile such as `release` or `debug`.
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
    /// Operating-system identity used for run compatibility checks.
    pub operating_system: Option<String>,
    /// Winit version used by the measured host, when applicable.
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
    /// GPU adapter identity reported by the renderer.
    pub gpu: Option<String>,
    /// Graphics driver identity/version reported by the renderer.
    pub driver: Option<String>,
    /// Requested window width in physical pixels.
    pub window_width: Option<u32>,
    /// Requested window height in physical pixels.
    pub window_height: Option<u32>,
    /// Requested logical-to-physical device-pixel ratio.
    pub scale_factor: Option<f64>,
    /// Device-pixel ratio read from the live native window.
    ///
    /// This is deliberately separate from [`Self::scale_factor`], which is a
    /// requested/configured value in version-one benchmark artifacts.
    pub observed_scale_factor: Option<f64>,
    /// Number of leading samples excluded from regression statistics.
    pub warmup_samples: Option<u32>,
    /// Number of samples intended for measured statistics.
    pub measured_samples: Option<u32>,
    /// Reference point for every `elapsed_us` value in this run.
    pub time_origin: TimeOrigin,
    /// Provider-specific diagnostics which do not warrant a schema revision.
    pub extensions: BTreeMap<String, serde_json::Value>,
}

impl RunMetadata {
    /// Overlays non-empty values from `update` onto this metadata set.
    ///
    /// `None` never erases an existing value. Extensions overwrite entries with
    /// the same key. A legacy `backend` update accompanied by GPU or driver
    /// metadata is migrated to `renderer_backend` and does not replace the
    /// original window backend.
    ///
    /// # Examples
    ///
    /// ```
    /// use ailloli_ui_bench::RunMetadata;
    ///
    /// let mut update = RunMetadata::default();
    /// update.renderer_backend = Some("vulkan".into());
    /// // Sessions apply this sparse update without clearing other metadata.
    /// assert_eq!(update.renderer_backend.as_deref(), Some("vulkan"));
    /// ```
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
///
/// # Examples
///
/// ```
/// use ailloli_ui_bench::SamplePhase;
/// assert_eq!(SamplePhase::default(), SamplePhase::Measured);
/// ```
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
///
/// # Examples
///
/// ```
/// use ailloli_ui_bench::MetricRole;
/// assert_eq!(MetricRole::default(), MetricRole::Diagnostic);
/// ```
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
///
/// All identifiers are optional. `presentation_generation` should be present
/// with `surface_id`; builders set the pair atomically. An empty cause list
/// means no predecessor was declared. Samples default to measured.
///
/// # Examples
///
/// ```
/// use ailloli_ui_bench::{EventContext, FrameId, SamplePhase};
///
/// let context = EventContext::default()
///     .with_frame(FrameId::new(3))
///     .with_sample_phase(SamplePhase::Warmup);
/// assert_eq!(context.frame_id, Some(FrameId::new(3)));
/// assert_eq!(context.sample_phase, SamplePhase::Warmup);
/// ```
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct EventContext {
    /// Frame containing the event, or `None` for run-level events.
    pub frame_id: Option<FrameId>,
    /// Logical host window associated with the event.
    pub logical_window_id: Option<BenchWindowId>,
    /// Logical presentation surface associated with the event.
    pub surface_id: Option<BenchSurfaceId>,
    /// Surface generation paired with [`Self::surface_id`].
    pub presentation_generation: Option<u64>,
    /// Earlier event IDs declared as causal predecessors.
    pub cause_event_ids: Vec<EventId>,
    /// Whether a numeric sample is warmup-only or measured.
    pub sample_phase: SamplePhase,
}

impl EventContext {
    /// Associates the event with a frame.
    ///
    /// # Examples
    ///
    /// ```
    /// use ailloli_ui_bench::{EventContext, FrameId};
    /// let context = EventContext::default().with_frame(FrameId::new(4));
    /// assert_eq!(context.frame_id, Some(FrameId::new(4)));
    /// ```
    pub fn with_frame(mut self, frame_id: FrameId) -> Self {
        self.frame_id = Some(frame_id);
        self
    }

    /// Associates the event with a logical window.
    ///
    /// # Examples
    ///
    /// ```
    /// use ailloli_ui_bench::{BenchWindowId, EventContext};
    /// let context = EventContext::default().with_window(BenchWindowId::new("main"));
    /// assert_eq!(context.logical_window_id.unwrap().as_str(), "main");
    /// ```
    pub fn with_window(mut self, window_id: BenchWindowId) -> Self {
        self.logical_window_id = Some(window_id);
        self
    }

    /// Associates the event with a logical surface and presentation generation.
    ///
    /// # Examples
    ///
    /// ```
    /// use ailloli_ui_bench::{BenchSurfaceId, EventContext};
    /// let context = EventContext::default().with_surface(BenchSurfaceId::new("s0"), 2);
    /// assert_eq!(context.presentation_generation, Some(2));
    /// ```
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
    ///
    /// Repeated IDs are retained in insertion order.
    ///
    /// # Examples
    ///
    /// ```
    /// use ailloli_ui_bench::{EventContext, EventId};
    /// let context = EventContext::default().caused_by(EventId::new(8));
    /// assert_eq!(context.cause_event_ids, vec![EventId::new(8)]);
    /// ```
    pub fn caused_by(mut self, event_id: EventId) -> Self {
        self.cause_event_ids.push(event_id);
        self
    }

    /// Marks whether this event is a warmup or measured sample.
    ///
    /// # Examples
    ///
    /// ```
    /// use ailloli_ui_bench::{EventContext, SamplePhase};
    /// let context = EventContext::default().with_sample_phase(SamplePhase::Warmup);
    /// assert_eq!(context.sample_phase, SamplePhase::Warmup);
    /// ```
    pub fn with_sample_phase(mut self, sample_phase: SamplePhase) -> Self {
        self.sample_phase = sample_phase;
        self
    }
}

/// One JSONL event payload.
///
/// Timestamps are wall-clock milliseconds since the Unix epoch. Durations are
/// microseconds. Dimensions and pixel counters use physical pixels unless a
/// field says otherwise. Numeric metric values must be finite before a session
/// accepts them.
///
/// # Examples
///
/// ```
/// use ailloli_ui_bench::Event;
///
/// let event = Event::Metric {
///     ts_ms: 1,
///     name: "layout_us".into(),
///     value: 42.0,
///     role: ailloli_ui_bench::MetricRole::GatingSteady,
/// };
/// assert!(matches!(event, Event::Metric { value: 42.0, .. }));
/// ```
#[non_exhaustive]
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum Event {
    /// Named checkpoint (including span end markers).
    Marker {
        /// Wall-clock timestamp in milliseconds since the Unix epoch.
        ts_ms: u128,
        /// Caller-selected checkpoint name.
        name: String,
    },

    /// Window resize queued before GPU apply.
    ResizePending {
        /// Wall-clock timestamp in milliseconds since the Unix epoch.
        ts_ms: u128,
        /// Pending physical width in pixels.
        w: u32,
        /// Pending physical height in pixels.
        h: u32,
    },
    /// Surface resize applied on the GPU path.
    ResizeApply {
        /// Wall-clock timestamp in milliseconds since the Unix epoch.
        ts_ms: u128,
        /// Applied physical width in pixels.
        w: u32,
        /// Applied physical height in pixels.
        h: u32,
        /// GPU-path resize duration in microseconds.
        dur_us: u128,
    },
    /// GPU surface configuration.
    SurfaceConfigure {
        /// Wall-clock timestamp in milliseconds since the Unix epoch.
        ts_ms: u128,
        /// Configured physical width in pixels.
        w: u32,
        /// Configured physical height in pixels.
        h: u32,
        /// Surface configuration duration in microseconds.
        dur_us: u128,
    },

    /// Full frame presented to the swapchain.
    RenderFrame {
        /// Wall-clock timestamp in milliseconds since the Unix epoch.
        ts_ms: u128,
        /// Complete frame duration in microseconds.
        dur_us: u128,
    },
    /// Failed to acquire the current swapchain texture.
    GetCurrentTextureErr {
        /// Wall-clock timestamp in milliseconds since the Unix epoch.
        ts_ms: u128,
        /// Redacted diagnostic representation of the acquisition error.
        err: String,
    },

    /// Window maximize state toggled.
    MaximizeToggle {
        /// Wall-clock timestamp in milliseconds since the Unix epoch.
        ts_ms: u128,
        /// `true` when transitioning to maximized state.
        to: bool,
    },
    /// Event loop about to wait; may schedule redraw after resize.
    AboutToWaitRedraw {
        /// Wall-clock timestamp in milliseconds since the Unix epoch.
        ts_ms: u128,
        /// Whether a queued resize still awaits renderer application.
        awaiting_resize: bool,
    },
    /// Sampled inner window size.
    WindowInnerSizeSample {
        /// Wall-clock timestamp in milliseconds since the Unix epoch.
        ts_ms: u128,
        /// Observed physical width in pixels.
        w: u32,
        /// Observed physical height in pixels.
        h: u32,
    },

    /// Structured numeric sample.
    Metric {
        /// Wall-clock timestamp in milliseconds since the Unix epoch.
        ts_ms: u128,
        /// Stable metric name used to group samples.
        name: String,
        /// Finite numeric sample in the unit encoded by `name`/documentation.
        value: f64,
        /// Explicit comparison behavior. Missing roles in older artifacts
        /// deserialize as [`MetricRole::Diagnostic`].
        #[serde(default)]
        role: MetricRole,
    },

    /// One text pipeline frame (layout + paint + render).
    TextPipelineFrame {
        /// Wall-clock timestamp in milliseconds since the Unix epoch.
        ts_ms: u128,
        /// Text layout duration in microseconds.
        layout_us: u128,
        /// Text paint-command generation duration in microseconds.
        paint_us: u128,
        /// Text renderer duration in microseconds.
        render_us: u128,
        /// Number of text draw commands produced for the frame.
        draw_text_cmds: u32,
    },

    /// Glyph atlas cache statistics for one frame.
    TextAtlasFrame {
        /// Wall-clock timestamp in milliseconds since the Unix epoch.
        ts_ms: u128,
        /// Glyph cache lookup hits this frame.
        hits: u32,
        /// Glyph cache lookup misses this frame.
        misses: u32,
        /// Glyphs rasterized and uploaded this frame.
        rasterized: u32,
        /// Atlas pages reset for reuse this frame.
        resets: u32,
        /// Evictions blocked because all candidate pages were pinned.
        evictions_blocked: u32,
        /// Glyphs skipped because no atlas allocation was available.
        glyphs_skipped: u32,
        /// Atlas pages active after the frame.
        pages_active: u32,
    },

    /// Isolated compositor metrics for one surface-backed frame.
    IsolatedCompositorFrame {
        /// Wall-clock timestamp in milliseconds since the Unix epoch.
        ts_ms: u128,
        /// Scenario label copied into renderer metrics; empty means unspecified.
        scenario: String,
        /// Number of isolated offscreen passes rendered.
        isolated_pass_count: u32,
        /// Total physical pixels covered by isolated targets.
        isolated_pixels_total: u64,
        /// Total physical pixels processed by blur passes.
        blur_pixels_total: u64,
        /// Peak bytes retained by the offscreen texture pool.
        offscreen_peak_bytes: u64,
        /// Pool leases served by an existing allocation.
        pool_reuse_hits: u32,
        /// New offscreen allocations performed this frame.
        pool_allocs: u32,
        /// Reuse hits divided by hits plus allocations, in `0.0..=1.0`.
        pool_reuse_ratio: f64,
        /// Number of separable blur passes recorded.
        blur_pass_count: u32,
        /// Isolated passes that required a stencil attachment.
        stencil_offscreen_count: u32,
        /// Total isolated-effect downgrades.
        downgrade_count: u32,
        /// Blur radii reduced by policy limits.
        downgrade_blur_clamped: u32,
        /// Offscreen surfaces reduced by dimension/pixel limits.
        downgrade_surface_clamped: u32,
        /// Isolated passes skipped by the byte budget.
        downgrade_bytes_skipped: u32,
        /// Backdrop regions captured from the main framebuffer.
        backdrop_capture_count: u32,
        /// Total physical pixels captured for backdrop effects.
        backdrop_pixels_total: u64,
        /// Blur passes applied to captured backdrop regions.
        backdrop_blur_pass_count: u32,
        /// Backdrop effects skipped by the capture budget.
        downgrade_backdrop_skipped: u32,
        /// Destination regions captured for non-normal blending.
        blend_capture_count: u32,
        /// Shader blend composites recorded.
        blend_composite_count: u32,
        /// Destination-aware blends downgraded by policy limits.
        downgrade_blend_skipped: u32,
    },
}

/// Parsed `run_start` line.
///
/// # Examples
///
/// ```
/// use ailloli_ui_bench::{RunId, RunMetadata, RunStartRecord, SCHEMA_VERSION};
/// let record = RunStartRecord {
///     schema_version: SCHEMA_VERSION,
///     run_id: RunId::new("run"),
///     started_unix_ms: 1,
///     metadata: RunMetadata::default(),
/// };
/// assert_eq!(record.schema_version, 1);
/// ```
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RunStartRecord {
    /// Wire schema version used to encode this record.
    pub schema_version: u32,
    /// Run identity shared by every record in the file.
    pub run_id: RunId,
    /// Run start in milliseconds since the Unix epoch.
    pub started_unix_ms: u128,
    /// Initial reproducibility metadata snapshot.
    pub metadata: RunMetadata,
}

/// Parsed `metadata_update` line.
///
/// # Examples
///
/// ```
/// use ailloli_ui_bench::{MetadataUpdateRecord, RunId, RunMetadata, SCHEMA_VERSION};
/// let record = MetadataUpdateRecord {
///     schema_version: SCHEMA_VERSION,
///     run_id: RunId::new("run"),
///     elapsed_us: 10,
///     metadata: RunMetadata::default(),
/// };
/// assert_eq!(record.elapsed_us, 10);
/// ```
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MetadataUpdateRecord {
    /// Wire schema version used to encode this record.
    pub schema_version: u32,
    /// Run identity shared by every record in the file.
    pub run_id: RunId,
    /// Microseconds since the run's declared [`TimeOrigin`].
    pub elapsed_us: u128,
    /// Sparse metadata values overlaid onto earlier metadata.
    pub metadata: RunMetadata,
}

/// Parsed correlated event line. The payload remains a JSON value so readers
/// can retain events introduced by future schema revisions.
///
/// # Examples
///
/// ```
/// use ailloli_ui_bench::{BenchEventRecord, EventContext, EventId, RunId, SCHEMA_VERSION};
/// let record = BenchEventRecord {
///     schema_version: SCHEMA_VERSION,
///     run_id: RunId::new("run"),
///     event_id: EventId::new(1),
///     elapsed_us: 20,
///     context: EventContext::default(),
///     event: serde_json::json!({"kind": "marker", "name": "ready"}),
/// };
/// assert_eq!(record.event_id.get(), 1);
/// ```
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BenchEventRecord {
    /// Wire schema version used to encode this record.
    pub schema_version: u32,
    /// Run identity shared by every record in the file.
    pub run_id: RunId,
    /// Strictly increasing event identifier within the run.
    pub event_id: EventId,
    /// Microseconds since the run's declared [`TimeOrigin`].
    pub elapsed_us: u128,
    /// Optional frame/window/surface/cause/sample correlation fields.
    #[serde(flatten)]
    pub context: EventContext,
    /// Forward-compatible event object, including its `kind` discriminator.
    pub event: serde_json::Value,
}

/// Parsed `run_end` line.
///
/// # Examples
///
/// ```
/// use ailloli_ui_bench::{RunEndRecord, RunId, SCHEMA_VERSION};
/// let record = RunEndRecord {
///     schema_version: SCHEMA_VERSION,
///     run_id: RunId::new("run"),
///     elapsed_us: 100,
///     valid: true,
///     dropped_records: 0,
///     records_written: 3,
/// };
/// assert!(record.valid);
/// ```
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RunEndRecord {
    /// Wire schema version used to encode this record.
    pub schema_version: u32,
    /// Run identity shared by every record in the file.
    pub run_id: RunId,
    /// Microseconds since the run's declared [`TimeOrigin`].
    pub elapsed_us: u128,
    /// Whether the run remained suitable for gate comparison.
    pub valid: bool,
    /// Records rejected because the bounded writer queue was full.
    pub dropped_records: u64,
    /// Records serialized before this terminal record, excluding `run_end`.
    pub records_written: u64,
}

/// Internal typed representation serialized into JSONL wire records.
///
/// # Examples
///
/// ```
/// // Public readers expose the corresponding tolerant `LogRecord` envelope.
/// let record = ailloli_ui_bench::LogRecord::Unknown(serde_json::json!({
///     "record_type": "future"
/// }));
/// assert!(matches!(record, ailloli_ui_bench::LogRecord::Unknown(_)));
/// ```
#[derive(Debug, Serialize)]
#[serde(tag = "record_type", rename_all = "snake_case")]
pub(crate) enum WireRecord {
    /// Initial metadata and run identity.
    RunStart(RunStartRecord),
    /// Sparse late metadata update.
    MetadataUpdate(MetadataUpdateRecord),
    /// Correlated event with a forward-compatible payload.
    Event(BenchEventRecord),
    /// Terminal validity and writer-count summary.
    RunEnd(RunEndRecord),
}
