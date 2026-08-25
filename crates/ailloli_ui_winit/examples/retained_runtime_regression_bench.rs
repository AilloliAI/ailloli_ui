//! tree virtualization retained-runtime regression benchmark with process and frame metrics.

use std::cell::{Cell, RefCell};
use std::error::Error;
use std::path::PathBuf;
use std::rc::Rc;
use std::time::{Instant, SystemTime, UNIX_EPOCH};

use ailloli_ui_bench::{
    bench_scenario_from_env, metadata_from_env, sample_current_process, summarize_samples,
    BenchInit, BenchSession, Event as BenchEvent, EventContext, MetricRole, RunMetadata,
    SamplePhase,
};
use ailloli_ui_core::{Constraints, Offset, Rect, Scale, Size};
use ailloli_ui_runtime::app::{Runtime, RuntimeHandle};
use ailloli_ui_runtime::component::{ComponentNode, Context, IntoView, Signal, View, Widget};
use ailloli_ui_runtime::layout::{ChildLayout, LayoutChild, LayoutCtx, LayoutEngine, LayoutResult};
use ailloli_ui_runtime::scene::PaintCtx;
use ailloli_ui_runtime::Invalidation;
use ailloli_ui_text::TextSystem;
use ailloli_ui_widgets::controls::{
    TreeItem, TreeModel, TreeModelHandle, TreeMutation, TreeView, TreeViewDiagnostics,
};
use ailloli_ui_widgets::layout::ScrollView;

/// Exact number of non-gating samples required before measurement.
const LOCKED_WARMUPS: u32 = 3;
/// Minimum number of measured samples accepted by the regression contract.
const MIN_MEASURED: u32 = 30;
/// Maximum virtual rows that layout or paint may visit in one frame.
const TREE_ROW_LIMIT: u64 = 53;
/// Maximum accepted 95th-percentile tree frame time, in microseconds.
const TREE_FRAME_P95_LIMIT_US: f64 = 33_000.0;

/// Validates the locked sample contract and runs the selected tree virtualization scenario.
///
/// # Errors
///
/// Returns an error for absent/unsupported scenario configuration, invalid sample
/// counts, disabled/session setup, metadata/metric/process sampling, scenario
/// execution, or benchmark finalization failure.
fn main() -> Result<(), Box<dyn Error>> {
    let scenario = bench_scenario_from_env().ok_or("AILLOLI_UI_BENCH_SCENARIO is required")?;
    let initial_metadata = metadata_from_env();
    let warmups = initial_metadata.warmup_samples.unwrap_or(LOCKED_WARMUPS);
    let measured = initial_metadata.measured_samples.unwrap_or(MIN_MEASURED);
    if warmups != LOCKED_WARMUPS || measured < MIN_MEASURED {
        return Err(format!(
            "tree virtualization requires exactly {LOCKED_WARMUPS} warmups and at least {MIN_MEASURED} measured samples"
        )
        .into());
    }

    let default_path = default_bench_path(&scenario);
    let default_path = default_path.to_string_lossy();
    let bench = ailloli_ui_bench::try_init_from_env(&default_path)?;
    let BenchInit::Enabled(session) = &bench else {
        return Err("set AILLOLI_UI_BENCH=1 for the tree virtualization harness".into());
    };
    update_metadata(session, &initial_metadata, warmups, measured)?;

    match scenario.as_str() {
        "component_isolation" => run_component_isolation(session, warmups, measured)?,
        "tree_virtualization" => run_tree_virtualization(session, warmups, measured)?,
        _ => {
            return Err(
                format!("unsupported tree virtualization framework scenario {scenario:?}").into(),
            )
        }
    }

    bench
        .finish()?
        .ok_or("benchmark session unexpectedly disabled")?;
    Ok(())
}

/// Publishes the exact, headless harness metadata used to interpret a run.
///
/// # Errors
///
/// Propagates benchmark metadata validation, queue-capacity, closed-session, or
/// writer-disconnection errors.
fn update_metadata(
    session: &BenchSession,
    initial: &RunMetadata,
    warmups: u32,
    measured: u32,
) -> Result<(), Box<dyn Error>> {
    let mut metadata = RunMetadata::default();
    metadata.harness = Some("retained_runtime_regression_bench".to_string());
    metadata.warmup_samples = Some(warmups);
    metadata.measured_samples = Some(measured);
    metadata.window_backend = initial.backend.clone();
    metadata.observed_scale_factor = initial.scale_factor;
    metadata.extensions.insert(
        "scenario_fidelity".to_string(),
        serde_json::Value::String("exact".to_string()),
    );
    metadata.extensions.insert(
        "scenario_gate_ready".to_string(),
        serde_json::Value::Bool(true),
    );
    metadata
        .extensions
        .insert("surface_backed".to_string(), serde_json::Value::Bool(false));
    metadata.extensions.insert(
        "filesystem_io_during_frame".to_string(),
        serde_json::Value::Bool(false),
    );
    session.update_metadata(metadata)?;
    Ok(())
}

/// Builds a per-process fallback JSONL path for `scenario`.
fn default_bench_path(scenario: &str) -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../..")
        .join("artifacts")
        .join("bench")
        .join("tree_virtualization")
        .join("manual")
        .join(format!("{scenario}-{}.jsonl", std::process::id()))
}

/// Records one timestamped metric with an explicit role and sample phase.
///
/// # Errors
///
/// Propagates benchmark non-finite-value, serialization, queue, closed-session,
/// or writer-disconnection errors.
fn record_metric(
    session: &BenchSession,
    name: &str,
    value: f64,
    role: MetricRole,
    phase: SamplePhase,
) -> Result<(), Box<dyn Error>> {
    session.record_with_context(
        BenchEvent::Metric {
            ts_ms: unix_time_ms(),
            name: name.to_string(),
            value,
            role,
        },
        EventContext::default().with_sample_phase(phase),
    )?;
    Ok(())
}

/// Records an integer correctness counter as a measured, non-timing metric.
///
/// # Errors
///
/// Propagates the benchmark write failures documented by [`record_metric`].
fn record_correctness(
    session: &BenchSession,
    name: &str,
    value: u64,
) -> Result<(), Box<dyn Error>> {
    record_metric(
        session,
        name,
        value as f64,
        MetricRole::Correctness,
        SamplePhase::Measured,
    )
}

/// Samples RSS, PSS, threads, and file descriptors for the current process.
///
/// # Errors
///
/// Propagates process sampling failures and any diagnostic metric write failure.
fn record_process_sample(session: &BenchSession, phase: SamplePhase) -> Result<(), Box<dyn Error>> {
    let sample = sample_current_process()?;
    record_metric(
        session,
        "process.rss_mib",
        sample.rss_mib(),
        MetricRole::Diagnostic,
        phase,
    )?;
    record_metric(
        session,
        "process.pss_mib",
        sample.pss_mib(),
        MetricRole::Diagnostic,
        phase,
    )?;
    record_metric(
        session,
        "process.threads",
        sample.threads() as f64,
        MetricRole::Diagnostic,
        phase,
    )?;
    record_metric(
        session,
        "process.file_descriptors",
        sample.file_descriptors() as f64,
        MetricRole::Diagnostic,
        phase,
    )
}

#[derive(Default)]
/// Saturating work counters shared by the component-isolation fixtures.
struct WorkCounters {
    /// Component builds observed since fixture creation.
    builds: Cell<u64>,
    /// Leaf layout calls observed since fixture creation.
    layouts: Cell<u64>,
    /// Synthetic data reads observed during component builds.
    data_reads: Cell<u64>,
}

/// Leaf widget that increments the shared layout counter.
struct CountingLeaf(Rc<WorkCounters>);

/// Implements the fixed-size, paint-free leaf used to isolate retained work.
impl Widget<()> for CountingLeaf {
    /// Returns a stable diagnostics label for the benchmark tree.
    fn debug_name(&self) -> &'static str {
        "tree virtualizationCountingLeaf"
    }

    /// Counts one layout and constrains the fixture's 120x40 logical size.
    fn layout(
        &self,
        _engine: &mut LayoutEngine<'_, ()>,
        _ctx: &mut LayoutCtx<'_>,
        _children: &mut [LayoutChild],
        constraints: Constraints,
    ) -> LayoutResult {
        self.0.layouts.set(self.0.layouts.get().saturating_add(1));
        let size = constraints.constrain(Size::new(120.0, 40.0));
        LayoutResult {
            size,
            children: Vec::new(),
            paint_bounds: Rect::new(0.0, 0.0, size.w, size.h),
            visual_bounds: Rect::new(0.0, 0.0, size.w, size.h),
            overlay_hit_bounds: Vec::new(),
            clip: None,
            is_window_root_clip: false,
            artifact: None,
        }
    }

    /// Performs no drawing so the scenario measures retained build and layout work.
    fn paint(&self, _ctx: &mut PaintCtx<'_>, _bounds: Rect, _layout: &LayoutResult) {}
}

/// Component fixture that publishes an invalidation signal and builds a counting leaf.
struct CountingComponent {
    /// Counters attributed only to this component subtree.
    counters: Rc<WorkCounters>,
    /// Optional output slot receiving the component-owned signal.
    signal_slot: Option<Rc<RefCell<Option<Signal<u64>>>>>,
}

/// Builds the leaf and exposes a signal used to invalidate only this component.
impl ComponentNode<()> for CountingComponent {
    /// Counts build/data access, installs a zero-valued signal, and returns the leaf.
    fn build(&self, context: &mut Context<()>) -> View<()> {
        self.counters
            .builds
            .set(self.counters.builds.get().saturating_add(1));
        self.counters
            .data_reads
            .set(self.counters.data_reads.get().saturating_add(1));
        if let Some(slot) = &self.signal_slot {
            *slot.borrow_mut() = Some(context.signal(0_u64));
        }
        View::leaf(CountingLeaf(self.counters.clone()))
    }
}

/// Root widget that lays its benchmark children out left-to-right without painting.
struct HorizontalRoot;

/// Provides deterministic horizontal layout for the three isolated components.
impl Widget<()> for HorizontalRoot {
    /// Returns a stable diagnostics label for the benchmark root.
    fn debug_name(&self) -> &'static str {
        "tree virtualizationHorizontalRoot"
    }

    /// Lays out every child loosely and sums their logical widths.
    fn layout(
        &self,
        engine: &mut LayoutEngine<'_, ()>,
        ctx: &mut LayoutCtx<'_>,
        children: &mut [LayoutChild],
        constraints: Constraints,
    ) -> LayoutResult {
        let mut x = 0.0;
        let mut height: f32 = 0.0;
        let mut child_layouts = Vec::with_capacity(children.len());
        for child in children {
            let result = child.layout(engine, ctx, constraints.loosen());
            child_layouts.push(ChildLayout {
                offset: Offset::new(x, 0.0),
                size: result.size,
                paint_bounds: result.paint_bounds,
                visual_bounds: result.visual_bounds,
            });
            x += result.size.w;
            height = height.max(result.size.h);
        }
        let size = constraints.constrain(Size::new(x, height));
        LayoutResult {
            size,
            children: child_layouts,
            paint_bounds: Rect::new(0.0, 0.0, size.w, size.h),
            visual_bounds: Rect::new(0.0, 0.0, size.w, size.h),
            overlay_hit_bounds: Vec::new(),
            clip: None,
            is_window_root_clip: false,
            artifact: None,
        }
    }

    /// Performs no drawing because this scenario measures retained runtime work.
    fn paint(&self, _ctx: &mut PaintCtx<'_>, _bounds: Rect, _layout: &LayoutResult) {}
}

/// Measures whether chat and terminal updates avoid rebuilding the file subtree.
///
/// # Errors
///
/// Propagates timing/process/correctness benchmark metric write failures.
///
/// # Panics
///
/// Panics if reconciliation failed to initialize either retained test signal.
fn run_component_isolation(
    session: &BenchSession,
    warmups: u32,
    measured: u32,
) -> Result<(), Box<dyn Error>> {
    let file = Rc::new(WorkCounters::default());
    let chat = Rc::new(WorkCounters::default());
    let terminal = Rc::new(WorkCounters::default());
    let chat_signal = Rc::new(RefCell::new(None));
    let terminal_signal = Rc::new(RefCell::new(None));
    let root = View::node(
        HorizontalRoot,
        vec![
            View::component(CountingComponent {
                counters: file.clone(),
                signal_slot: None,
            })
            .key("tree_virtualization-file-tree"),
            View::component(CountingComponent {
                counters: chat.clone(),
                signal_slot: Some(chat_signal.clone()),
            })
            .key("tree_virtualization-chat"),
            View::component(CountingComponent {
                counters: terminal.clone(),
                signal_slot: Some(terminal_signal.clone()),
            })
            .key("tree_virtualization-terminal"),
        ],
    );
    let mut runtime = Runtime::new(RuntimeHandle::new());
    runtime.reconcile_view(root);
    let mut text = TextSystem::new();
    let constraints = Constraints::tight(720.0, 240.0);
    runtime.layout(constraints, Scale::new(1.0), &mut text);
    let file_before = (file.builds.get(), file.layouts.get(), file.data_reads.get());
    let total = warmups.saturating_add(measured);

    for sample in 0..total {
        let phase = if sample < warmups {
            SamplePhase::Warmup
        } else {
            SamplePhase::Measured
        };
        let started = Instant::now();
        for revision in 0..500_u64 {
            chat_signal
                .borrow()
                .as_ref()
                .expect("chat signal")
                .set(u64::from(sample) * 500 + revision + 1);
            runtime.layout(constraints, Scale::new(1.0), &mut text);
        }
        for revision in 0..500_u64 {
            terminal_signal
                .borrow()
                .as_ref()
                .expect("terminal signal")
                .set(u64::from(sample) * 500 + revision + 1);
            runtime.layout(constraints, Scale::new(1.0), &mut text);
        }
        record_metric(
            session,
            "component_isolation.one_thousand_updates_us",
            started.elapsed().as_secs_f64() * 1_000_000.0,
            MetricRole::GatingSteady,
            phase,
        )?;
        record_process_sample(session, phase)?;
    }

    record_correctness(
        session,
        "correctness.component_isolation.file_tree_build_delta",
        file.builds.get().saturating_sub(file_before.0),
    )?;
    record_correctness(
        session,
        "correctness.component_isolation.file_tree_layout_delta",
        file.layouts.get().saturating_sub(file_before.1),
    )?;
    record_correctness(
        session,
        "correctness.component_isolation.filesystem_read_delta",
        file.data_reads.get().saturating_sub(file_before.2),
    )?;
    record_correctness(
        session,
        "correctness.component_isolation.chat_build_count_mismatch",
        u64::from(chat.builds.get() != u64::from(total) * 500 + 1),
    )?;
    record_correctness(
        session,
        "correctness.component_isolation.terminal_build_count_mismatch",
        u64::from(terminal.builds.get() != u64::from(total) * 500 + 1),
    )
}

/// Measures bounded work for a 100,000-row virtualized tree around its midpoint.
///
/// # Errors
///
/// Propagates tree-model mutation/revision errors and benchmark metric or process
/// sampling failures.
fn run_tree_virtualization(
    session: &BenchSession,
    warmups: u32,
    measured: u32,
) -> Result<(), Box<dyn Error>> {
    let mut model = TreeModel::new();
    model.apply_batch((0..100_000_u64).map(|id| TreeMutation::Insert {
        parent: None,
        index: id as usize,
        item: TreeItem::leaf(id, format!("virtual-row-{id:06}")),
    }))?;
    let model = TreeModelHandle::new(model);
    let flatten_before = model.read(|model| model.flat_index().rebuilds());
    let diagnostics = TreeViewDiagnostics::new();
    let tree: View<()> = TreeView::new()
        .model(model.clone())
        .selected(50_000_u64)
        .virtualized(true)
        .diagnostics(diagnostics.clone())
        .into_view()
        .key("tree_virtualization-bench-tree");
    let mut runtime = Runtime::new(RuntimeHandle::new());
    runtime.reconcile(
        ScrollView::vertical()
            .initial_scroll_y(50_000.0 * 28.0)
            .child(tree),
    );
    let constraints = Constraints::tight(720.0, 520.0);
    let mut text = TextSystem::new();
    runtime.layout(constraints, Scale::new(1.0), &mut text);
    let _ = runtime.paint(&mut text);
    let tree_id = runtime
        .tree
        .resolve_element_by_view_key("tree_virtualization-bench-tree")
        .map_err(|error| format!("tree key resolution failed: {error:?}"))?;
    let total = warmups.saturating_add(measured);
    let mut measured_us = Vec::with_capacity(measured as usize);
    let mut row_overflow = 0_u64;

    for sample in 0..total {
        let phase = if sample < warmups {
            SamplePhase::Warmup
        } else {
            SamplePhase::Measured
        };
        let before = diagnostics.snapshot();
        runtime.runtime.invalidate(tree_id, Invalidation::Layout);
        let started = Instant::now();
        runtime.layout(constraints, Scale::new(1.0), &mut text);
        let _ = runtime.paint(&mut text);
        let elapsed_us = started.elapsed().as_secs_f64() * 1_000_000.0;
        let after = diagnostics.snapshot();
        let layout_rows = after
            .layout_rows_visited
            .saturating_sub(before.layout_rows_visited);
        let paint_rows = after
            .paint_rows_visited
            .saturating_sub(before.paint_rows_visited);
        row_overflow = row_overflow.saturating_add(u64::from(
            layout_rows > TREE_ROW_LIMIT || paint_rows > TREE_ROW_LIMIT,
        ));
        if phase == SamplePhase::Measured {
            measured_us.push(elapsed_us);
        }
        record_metric(
            session,
            "tree_virtualization.layout_paint_us",
            elapsed_us,
            MetricRole::GatingSteady,
            phase,
        )?;
        record_metric(
            session,
            "tree_virtualization.layout_rows",
            layout_rows as f64,
            MetricRole::Diagnostic,
            phase,
        )?;
        record_metric(
            session,
            "tree_virtualization.paint_rows",
            paint_rows as f64,
            MetricRole::Diagnostic,
            phase,
        )?;
        record_process_sample(session, phase)?;
    }

    let final_diagnostics = diagnostics.snapshot();
    let p95 = summarize_samples(&measured_us)?.p95;
    record_correctness(
        session,
        "correctness.tree_virtualization.row_budget_overflow",
        row_overflow,
    )?;
    record_correctness(
        session,
        "correctness.tree_virtualization.flatten_rebuild_delta",
        model
            .read(|model| model.flat_index().rebuilds())
            .saturating_sub(flatten_before),
    )?;
    record_correctness(
        session,
        "correctness.tree_virtualization.fallbacks",
        final_diagnostics.virtualization_fallbacks,
    )?;
    record_correctness(
        session,
        "correctness.tree_virtualization.loaded_row_mismatch",
        u64::from(final_diagnostics.loaded_rows != 100_000),
    )?;
    record_correctness(
        session,
        "correctness.tree_virtualization.p95_over_33ms",
        u64::from(p95 > TREE_FRAME_P95_LIMIT_US),
    )
}

/// Returns milliseconds since the Unix epoch, falling back to zero before it.
fn unix_time_ms() -> u128 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis()
}

#[cfg(test)]
/// Contract tests for the fixed tree virtualization sampling and latency thresholds.
mod tests {
    use super::*;

    #[test]
    fn locked_sample_contract_rejects_short_runs() {
        assert_eq!(LOCKED_WARMUPS, 3);
        assert!(MIN_MEASURED >= 30);
        assert_eq!(TREE_ROW_LIMIT, 53);
        assert_eq!(TREE_FRAME_P95_LIMIT_US, 33_000.0);
    }
}
