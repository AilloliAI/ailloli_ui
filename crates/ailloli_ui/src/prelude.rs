//! Ergonomic prelude for building Ailloli UI views and apps.
//!
//! # Composing views
//!
//! Return [`View`] from UI functions (static or dynamic). Call `.into_view()` on the root builder
//! (see application `view/prelude`: re-export `ailloli_ui::IntoView` there — not in this crate prelude).
//! Containers still accept builders in `.child(...)` without converting each child;
//! [`Window::content`] converts the closure result once per frame.
//!
//! [`IntoView`] is a lower-level trait (`.child`, custom widgets) — not exported from this prelude.
//!
//! ```rust
//! use ailloli_ui::prelude::*;
//! use ailloli_ui::IntoView;
//!
//! fn header() -> View<()> {
//!     Row::new()
//!         .child(Text::new("Ailloli UI"))
//!         .into_view()
//! }
//!
//! fn content(enabled: bool) -> View<()> {
//!     if enabled {
//!         Editor::new(State::<TextBuffer>::new(TextBuffer::new())).into_view()
//!     } else {
//!         Text::new("Disabled").into_view()
//!     }
//! }
//! ```
//!
//! Application crates add a thin `view/prelude` (see `sample_app`): `ailloli_ui::prelude::*`,
//! `pub use ailloli_ui::IntoView`, alias `Action`, and `type View = View<Action>`.
//!
//! # Application entry
//!
//! ```ignore
//! use ailloli_ui::prelude::*;
//!
//! fn main() -> ailloli_ui::Result<()> {
//!     App::new()
//!         .window(
//!             Window::new("main")
//!                 .title("My app")
//!                 .content(|| Column::new().child(Text::new("Hello"))),
//!         )
//!         .run()
//! }
//! ```

pub use crate::app::{ActionSchema, App, Commands, Window, WindowChrome, Windows};
pub use crate::app_icon;
#[cfg(feature = "winit")]
pub use crate::capture::{CaptureOpts, CapturedArtifact};
pub use crate::core::style::{AlignItems, JustifyContent, Length};
pub use crate::core::style::{
    ThemePalette, ThemeRadius, ThemeShadows, ThemeSpacing, ThemeState, ThemeTypography,
};
pub use crate::core::{
    AppIcon, AppIconMetadata, AppIdentity, AppIdentityError, AppIdentityMetadata, ApplicationId,
    BoxShadow, ChartPoint, ChartRange, ChartSeries, ChatEvent, ChatItemId, ChatMessage,
    ChatMessageKind, ChatMessageStatus, ChatRequestId, ChatRole, ChatSessionId, ChatSessionState,
    ChatSessionStatus, ChatSessionSummary, Color, Constraints, DateValue, FontId, HsvColor, IconId,
    LineCap, LineJoin, MonthValue, Offset, ProgressSpec, Rect, ScrollAxes, ScrollBehavior,
    ScrollMetrics, ScrollState, Size, SliderRangeValue, SliderSpec, SliderThumb, StrokeStyle,
    SvgSource, Theme, TimeFormat, TimeValue, UploadAccept, UploadFile, WeekStart,
};
pub use crate::fs::{
    FileCapabilities, FileEntry, FileError, FileKind, FileMetadata, FileOperation,
    FileOperationKind, FileProgress, FileProvider, FileTransfer, FileUri,
};
pub use crate::{
    AppId, AppPreferencesDocument, AppStorage, AppStorageBuilder, AppStorageDiagnostics,
    AppStorageDirs, AppStorageError, AppStorageMode, HomeSymlinkView, HomeSymlinkViewState,
    HomeSymlinkViewStatus, LogicalWindowPosition, LogicalWindowSize, WindowSnapshot,
    WindowStateDocument,
};
#[cfg(feature = "native-overlay")]
pub use crate::{
    NativeOverlayInputMode, NativeOverlayOptions, NativeOverlayRect, NativeOverlayTarget,
};

pub use crate::runtime::component::{IntoView, IntoViewKeyExt, Memo, State, View};
pub use crate::widgets::layout::FlexItemExt;
#[cfg(feature = "winit")]
pub use crate::CaptureHandle;
pub use crate::Derived;
pub use crate::{ClickAction, DeferredAction, IntoClickAction};

pub use crate::text::{TextBuffer, WrapMode};
pub use crate::widgets::chrome::{
    WindowAffordanceDragPhase, WindowAffordanceEvent, WindowAffordanceFrame, WindowAffordanceKind,
    WindowAffordanceState, WindowAffordanceStyle,
};
pub use crate::widgets::controls::{
    terminal_key_bytes, terminal_key_bytes_with_modes, terminal_selection_text, Accordion,
    AccordionItem, AccordionMode, AccordionSize, AccordionStyle, Autocomplete, AutocompleteItem,
    AutocompleteSize, AutocompleteStyle, Avatar, AvatarStyle, AvatarTone, Badge, BadgeStyle,
    BadgeTone, BadgeVariant, BarChart, Button, ButtonStyle, ButtonVariant, Card, CardStyle,
    CardVariant, ChartSize, ChartStyle, ChartTone, ChatComposerControls, ChatComposerOption,
    ChatMessageRenderer, ChatWidget, ChatWidgetAction, ChatWidgetActionHandler,
    ChatWidgetCopyHandler, ChatWidgetRetryHandler, ChatWidgetSendHandler, ChatWidgetStyle, Chip,
    CircularProgress, ColorPicker, ColorPickerSize, ColorPickerStyle, ComboBox, ComboBoxOption,
    ComboBoxSize, ComboBoxStyle, CommandItem, CommandPalette, CommandPaletteSize,
    CommandPaletteStyle, DatePicker, DatePickerSize, DatePickerStyle, Dialog, DialogStyle,
    DialogTone, DisclosureRow, DisclosureRowStyle, DisclosureRowVariant, Divider,
    DividerOrientation, DividerStyle, DividerVariant, Dropdown, DropdownItem, DropdownSize,
    DropdownStyle, LineChart, ListItem, ListItemStyle, ListItemVariant, ListView, ListViewStyle,
    NavItem, NavItemStyle, NavItemVariant, PopupPlacement, ProgressBar, ProgressSize,
    ProgressStyle, ProgressVariant, RadialGauge, RadioButton, RadioDirection, RadioGroup,
    RadioOption, RadioSize, RadioStyle, RangeSlider, SegmentedControl, SegmentedOption,
    SegmentedSize, SegmentedStyle, Select, SelectOption, SelectSize, SelectStyle, Sidebar,
    SidebarStyle, Slider, SliderOrientation, SliderSize, SliderStyle, StatusIndicator, StatusStyle,
    StatusTone, StatusVariant, Switch, SwitchOrientation, SwitchSize, SwitchStyle, TableAlign,
    TableCell, TableCellKind, TableColumn, TableColumnWidth, TableRow, TableView, TableViewSize,
    TableViewStyle, Tag, Terminal, TerminalEvent, TerminalEventBuffer, TerminalEventKind,
    TerminalEventSource, TerminalLine, TerminalLineAttrs, TerminalLineKind, TerminalPosition,
    TerminalSearchMatch, TerminalSelection, TerminalSelectionMode, TerminalView, TerminalViewStyle,
    TerminalWidgetStyle, TextInput, TextInputStyle, TimePicker, TimePickerSize, TimePickerStyle,
    Toast, ToastHost, ToastPosition, ToastStyle, ToastTone, TreeCreate, TreeCreateKind,
    TreeCreateRequest, TreeDelete, TreeDropPosition, TreeMove, TreeNode, TreeNodeTrailingAction,
    TreeRename, TreeView, TreeViewSize, TreeViewStyle, UploadDropzone, UploadDropzoneStyle,
    UploadDropzoneVariant,
};
pub use crate::widgets::editor::{
    CodeEditor, CodeEditorConfig, CodeEditorFeatureFlags, CodeFileSummary, CodeSymbol, CodeTheme,
    Diagnostic, DiagnosticHit, DiagnosticSeverity, DiagnosticSource, Document, DocumentId,
    DocumentSource, DocumentVersion, Editor, EditorLanguage, EditorPane, EditorPaneAction,
    EditorPaneSize, EditorPaneStyle, EditorPaneTab, EditorPaneTabKind, EditorScrollbarConfig,
    EditorScrollbarStyle, EditorWrapMode, FoldRegion, FoldRegionId, GutterConfig, SearchMatch,
    SearchQuery, SearchState, SymbolEdge, SymbolEdgeKind, SymbolId, SymbolKind, SymbolSource,
};
#[cfg(feature = "files-local")]
pub use crate::widgets::files::{
    local_file_tree_nodes, FileExplorerIoRequest, FileExplorerIoResponse, LocalFileExplorer,
    LocalFileExplorerCacheMode, LocalFileExplorerIoWorker, LocalFileExplorerLoadingMode,
};
#[cfg(feature = "files")]
pub use crate::widgets::files::{
    DirLoadState, FileBreadcrumb, FileBreadcrumbSegment, FileBreadcrumbStyle, FileExplorer,
    FileExplorerAction, FileExplorerCreateDir, FileExplorerNode, FileExplorerRename,
    FileExplorerRow, FileExplorerSize, FileExplorerStyle, FileIconVisual, FileTreeLoadMode,
    FileTreeNode, FileTreeNodeId, FileTreeOptions, FileTreeStore, LargeDirectoryPolicy,
    SymlinkTraversalPolicy,
};
pub use crate::widgets::layout::{
    Column, Container, ResizeAxis, ResizeBar, ResizeBarStyle, ResizeDragPhase, Row, ScrollView,
    ScrollbarStyle, SplitPane, SplitPaneStyle, SplitResizeEvent,
};
pub use crate::widgets::primitives::Icon;
pub use crate::widgets::text::{RichText, Text, TextSpan};
#[cfg(feature = "devtools")]
pub use crate::{
    build_devtools_overlay, collect_debug_snapshot, debug_draw_cmds, pick_element_at, DebugDrawCmd,
    DebugSnapshot, DebugWarningKind, DevToolsAction, DevToolsMode, DevToolsState,
};
#[cfg(feature = "devtools-terminal")]
pub use crate::{terminal_debug_snapshot, TerminalDebugSnapshot};
