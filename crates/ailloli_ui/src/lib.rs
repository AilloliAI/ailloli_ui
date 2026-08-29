//! Public façade for the Ailloli UI framework.
//!
//! This crate re-exports the workspace sub-crates so applications can depend on a
//! single package:
//!
//! | Module / alias | Crate | Role |
//! |----------------|-------|------|
//! | [`core`] | `ailloli_ui_core` | Geometry, colors, theme, events |
//! | [`runtime`] | `ailloli_ui_runtime` | Reconcile, layout, paint, reactivity |
//! | [`terminal_core`] | `ailloli_ui_terminal_core` | Pure terminal state/grid model |
//! | [`text`] | `ailloli_ui_text` | Text layout and buffers |
//! | [`widgets`] | `ailloli_ui_widgets` | Built-in widgets |
//!
//! ## Application entry point
//!
//! For desktop apps, use [`app::App`] and [`prelude`] — see [`App::new`] and
//! [`AppBuilder::run`] (requires the **`winit`** feature, enabled by default).
//!
//! Lower-level integration (custom event loop, raw `DrawCmd`) is available via
//! [`runtime`] and, with `winit`, [`ailloli_ui_winit`](https://docs.rs/ailloli_ui_winit).
//!
//! Product-specific integrations (chat providers, IDE chrome, remote workspaces,
//! desktop automation, etc.) deliberately live outside this public façade.

pub use ailloli_ui_core as core;
#[cfg(feature = "devtools")]
pub use ailloli_ui_devtools_core as devtools_core;
#[cfg(feature = "devtools")]
pub use ailloli_ui_devtools_ui as devtools_ui;
pub use ailloli_ui_fs as fs;
pub use ailloli_ui_runtime as runtime;
pub use ailloli_ui_terminal_core as terminal_core;
#[cfg(feature = "terminal_pty")]
pub use ailloli_ui_terminal_pty as terminal_pty;
pub use ailloli_ui_text as text;
pub use ailloli_ui_widgets as widgets;

/// High-level [`App`] / [`Window`] API and command routing.
pub mod app;
#[cfg(feature = "winit")]
pub mod capture;
/// Ergonomic imports for building views (`use ailloli_ui::prelude::*`).
pub mod prelude;

pub use ailloli_ui_runtime::app::{
    RuntimeInbox, RuntimeInboxStats, RuntimeSendError, RuntimeSender, UiWake, UiWakeError,
};
pub use app::{
    ActionSchema, App, AppBuilder, Command, Commands, Result, RuntimeInboxAttachError, Window,
    WindowChrome, Windows,
};

/// Embeds the conventional application icon from `src/assets/icons/icon.svg` in the
/// consuming crate.
///
/// The SVG bytes are embedded at compile time; the path is never read at
/// runtime. Compilation fails if the consuming package does not provide the
/// conventional file. Validation remains deferred until icon rasterization or
/// [`AppBuilder::run`].
///
/// # Examples
///
/// A consumer with `src/assets/icons/icon.svg` can invoke the disabled line
/// below directly. The doctest keeps it configuration-disabled because the
/// facade package is not itself an application and intentionally owns no icon.
///
/// ```
/// use ailloli_ui::{AppIcon, CONVENTIONAL_APP_ICON_PATH};
/// #[cfg(any())]
/// let icon: AppIcon = ailloli_ui::app_icon!();
/// let expected_type: Option<AppIcon> = None;
/// assert!(expected_type.is_none());
/// assert_eq!(CONVENTIONAL_APP_ICON_PATH, "src/assets/icons/icon.svg");
/// ```
#[macro_export]
macro_rules! app_icon {
    () => {
        $crate::AppIcon::from_static_svg(
            include_bytes!(concat!(
                env!("CARGO_MANIFEST_DIR"),
                "/src/assets/icons/icon.svg"
            )),
            $crate::CONVENTIONAL_APP_ICON_PATH,
        )
    };
}
pub use ailloli_ui_app_storage::{
    atomic_write_bytes as atomic_write_app_storage_bytes, read_optional_json,
    write_json_atomic as write_app_storage_json_atomic, AppId, AppPreferencesDocument, AppStorage,
    AppStorageBuilder, AppStorageDiagnostics, AppStorageDirs, AppStorageError, AppStorageMode,
    EnvOverride, HomeSymlinkEntryState, HomeSymlinkEntryStatus, HomeSymlinkView,
    HomeSymlinkViewState, HomeSymlinkViewStatus, LogicalWindowPosition, LogicalWindowSize,
    WindowSnapshot, WindowStateDocument, APP_PREFERENCES_VERSION, WINDOW_STATE_VERSION,
};
pub use ailloli_ui_core::{
    AppIcon, AppIconMetadata, AppIdentity, AppIdentityError, AppIdentityMetadata, ApplicationId,
    BoxShadow, ChartPoint, ChartRange, ChartSeries, ChatEvent, ChatItemId, ChatMessage,
    ChatMessageKind, ChatMessageStatus, ChatRequestId, ChatRole, ChatSessionId, ChatSessionState,
    ChatSessionStatus, ChatSessionSummary, Color, Constraints, DateValue, FontId, HsvColor, IconId,
    MonthValue, Offset, Rect, ScrollbarAxis, ScrollbarDrag, ScrollbarGeometry,
    ScrollbarGeometrySpec, ScrollbarPart, Size, SvgSource, TextDecoration, TextStyle, Theme,
    ThemePalette, ThemeRadius, ThemeShadows, ThemeSpacing, ThemeState, ThemeTypography, TimeFormat,
    TimeValue, UploadAccept, UploadFile, WeekStart, APP_IDENTITY_METADATA_VERSION,
    CONVENTIONAL_APP_ICON_PATH,
};
#[cfg(feature = "devtools")]
pub use ailloli_ui_devtools_core::{
    collect_debug_snapshot, collect_debug_snapshot_with_state, compute_warnings, debug_draw_cmds,
    pick_element_at, DebugDrawCmd, DebugSnapshot, DebugWarningKind, DevToolsMode,
};
#[cfg(feature = "devtools_terminal")]
pub use ailloli_ui_devtools_core::{terminal_debug_snapshot, TerminalDebugSnapshot};
#[cfg(feature = "devtools")]
pub use ailloli_ui_devtools_ui::{build_devtools_overlay, DevToolsAction, DevToolsState};
pub use ailloli_ui_fs::{
    FileCapabilities, FileEntry, FileError, FileKind, FileMetadata, FileOperation,
    FileOperationKind, FileProgress, FileProvider, FileTransfer, FileUri,
};
pub use ailloli_ui_runtime::component::{IntoView, IntoViewKeyExt, Memo, State, View, Widget};
pub use ailloli_ui_runtime::input::{ClickAction, DeferredAction, IntoClickAction};
pub use ailloli_ui_runtime::{
    DrawBoxShadow, DrawCmd, DrawImage, DrawRRect, DrawRect, DrawText, Invalidation, Layer,
    LayoutCtx, LayoutPass, PaintCtx, Painter, Scene,
};
#[cfg(feature = "terminal_pty_portable")]
pub use ailloli_ui_terminal_pty::PortablePtyBackend;
#[cfg(feature = "terminal_pty")]
pub use ailloli_ui_terminal_pty::{
    MockPtyBackend, PtyBackend, PtyBatchConfig, PtyError, PtyEvent, PtyExitStatus, PtyHandle,
    PtyOutputBatcher, PtySize, PtySpawnConfig,
};
pub use ailloli_ui_text::{ApproxTextMeasure, FontMetrics, TextBuffer, TextMeasure, WrapMode};
pub use ailloli_ui_widgets::controls::{
    terminal_key_bytes, terminal_key_bytes_with_modes, terminal_selection_text,
    ChatMessageRenderer, ChatWidget, ChatWidgetAction, ChatWidgetActionHandler,
    ChatWidgetCopyHandler, ChatWidgetRetryHandler, ChatWidgetSendHandler, ChatWidgetStyle,
    ContextMenu, ContextMenuEntry, ContextMenuItem, ContextMenuStyle, Link, LinkStyle,
    PopupAlignment, PopupPlacement, Terminal, TerminalSelectionMode, TerminalWidgetStyle, Tooltip,
    TooltipStyle, TreeItem, TreeModel, TreeModelDelta, TreeModelError, TreeModelHandle,
    TreeMutation, TreeViewDiagnostics, TreeViewDiagnosticsSnapshot,
};
pub use ailloli_ui_widgets::editor::{
    CodeEditorFeatureFlags, CodeFileSummary, CodeSymbol, Diagnostic, DiagnosticHit,
    DiagnosticSeverity, DiagnosticSource, DocumentSource, EditorPane, EditorPaneAction,
    EditorPaneSize, EditorPaneStyle, EditorPaneTab, EditorPaneTabKind, FoldRegion, FoldRegionId,
    LexicalRustSymbolIndexer, LspBackend, LspCapabilities, LspDiagnostic, LspEnrichment, LspError,
    LspRequestId, NoopLspBackend, ScipDocumentIndex, ScipImportError, ScipNavigationLink,
    ScipOccurrence, ScipOccurrenceRole, ScipProjectIndex, ScipProjectMetadata, ScipProjectSummary,
    ScipRelation, ScipSymbol, SearchMatch, SearchQuery, SearchState, SymbolEdge, SymbolEdgeKind,
    SymbolId, SymbolIndexer, SymbolKind, SymbolSource,
};
#[cfg(feature = "files")]
pub use ailloli_ui_widgets::files::{
    breadcrumb_segments, file_icon_for_entry, file_icon_for_name, file_icon_visual_for_entry,
    file_icon_visual_for_name, flatten_file_nodes, load_file_tree, sort_file_nodes, DirLoadState,
    FileBreadcrumb, FileBreadcrumbSegment, FileBreadcrumbStyle, FileExplorer, FileExplorerAction,
    FileExplorerCreateDir, FileExplorerNode, FileExplorerRename, FileExplorerRow, FileExplorerSize,
    FileExplorerStyle, FileIconVisual, FileTreeLoadMode, FileTreeNode, FileTreeNodeId,
    FileTreeOptions, FileTreeStore, LargeDirectoryPolicy,
};
#[cfg(feature = "files_local")]
pub use ailloli_ui_widgets::files::{
    local_file_tree_nodes, FileExplorerIoRequest, FileExplorerIoResponse, LocalFileExplorer,
    LocalFileExplorerCacheMode, LocalFileExplorerIoWorker, LocalFileExplorerLoadingMode,
};
pub use ailloli_ui_widgets::layout::{
    ResizeAxis, ResizeBar, ResizeBarStyle, ResizeDragPhase, SplitPane, SplitPaneStyle,
    SplitResizeEvent,
};
pub use ailloli_ui_widgets::primitives::Icon;
#[cfg(feature = "winit")]
pub use ailloli_ui_winit::{
    crop_captured_frame, strip_png_if_disabled, CaptureError, CaptureHandle, CaptureRequest,
    CaptureRequestId, CaptureResult, CaptureTarget, FrameCaptureHook, FrameCaptureResult,
};
#[cfg(feature = "native_overlay")]
pub use ailloli_ui_winit::{
    NativeCalibrationMarkerGuard, NativeCalibrationMarkerPixel, NativeCalibrationMarkerSpec,
    NativeOutputDescriptor, NativeOutputProbeService, NativeOutputScale, NativeOutputTransform,
    NativeOverlayBackend, NativeOverlayCapabilities, NativeOverlayError, NativeOverlayInputMode,
    NativeOverlayOptions, NativeOverlayRect, NativeOverlayTarget,
};
#[cfg(feature = "winit")]
pub use capture::{CaptureOpts, CaptureTargetSpec, CapturedArtifact};
/// Alias for [`Memo`] — derived reactive values.
///
/// # Examples
///
/// ```
/// use ailloli_ui::{Derived, Memo};
/// fn accepts_derived<T>(_: Derived<T>) {}
/// let memo: Memo<u32> = Memo::new(|| 42);
/// accepts_derived(memo);
/// ```
pub type Derived<T> = Memo<T>;
