//! Framed text/code editor composition with tabs and active-file metadata.
//!
//! An [`EditorPane`] owns one editor content view and paints two chrome rows:
//! selectable tabs followed by a title, path, or file breadcrumb. Tabs and the
//! active tab can be supplied as static values or live signals.

use std::path::Path;
use std::rc::Rc;

use ailloli_ui_core::event::pointer::{MouseButton, PointerEvent};
use ailloli_ui_core::event::Event;
use ailloli_ui_core::style::{FlexItemStyle, LayoutSizeHint, LayoutStyle};
use ailloli_ui_core::{
    Border, ClipShape, Color, Constraints, FontId, IconId, Offset, Radius, Rect, Size, TextStyle,
    Theme,
};
use ailloli_ui_editor::{Document, DocumentSource};
use ailloli_ui_runtime::component::{
    Binding, ComponentNode, Context, IntoView, Signal, View, Widget,
};
use ailloli_ui_runtime::input::EventCtx;
use ailloli_ui_runtime::layout::{ChildLayout, LayoutChild, LayoutCtx, LayoutResult};
use ailloli_ui_runtime::scene::PaintCtx;
use ailloli_ui_runtime::{DrawBorder, DrawCmd, DrawImage, DrawRRect, DrawRect, DrawText};
use ailloli_ui_text::{TextBuffer, TextLayoutParams, TextSystem, WrapMode};

use crate::controls::tabs::{TabsItem, TabsStyle};
use crate::controls::{draw_tabs_bar_with_options, TabsBarOptions};
use crate::editor::{CodeEditor, Editor};
#[cfg(feature = "files")]
use crate::files::{breadcrumb_segments, FileBreadcrumbStyle};
use crate::layout::layout_ext::{apply_layout_size, finish_view_sized};
#[cfg(feature = "files")]
use ailloli_ui_fs::FileUri;

/// Shared retained callback for the pane's combined action stream.
type ActionHandler<A> = Rc<dyn Fn(&mut EventCtx<A>, EditorPaneAction)>;
/// Shared retained callback for one tab identifier.
type TabHandler<A> = Rc<dyn Fn(&mut EventCtx<A>, String)>;

/// Default intrinsic pane width in logical pixels before layout constraints.
const DEFAULT_PANE_WIDTH: f32 = 640.0;
/// Default intrinsic pane height in logical pixels before layout constraints.
const DEFAULT_PANE_HEIGHT: f32 = 420.0;

/// Metadata rendered for one editor-pane tab.
///
/// Empty identifiers are accepted but should be avoided because identifiers
/// drive selection and callbacks. `None` for `path`, `icon`, or `icon_tint`
/// means that the corresponding decoration or override is absent.
///
/// # Examples
///
/// ```
/// use ailloli_ui_widgets::editor::{EditorPaneTab, EditorPaneTabKind};
/// let tab = EditorPaneTab::code("main", "main.rs").path("src/main.rs");
/// assert_eq!(tab.id, "main");
/// assert_eq!(tab.kind, EditorPaneTabKind::Code);
/// assert_eq!(tab.path.as_deref(), Some("src/main.rs"));
/// ```
#[derive(Clone, Debug, PartialEq)]
pub struct EditorPaneTab {
    /// Stable identifier emitted by selection and close actions.
    pub id: String,
    /// Visible tab label and fallback active title.
    pub title: String,
    /// Optional display path or URI used by the header/breadcrumb.
    pub path: Option<String>,
    /// Whether to render the unsaved-change indicator.
    pub dirty: bool,
    /// Semantic tab category used for the scope strip.
    pub kind: EditorPaneTabKind,
    /// Optional leading icon.
    pub icon: Option<IconId>,
    /// Optional icon tint; `None` uses the pane's muted-path color.
    pub icon_tint: Option<Color>,
}

impl EditorPaneTab {
    /// Creates an undecorated, clean tab of [`EditorPaneTabKind::Other`].
    ///
    /// # Examples
    ///
    /// ```
    /// use ailloli_ui_widgets::editor::{EditorPaneTab, EditorPaneTabKind};
    /// let tab = EditorPaneTab::new("notes", "Notes");
    /// assert_eq!(tab.kind, EditorPaneTabKind::Other);
    /// assert!(!tab.dirty && tab.path.is_none() && tab.icon.is_none());
    /// ```
    pub fn new(id: impl Into<String>, title: impl Into<String>) -> Self {
        Self {
            id: id.into(),
            title: title.into(),
            path: None,
            dirty: false,
            kind: EditorPaneTabKind::Other,
            icon: None,
            icon_tint: None,
        }
    }

    /// Creates a tab categorized as plain text.
    ///
    /// # Examples
    ///
    /// ```
    /// use ailloli_ui_widgets::editor::{EditorPaneTab, EditorPaneTabKind};
    /// assert_eq!(EditorPaneTab::text("readme", "README").kind, EditorPaneTabKind::Text);
    /// ```
    pub fn text(id: impl Into<String>, title: impl Into<String>) -> Self {
        Self::new(id, title).kind(EditorPaneTabKind::Text)
    }

    /// Creates a tab categorized as source code.
    ///
    /// # Examples
    ///
    /// ```
    /// use ailloli_ui_widgets::editor::{EditorPaneTab, EditorPaneTabKind};
    /// assert_eq!(EditorPaneTab::code("lib", "lib.rs").kind, EditorPaneTabKind::Code);
    /// ```
    pub fn code(id: impl Into<String>, title: impl Into<String>) -> Self {
        Self::new(id, title).kind(EditorPaneTabKind::Code)
    }

    /// Sets the display path or URI used by the active header.
    ///
    /// Empty strings behave as absent while resolving the header. With the
    /// `files` feature, URI strings, slash paths, backslash paths, and
    /// `parent > child` labels are normalized for breadcrumb rendering.
    ///
    /// # Examples
    ///
    /// ```
    /// use ailloli_ui_widgets::editor::EditorPaneTab;
    /// let tab = EditorPaneTab::code("lib", "lib.rs").path("src/lib.rs");
    /// assert_eq!(tab.path.as_deref(), Some("src/lib.rs"));
    /// ```
    pub fn path(mut self, path: impl Into<String>) -> Self {
        self.path = Some(path.into());
        self
    }

    /// Marks whether the tab has unsaved changes.
    ///
    /// # Examples
    ///
    /// ```
    /// use ailloli_ui_widgets::editor::EditorPaneTab;
    /// assert!(EditorPaneTab::text("draft", "Draft").dirty(true).dirty);
    /// ```
    pub fn dirty(mut self, dirty: bool) -> Self {
        self.dirty = dirty;
        self
    }

    /// Replaces the semantic category used by the tab scope strip.
    ///
    /// # Examples
    ///
    /// ```
    /// use ailloli_ui_widgets::editor::{EditorPaneTab, EditorPaneTabKind};
    /// let tab = EditorPaneTab::new("log", "Log").kind(EditorPaneTabKind::Text);
    /// assert_eq!(tab.kind, EditorPaneTabKind::Text);
    /// ```
    pub fn kind(mut self, kind: EditorPaneTabKind) -> Self {
        self.kind = kind;
        self
    }

    /// Sets a leading tab icon.
    ///
    /// # Examples
    ///
    /// ```
    /// use ailloli_ui_core::IconId;
    /// use ailloli_ui_widgets::editor::EditorPaneTab;
    /// let tab = EditorPaneTab::new("new", "New").icon(IconId::Plus);
    /// assert_eq!(tab.icon, Some(IconId::Plus));
    /// ```
    pub fn icon(mut self, icon: IconId) -> Self {
        self.icon = Some(icon);
        self
    }

    /// Sets the leading icon tint.
    ///
    /// The tint is retained even when no icon is configured and becomes visible
    /// if an icon is added later.
    ///
    /// # Examples
    ///
    /// ```
    /// use ailloli_ui_core::Color;
    /// use ailloli_ui_widgets::editor::EditorPaneTab;
    /// let color = Color::rgba(255, 0, 0, 1.0);
    /// assert_eq!(EditorPaneTab::new("a", "A").icon_tint(color).icon_tint, Some(color));
    /// ```
    pub fn icon_tint(mut self, color: Color) -> Self {
        self.icon_tint = Some(color);
        self
    }
}

/// Semantic category used to decorate an editor tab.
///
/// # Examples
///
/// ```
/// use ailloli_ui_widgets::editor::EditorPaneTabKind;
/// assert_eq!(EditorPaneTabKind::default(), EditorPaneTabKind::Other);
/// ```
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum EditorPaneTabKind {
    /// Plain-text document.
    Text,
    /// Source-code document.
    Code,
    /// Unclassified content and the default.
    #[default]
    Other,
}

/// High-level user action emitted by editor-pane tab chrome.
///
/// The contained string is the [`EditorPaneTab::id`]. A click on the close
/// affordance emits only `CloseTab`; it does not first select the tab.
///
/// # Examples
///
/// ```
/// use ailloli_ui_widgets::editor::EditorPaneAction;
/// let action = EditorPaneAction::SelectTab("main".into());
/// assert_eq!(action, EditorPaneAction::SelectTab("main".into()));
/// ```
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum EditorPaneAction {
    /// The user selected the identified tab.
    SelectTab(String),
    /// The user requested closing the identified tab.
    CloseTab(String),
}

/// Preset height for the tabs and metadata header rows.
///
/// # Examples
///
/// ```
/// use ailloli_ui_widgets::editor::EditorPaneSize;
/// assert_eq!(EditorPaneSize::default(), EditorPaneSize::Default);
/// ```
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum EditorPaneSize {
    /// 32-logical-pixel tabs plus a 24-pixel header.
    Compact,
    /// 36-logical-pixel tabs plus a 28-pixel header.
    #[default]
    Default,
}

/// Colors and logical-pixel geometry for an [`EditorPane`].
///
/// No field is automatically clamped. Negative row heights or radius values
/// are therefore forwarded to layout/painting and should be avoided.
///
/// # Examples
///
/// ```
/// use ailloli_ui_widgets::editor::EditorPaneStyle;
/// let style = EditorPaneStyle::default();
/// assert_eq!(style.tabs_height, 36.0);
/// assert_eq!(style.header_height, 28.0);
/// ```
#[derive(Clone, Debug)]
pub struct EditorPaneStyle {
    /// Content-frame fill color.
    pub background: Color,
    /// One-logical-pixel outer border color.
    pub border: Color,
    /// Metadata-header fill color.
    pub header_bg: Color,
    /// One-logical-pixel divider color below the header.
    pub header_border: Color,
    /// Active breadcrumb-segment color.
    pub title_fg: Color,
    /// Inactive breadcrumb/path and fallback icon color.
    pub path_fg: Color,
    /// Unsaved-change dot color.
    pub dirty: Color,
    /// Outer clip and border radius in logical pixels.
    pub radius: f32,
    /// Tab-row height in logical pixels.
    pub tabs_height: f32,
    /// Metadata-header height in logical pixels.
    pub header_height: f32,
    /// Nested tab-bar colors.
    pub tabs: TabsStyle,
}

/// Builds default-sized chrome from the process-independent default theme.
impl Default for EditorPaneStyle {
    fn default() -> Self {
        Self::from_theme(Theme::default(), EditorPaneSize::Default)
    }
}

impl EditorPaneStyle {
    /// Derives pane colors from `theme` and row heights from `size`.
    ///
    /// Compact rows are 32/24 logical pixels; default rows are 36/28. The
    /// dirty indicator uses opaque amber and the radius uses `theme.radius().md`.
    ///
    /// # Examples
    ///
    /// ```
    /// use ailloli_ui_core::Theme;
    /// use ailloli_ui_widgets::editor::{EditorPaneSize, EditorPaneStyle};
    /// let compact = EditorPaneStyle::from_theme(Theme::default(), EditorPaneSize::Compact);
    /// assert_eq!((compact.tabs_height, compact.header_height), (32.0, 24.0));
    /// ```
    pub fn from_theme(theme: Theme, size: EditorPaneSize) -> Self {
        let p = theme.palette();
        let (tabs_height, header_height) = match size {
            EditorPaneSize::Compact => (32.0, 24.0),
            EditorPaneSize::Default => (36.0, 28.0),
        };
        Self {
            background: p.surface,
            border: p.border,
            header_bg: p.surface_elevated.with_alpha(0.84),
            header_border: p.border.with_alpha(0.72),
            title_fg: p.text,
            path_fg: p.text_muted,
            dirty: Color::rgba(245, 158, 11, 1.0),
            radius: theme.radius().md,
            tabs_height,
            header_height,
            tabs: TabsStyle {
                bar_bg: p.background,
                tab_bg: p.surface,
                tab_bg_selected: p.surface_elevated,
                tab_border: p.border.with_alpha(0.72),
                tab_border_selected: p.accent,
                text_fg: p.text,
                text_muted: p.text_muted,
                unread_dot: Color::rgba(245, 158, 11, 1.0),
            },
        }
    }
}

/// Framed editor view with tab and active-document chrome.
///
/// The pane has a 640×420 logical-pixel intrinsic size, then obeys the standard
/// layout builders and incoming constraints. Its tabs and metadata header take
/// their configured heights; a smaller constrained height gives the content a
/// zero-height remainder. `A` is the application action type dispatched by
/// callback adapters.
///
/// # Examples
///
/// ```
/// use ailloli_ui_widgets::editor::{EditorPane, EditorPaneTab};
/// use ailloli_ui_widgets::text::Text;
/// let pane = EditorPane::<()>::new(Text::new("Preview"))
///     .tabs([EditorPaneTab::text("preview", "Preview")])
///     .active_tab("preview");
/// let _ = pane;
/// ```
pub struct EditorPane<A = ()> {
    /// Standard logical-pixel size and position constraints.
    pub(crate) layout: LayoutStyle,
    /// Standard flex-parent participation settings.
    pub(crate) flex_item: FlexItemStyle,
    content: View<A>,
    tabs: Vec<EditorPaneTab>,
    bound_tabs: Option<Signal<Vec<EditorPaneTab>>>,
    active_tab: Option<Binding<String>>,
    bound_active_tab: Option<Signal<String>>,
    active_title: Option<Binding<String>>,
    active_path: Option<Binding<String>>,
    dirty: Option<Binding<bool>>,
    active_document: Option<Signal<Document>>,
    style: EditorPaneStyle,
    on_select_tab: Option<TabHandler<A>>,
    on_close_tab: Option<TabHandler<A>>,
    on_action: Option<ActionHandler<A>>,
}

crate::impl_layout_builders!(EditorPane);

impl<A: 'static> EditorPane<A> {
    /// Wraps any view as pane content with empty tabs and default chrome.
    ///
    /// # Examples
    ///
    /// ```
    /// use ailloli_ui_widgets::editor::EditorPane;
    /// use ailloli_ui_widgets::text::Text;
    /// let pane = EditorPane::<()>::new(Text::new("content"));
    /// let _ = pane;
    /// ```
    pub fn new(child: impl IntoView<A>) -> Self {
        Self {
            layout: LayoutStyle::default(),
            flex_item: FlexItemStyle::default(),
            content: child.into_view(),
            tabs: Vec::new(),
            bound_tabs: None,
            active_tab: None,
            bound_active_tab: None,
            active_title: None,
            active_path: None,
            dirty: None,
            active_document: None,
            style: EditorPaneStyle::default(),
            on_select_tab: None,
            on_close_tab: None,
            on_action: None,
        }
    }

    /// Creates a pane whose content is a fill-sized plain-text [`Editor`].
    ///
    /// Edits mutate the shared buffer signal. No tab is synthesized from the
    /// buffer; configure tabs/title/path explicitly when chrome text is wanted.
    ///
    /// # Examples
    ///
    /// ```
    /// use ailloli_ui_runtime::component::State;
    /// use ailloli_ui_text::TextBuffer;
    /// use ailloli_ui_widgets::editor::EditorPane;
    /// let pane: EditorPane<()> = EditorPane::text(State::new(TextBuffer::from_string("notes")));
    /// let _ = pane;
    /// ```
    pub fn text(buffer: impl Into<Signal<TextBuffer>>) -> Self {
        Self::new(Editor::new(buffer).fill())
    }

    /// Creates a pane whose content is a fill-sized [`CodeEditor`].
    ///
    /// Document source metadata and dirty state become fallbacks for the pane
    /// header. No visible tab is synthesized automatically.
    ///
    /// # Examples
    ///
    /// ```
    /// use ailloli_ui_editor::{Document, DocumentId};
    /// use ailloli_ui_runtime::component::State;
    /// use ailloli_ui_text::TextBuffer;
    /// use ailloli_ui_widgets::editor::EditorPane;
    /// let document = Document::new(DocumentId(1), TextBuffer::new()).with_path("src/lib.rs");
    /// let pane: EditorPane<()> = EditorPane::code(State::new(document));
    /// let _ = pane;
    /// ```
    pub fn code(document: impl Into<Signal<Document>>) -> Self {
        let document = document.into();
        Self::new(CodeEditor::<A>::new(document.clone()).fill()).with_active_document(document)
    }

    /// Replaces static tabs and removes any previously bound tab signal.
    ///
    /// Iteration order is paint and hit-test order. An empty list suppresses
    /// tabs; no implicit tab is derived from editor content.
    ///
    /// # Examples
    ///
    /// ```
    /// use ailloli_ui_widgets::editor::{EditorPane, EditorPaneTab};
    /// use ailloli_ui_widgets::text::Text;
    /// let pane = EditorPane::<()>::new(Text::new("body"))
    ///     .tabs([EditorPaneTab::text("one", "One"), EditorPaneTab::text("two", "Two")]);
    /// let _ = pane;
    /// ```
    pub fn tabs(mut self, tabs: impl IntoIterator<Item = EditorPaneTab>) -> Self {
        self.tabs = tabs.into_iter().collect();
        self.bound_tabs = None;
        self
    }

    /// Binds the visible tabs to live shared state.
    ///
    /// Bound tabs take precedence over stored static tabs. Later calling
    /// [`Self::tabs`] returns to static input by clearing this binding.
    ///
    /// # Examples
    ///
    /// ```
    /// use ailloli_ui_runtime::component::State;
    /// use ailloli_ui_widgets::editor::{EditorPane, EditorPaneTab};
    /// use ailloli_ui_widgets::text::Text;
    /// let tabs = State::new(vec![EditorPaneTab::code("lib", "lib.rs")]);
    /// let pane = EditorPane::<()>::new(Text::new("body")).bind_tabs(tabs);
    /// let _ = pane;
    /// ```
    pub fn bind_tabs(mut self, tabs: impl Into<Signal<Vec<EditorPaneTab>>>) -> Self {
        self.bound_tabs = Some(tabs.into());
        self
    }

    /// Sets a static or generic binding for the active tab identifier.
    ///
    /// Empty or unknown identifiers fall back to the first tab. This method
    /// removes the special writable signal installed by [`Self::bind_active_tab`],
    /// so user clicks no longer update active state automatically.
    ///
    /// # Examples
    ///
    /// ```
    /// use ailloli_ui_widgets::editor::EditorPane;
    /// use ailloli_ui_widgets::text::Text;
    /// let pane = EditorPane::<()>::new(Text::new("body")).active_tab("main");
    /// let _ = pane;
    /// ```
    pub fn active_tab(mut self, active_tab: impl Into<Binding<String>>) -> Self {
        self.active_tab = Some(active_tab.into());
        self.bound_active_tab = None;
        self
    }

    /// Binds active-tab selection to writable shared state.
    ///
    /// A left click on a tab writes its identifier before invoking selection
    /// and aggregate callbacks. Empty or unknown values display the first tab.
    ///
    /// # Examples
    ///
    /// ```
    /// use ailloli_ui_runtime::component::State;
    /// use ailloli_ui_widgets::editor::EditorPane;
    /// use ailloli_ui_widgets::text::Text;
    /// let active = State::new(String::from("main"));
    /// let pane = EditorPane::<()>::new(Text::new("body")).bind_active_tab(active);
    /// let _ = pane;
    /// ```
    pub fn bind_active_tab(mut self, active_tab: impl Into<Signal<String>>) -> Self {
        let signal = active_tab.into();
        self.active_tab = Some(Binding::Signal(signal.clone()));
        self.bound_active_tab = Some(signal);
        self
    }

    /// Overrides the selected tab or document title.
    ///
    /// An empty value is treated as no override. Resolution order is explicit
    /// non-empty title, selected-tab title, document filename, then empty.
    ///
    /// # Examples
    ///
    /// ```
    /// use ailloli_ui_widgets::editor::EditorPane;
    /// use ailloli_ui_widgets::text::Text;
    /// let pane = EditorPane::<()>::new(Text::new("body")).active_title("Untitled");
    /// let _ = pane;
    /// ```
    pub fn active_title(mut self, title: impl Into<Binding<String>>) -> Self {
        self.active_title = Some(title.into());
        self
    }

    /// Overrides the selected tab or document path in the header.
    ///
    /// An empty value is treated as absent. Resolution order is explicit path,
    /// selected-tab path, then document source path/URI.
    ///
    /// # Examples
    ///
    /// ```
    /// use ailloli_ui_widgets::editor::EditorPane;
    /// use ailloli_ui_widgets::text::Text;
    /// let pane = EditorPane::<()>::new(Text::new("body")).active_path("src/main.rs");
    /// let _ = pane;
    /// ```
    pub fn active_path(mut self, path: impl Into<Binding<String>>) -> Self {
        self.active_path = Some(path.into());
        self
    }

    /// Overrides active dirty state, including an explicit `false`.
    ///
    /// Without an override, the selected tab and active document are combined
    /// with logical OR. The value controls the header dot and selected tab's
    /// unread/dirty decoration only.
    ///
    /// # Examples
    ///
    /// ```
    /// use ailloli_ui_widgets::editor::EditorPane;
    /// use ailloli_ui_widgets::text::Text;
    /// let pane = EditorPane::<()>::new(Text::new("body")).dirty(true);
    /// let _ = pane;
    /// ```
    pub fn dirty(mut self, dirty: impl Into<Binding<bool>>) -> Self {
        self.dirty = Some(dirty.into());
        self
    }

    /// Replaces all pane and tab chrome styling.
    ///
    /// # Examples
    ///
    /// ```
    /// use ailloli_ui_widgets::editor::{EditorPane, EditorPaneStyle};
    /// use ailloli_ui_widgets::text::Text;
    /// let pane = EditorPane::<()>::new(Text::new("body")).pane_style(EditorPaneStyle::default());
    /// let _ = pane;
    /// ```
    pub fn pane_style(mut self, style: EditorPaneStyle) -> Self {
        self.style = style;
        self
    }

    /// Applies a size preset using the default theme.
    ///
    /// This replaces the entire existing [`EditorPaneStyle`], so call it before
    /// [`Self::pane_style`] when combining a size preset with custom colors.
    ///
    /// # Examples
    ///
    /// ```
    /// use ailloli_ui_widgets::editor::{EditorPane, EditorPaneSize};
    /// use ailloli_ui_widgets::text::Text;
    /// let pane = EditorPane::<()>::new(Text::new("body")).pane_size(EditorPaneSize::Compact);
    /// let _ = pane;
    /// ```
    pub fn pane_size(mut self, size: EditorPaneSize) -> Self {
        self.style = EditorPaneStyle::from_theme(Theme::default(), size);
        self
    }

    /// Maps a selected tab identifier into an application action.
    ///
    /// The callback runs after a bound active-tab signal is updated and before
    /// the optional aggregate action callback.
    ///
    /// # Examples
    ///
    /// ```
    /// use ailloli_ui_widgets::editor::EditorPane;
    /// use ailloli_ui_widgets::text::Text;
    /// enum Action { Select(String) }
    /// let pane = EditorPane::<Action>::new(Text::new("body")).on_select_tab(Action::Select);
    /// let _ = pane;
    /// ```
    pub fn on_select_tab(mut self, f: impl Fn(String) -> A + 'static) -> Self {
        self.on_select_tab = Some(Rc::new(move |ctx, id| ctx.dispatch(f(id))));
        self
    }

    /// Handles selection with direct access to the event context.
    ///
    /// Use this form to dispatch zero or multiple actions or request additional
    /// runtime work. Callback order matches [`Self::on_select_tab`].
    ///
    /// # Examples
    ///
    /// ```
    /// use ailloli_ui_widgets::editor::EditorPane;
    /// use ailloli_ui_widgets::text::Text;
    /// let pane = EditorPane::<()>::new(Text::new("body"))
    ///     .on_select_tab_ctx(|_ctx, _id| {});
    /// let _ = pane;
    /// ```
    pub fn on_select_tab_ctx(mut self, f: impl Fn(&mut EventCtx<A>, String) + 'static) -> Self {
        self.on_select_tab = Some(Rc::new(f));
        self
    }

    /// Maps a close request identifier into an application action.
    ///
    /// Clicking a tab's trailing 22-logical-pixel close region takes priority
    /// over selection, invokes this callback, then invokes the aggregate action
    /// callback. The pane does not remove the tab itself.
    ///
    /// # Examples
    ///
    /// ```
    /// use ailloli_ui_widgets::editor::EditorPane;
    /// use ailloli_ui_widgets::text::Text;
    /// enum Action { Close(String) }
    /// let pane = EditorPane::<Action>::new(Text::new("body")).on_close_tab(Action::Close);
    /// let _ = pane;
    /// ```
    pub fn on_close_tab(mut self, f: impl Fn(String) -> A + 'static) -> Self {
        self.on_close_tab = Some(Rc::new(move |ctx, id| ctx.dispatch(f(id))));
        self
    }

    /// Handles a close request with direct access to the event context.
    ///
    /// # Examples
    ///
    /// ```
    /// use ailloli_ui_widgets::editor::EditorPane;
    /// use ailloli_ui_widgets::text::Text;
    /// let pane = EditorPane::<()>::new(Text::new("body"))
    ///     .on_close_tab_ctx(|_ctx, _id| {});
    /// let _ = pane;
    /// ```
    pub fn on_close_tab_ctx(mut self, f: impl Fn(&mut EventCtx<A>, String) + 'static) -> Self {
        self.on_close_tab = Some(Rc::new(f));
        self
    }

    /// Maps every select/close event into one application action.
    ///
    /// A specialized selection or close callback, when installed, runs first;
    /// both callbacks may therefore dispatch actions for one pointer event.
    ///
    /// # Examples
    ///
    /// ```
    /// use ailloli_ui_widgets::editor::{EditorPane, EditorPaneAction};
    /// use ailloli_ui_widgets::text::Text;
    /// let pane = EditorPane::<EditorPaneAction>::new(Text::new("body")).on_action(|action| action);
    /// let _ = pane;
    /// ```
    pub fn on_action(mut self, f: impl Fn(EditorPaneAction) -> A + 'static) -> Self {
        self.on_action = Some(Rc::new(move |ctx, action| ctx.dispatch(f(action))));
        self
    }

    /// Handles every select/close event with direct event-context access.
    ///
    /// # Examples
    ///
    /// ```
    /// use ailloli_ui_widgets::editor::EditorPane;
    /// use ailloli_ui_widgets::text::Text;
    /// let pane = EditorPane::<()>::new(Text::new("body"))
    ///     .on_action_ctx(|_ctx, _action| {});
    /// let _ = pane;
    /// ```
    pub fn on_action_ctx(
        mut self,
        f: impl Fn(&mut EventCtx<A>, EditorPaneAction) + 'static,
    ) -> Self {
        self.on_action = Some(Rc::new(f));
        self
    }

    /// Retains code-document metadata as a title/path/dirty fallback.
    fn with_active_document(mut self, document: Signal<Document>) -> Self {
        self.active_document = Some(document);
        self
    }
}

/// Converts the pane builder into a clipped frame, chrome, and content tree.
impl<A: 'static> IntoView<A> for EditorPane<A> {
    fn into_view(self) -> View<A> {
        finish_view_sized(
            View::component(EditorPaneComponent {
                layout: self.layout,
                content: self.content,
                tabs: self.tabs,
                bound_tabs: self.bound_tabs,
                active_tab: self.active_tab,
                bound_active_tab: self.bound_active_tab,
                active_title: self.active_title,
                active_path: self.active_path,
                dirty: self.dirty,
                active_document: self.active_document,
                style: self.style,
                on_select_tab: self.on_select_tab,
                on_close_tab: self.on_close_tab,
                on_action: self.on_action,
            }),
            self.flex_item,
            LayoutSizeHint::from_layout(self.layout),
        )
    }
}

/// Component boundary that expands the builder into frame/chrome/content nodes.
struct EditorPaneComponent<A> {
    layout: LayoutStyle,
    content: View<A>,
    tabs: Vec<EditorPaneTab>,
    bound_tabs: Option<Signal<Vec<EditorPaneTab>>>,
    active_tab: Option<Binding<String>>,
    bound_active_tab: Option<Signal<String>>,
    active_title: Option<Binding<String>>,
    active_path: Option<Binding<String>>,
    dirty: Option<Binding<bool>>,
    active_document: Option<Signal<Document>>,
    style: EditorPaneStyle,
    on_select_tab: Option<TabHandler<A>>,
    on_close_tab: Option<TabHandler<A>>,
    on_action: Option<ActionHandler<A>>,
}

/// Rebuilds chrome children from the latest builder-held bindings.
impl<A: 'static> ComponentNode<A> for EditorPaneComponent<A> {
    fn build(&self, _context: &mut Context<A>) -> View<A> {
        let chrome = EditorPaneChromeWidget {
            tabs: self.tabs.clone(),
            bound_tabs: self.bound_tabs.clone(),
            active_tab: self.active_tab.clone(),
            bound_active_tab: self.bound_active_tab.clone(),
            active_title: self.active_title.clone(),
            active_path: self.active_path.clone(),
            dirty: self.dirty.clone(),
            active_document: self.active_document.clone(),
            style: self.style.clone(),
            on_select_tab: self.on_select_tab.clone(),
            on_close_tab: self.on_close_tab.clone(),
            on_action: self.on_action.clone(),
        };
        let chrome_children = editor_pane_breadcrumb_children(
            self.tabs.clone(),
            self.bound_tabs.clone(),
            self.active_tab.clone(),
            self.active_path.clone(),
            self.active_document.clone(),
            self.style.clone(),
        );

        View::node(
            EditorPaneFrameWidget {
                layout: self.layout,
                style: self.style.clone(),
            },
            vec![View::node(chrome, chrome_children), self.content.clone()],
        )
    }
}

/// Lays out and clips the two-row chrome above the editor content.
struct EditorPaneFrameWidget {
    layout: LayoutStyle,
    style: EditorPaneStyle,
}

/// Implements constrained 640x420 sizing, round clipping, fill, and border.
impl<A: 'static> Widget<A> for EditorPaneFrameWidget {
    fn debug_name(&self) -> &'static str {
        "EditorPane"
    }

    fn layout(
        &self,
        engine: &mut ailloli_ui_runtime::layout::LayoutEngine<'_, A>,
        ctx: &mut LayoutCtx<'_>,
        children: &mut [LayoutChild],
        constraints: Constraints,
    ) -> LayoutResult {
        let intrinsic = Size::new(DEFAULT_PANE_WIDTH, DEFAULT_PANE_HEIGHT);
        let size = apply_layout_size(intrinsic, self.layout, constraints);
        let chrome_h = self.style.tabs_height + self.style.header_height;
        let content_h = (size.h - chrome_h).max(0.0);

        let mut child_layouts = Vec::with_capacity(children.len());
        if let Some(chrome) = children.get_mut(0) {
            let result = chrome.layout(engine, ctx, Constraints::tight(size.w, chrome_h));
            child_layouts.push(ChildLayout {
                offset: Offset::new(0.0, 0.0),
                size: result.size,
                paint_bounds: Rect::new(0.0, 0.0, result.size.w, result.size.h),
                visual_bounds: result.visual_bounds,
            });
        }
        if let Some(content) = children.get_mut(1) {
            let result = content.layout(engine, ctx, Constraints::tight(size.w, content_h));
            child_layouts.push(ChildLayout {
                offset: Offset::new(0.0, chrome_h),
                size: result.size,
                paint_bounds: Rect::new(0.0, chrome_h, result.size.w, result.size.h),
                visual_bounds: result.visual_bounds.translate(Offset::new(0.0, chrome_h)),
            });
        }

        let bounds = Rect::new(0.0, 0.0, size.w, size.h);
        LayoutResult {
            size,
            children: child_layouts,
            paint_bounds: bounds,
            visual_bounds: bounds,
            overlay_hit_bounds: Vec::new(),
            clip: Some(ClipShape::RoundRect {
                rect: bounds,
                radius: self.style.radius,
            }),
            is_window_root_clip: false,
            artifact: None,
        }
    }

    fn paint(&self, ctx: &mut PaintCtx<'_>, bounds: Rect, _layout: &LayoutResult) {
        ctx.push(DrawCmd::RRect(DrawRRect {
            rect: bounds,
            radius: self.style.radius,
            color: self.style.background,
        }));
    }

    fn paint_overlay(&self, ctx: &mut PaintCtx<'_>, bounds: Rect, _layout: &LayoutResult) {
        ctx.push(DrawCmd::Border(DrawBorder {
            rect: bounds,
            radius: Radius::uniform(self.style.radius),
            border: Border::new(1.0, self.style.border),
        }));
    }

    fn event(&self, _ctx: &mut EventCtx<A>, _event: &Event, _bounds: Rect, _layout: &LayoutResult) {
    }
}

/// Resolves reactive tab metadata and paints/handles the two chrome rows.
struct EditorPaneChromeWidget<A> {
    tabs: Vec<EditorPaneTab>,
    bound_tabs: Option<Signal<Vec<EditorPaneTab>>>,
    active_tab: Option<Binding<String>>,
    bound_active_tab: Option<Signal<String>>,
    active_title: Option<Binding<String>>,
    active_path: Option<Binding<String>>,
    dirty: Option<Binding<bool>>,
    active_document: Option<Signal<Document>>,
    style: EditorPaneStyle,
    on_select_tab: Option<TabHandler<A>>,
    on_close_tab: Option<TabHandler<A>>,
    on_action: Option<ActionHandler<A>>,
}

/// Paints closeable tabs and routes left-clicks with close-before-row priority.
impl<A: 'static> Widget<A> for EditorPaneChromeWidget<A> {
    fn debug_name(&self) -> &'static str {
        "EditorPaneChrome"
    }

    fn layout(
        &self,
        engine: &mut ailloli_ui_runtime::layout::LayoutEngine<'_, A>,
        ctx: &mut LayoutCtx<'_>,
        children: &mut [LayoutChild],
        constraints: Constraints,
    ) -> LayoutResult {
        let size = constraints.constrain(Size::new(
            constraints.max_w,
            self.style.tabs_height + self.style.header_height,
        ));
        let mut child_layouts = Vec::new();
        if let Some(child) = children.get_mut(0) {
            let model = self.model();
            let header_text_x = self.header_text_x(size.w, model.active_icon.is_some());
            let right_pad = if model.active_dirty { 30.0 } else { 12.0 };
            let child_w = (size.w - header_text_x - right_pad).max(0.0);
            let result = child.layout(
                engine,
                ctx,
                Constraints::tight(child_w, self.style.header_height),
            );
            let offset = Offset::new(header_text_x, self.style.tabs_height);
            child_layouts.push(ChildLayout {
                offset,
                size: result.size,
                paint_bounds: Rect::new(offset.x, offset.y, result.size.w, result.size.h),
                visual_bounds: result.visual_bounds.translate(offset),
            });
        }
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

    fn paint(&self, ctx: &mut PaintCtx<'_>, bounds: Rect, _layout: &LayoutResult) {
        let model = self.model();
        let tab_rect = Rect::new(bounds.x, bounds.y, bounds.w, self.style.tabs_height);
        if let Some(text) = ctx.text_system.as_deref_mut() {
            let (cmds, _) = draw_tabs_bar_with_options(
                tab_rect,
                &model.tabs,
                false,
                self.style.tabs,
                text,
                TabsBarOptions {
                    show_trailing_actions: false,
                    show_tab_close_affordance: true,
                    show_scope_strip: true,
                },
            );
            for cmd in cmds {
                ctx.push(cmd);
            }
        } else {
            ctx.push(DrawCmd::Rect(DrawRect {
                rect: tab_rect,
                color: self.style.tabs.bar_bg,
            }));
        }

        let header = Rect::new(
            bounds.x,
            bounds.y + self.style.tabs_height,
            bounds.w,
            self.style.header_height,
        );
        ctx.push(DrawCmd::Rect(DrawRect {
            rect: header,
            color: self.style.header_bg,
        }));
        ctx.push(DrawCmd::Rect(DrawRect {
            rect: Rect::new(header.x, header.bottom() - 1.0, header.w, 1.0),
            color: self.style.header_border,
        }));

        let mut text_x = header.x + 14.0;
        if let Some(icon) = &model.active_icon {
            let size = 14.0;
            ctx.push(DrawCmd::Image(DrawImage {
                rect: Rect::new(
                    header.x + 12.0,
                    header.y + (header.h - size) * 0.5,
                    size,
                    size,
                ),
                icon: icon.clone(),
                tint: model.active_icon_tint.unwrap_or(self.style.path_fg),
                rotation_rad: 0.0,
            }));
            text_x += 20.0;
        }

        let header_cmd = if !model_has_breadcrumb(&model) {
            ctx.text_system.as_deref_mut().and_then(|text| {
                let header_text = model
                    .active_path
                    .as_deref()
                    .filter(|path| !path.is_empty())
                    .unwrap_or(&model.active_title);
                (!header_text.is_empty()).then(|| {
                    label_cmd(
                        text,
                        [text_x, header.y + 18.0],
                        self.style.path_fg,
                        12,
                        header_text,
                    )
                })
            })
        } else {
            None
        };
        if let Some(cmd) = header_cmd {
            ctx.push(cmd);
        }

        if model.active_dirty {
            ctx.push(DrawCmd::RRect(DrawRRect {
                rect: Rect::new(
                    header.right() - 18.0,
                    header.y + (header.h - 8.0) * 0.5,
                    8.0,
                    8.0,
                ),
                radius: 4.0,
                color: self.style.dirty,
            }));
        }
    }

    fn event(&self, ctx: &mut EventCtx<A>, event: &Event, bounds: Rect, _layout: &LayoutResult) {
        let Event::Pointer(PointerEvent::Button {
            pos,
            button: MouseButton::Left,
            pressed: true,
            ..
        }) = event
        else {
            return;
        };
        if !bounds.contains(pos.x, pos.y) {
            return;
        }

        let model = self.model();
        let tab_rect = Rect::new(bounds.x, bounds.y, bounds.w, self.style.tabs_height);
        for (id, row, close) in tab_hit_layout(tab_rect, &model.tabs) {
            if close.contains(pos.x, pos.y) {
                self.emit_close(ctx, id);
                ctx.stop_propagation();
                return;
            }
            if row.contains(pos.x, pos.y) {
                self.emit_select(ctx, id);
                ctx.stop_propagation();
                return;
            }
        }
    }
}

impl<A: 'static> EditorPaneChromeWidget<A> {
    /// Returns the bounded x origin for header text after optional icon space.
    fn header_text_x(&self, width: f32, has_icon: bool) -> f32 {
        let mut x: f32 = 14.0;
        if has_icon {
            x += 20.0;
        }
        x.min(width)
    }
}

/// Builds one breadcrumb child when file integration is enabled, otherwise none.
fn editor_pane_breadcrumb_children<A: 'static>(
    tabs: Vec<EditorPaneTab>,
    bound_tabs: Option<Signal<Vec<EditorPaneTab>>>,
    active_tab: Option<Binding<String>>,
    active_path: Option<Binding<String>>,
    active_document: Option<Signal<Document>>,
    style: EditorPaneStyle,
) -> Vec<View<A>> {
    #[cfg(feature = "files")]
    {
        vec![View::leaf(EditorPaneBreadcrumbWidget {
            tabs,
            bound_tabs,
            active_tab,
            active_path,
            active_document,
            style: FileBreadcrumbStyle {
                text: TextStyle::new(FontId::Ui, 12, style.path_fg),
                active_text: TextStyle::new(FontId::Ui, 12, style.title_fg),
                separator: TextStyle::new(FontId::Ui, 12, style.path_fg.with_alpha(0.72)),
                gap: 6.0,
            },
        })]
    }
    #[cfg(not(feature = "files"))]
    {
        let _ = tabs;
        let _ = bound_tabs;
        let _ = active_tab;
        let _ = active_path;
        let _ = active_document;
        let _ = style;
        Vec::new()
    }
}

#[cfg(feature = "files")]
/// Parses a URI or normalizes display-path separators into a local file URI.
///
/// Whitespace-only input and paths with no components return `None`. Native
/// backslashes and `parent > child` display paths become forward slashes.
fn breadcrumb_uri_from_path(path: &str) -> Option<FileUri> {
    let path = path.trim();
    if path.is_empty() {
        return None;
    }
    if let Ok(uri) = FileUri::parse(path) {
        return Some(uri);
    }

    let normalized = if path.contains('>') {
        path.split('>')
            .map(str::trim)
            .filter(|part| !part.is_empty())
            .collect::<Vec<_>>()
            .join("/")
    } else {
        path.replace('\\', "/")
    };
    let normalized = normalized.trim_matches('/');
    if normalized.is_empty() {
        return None;
    }
    FileUri::new("file", None::<String>, format!("/{normalized}")).ok()
}

impl<A: 'static> EditorPaneChromeWidget<A> {
    /// Resolves current signals and explicit metadata into one paint/hit model.
    ///
    /// Bound tabs override static tabs. Empty/unknown active identifiers select
    /// the first resolved tab. Explicit non-empty title/path values override tab
    /// values, which override document metadata; an explicit dirty boolean wins
    /// over the tab/document logical-OR fallback.
    fn model(&self) -> EditorPaneChromeModel {
        let document = self.active_document.as_ref().map(Signal::read);
        let document_meta = document.as_ref().and_then(document_title_path);
        let document_dirty = document.as_ref().is_some_and(|document| document.dirty);

        let source_tabs = self
            .bound_tabs
            .as_ref()
            .map(Signal::read)
            .unwrap_or_else(|| self.tabs.clone());

        let active_id = self
            .active_tab
            .as_ref()
            .map(Binding::read)
            .filter(|id| !id.is_empty())
            .or_else(|| source_tabs.first().map(|tab| tab.id.clone()));

        let active_tab = active_id
            .as_ref()
            .and_then(|id| source_tabs.iter().find(|tab| &tab.id == id));

        let explicit_title = self
            .active_title
            .as_ref()
            .map(Binding::read)
            .filter(|title| !title.is_empty());
        let explicit_path = self
            .active_path
            .as_ref()
            .map(Binding::read)
            .filter(|path| !path.is_empty());
        let explicit_dirty = self.dirty.as_ref().map(Binding::read);

        let active_title = explicit_title
            .clone()
            .or_else(|| {
                active_tab
                    .map(|tab| tab.title.clone())
                    .filter(|title| !title.is_empty())
            })
            .or_else(|| document_meta.as_ref().map(|meta| meta.title.clone()))
            .unwrap_or_default();
        let active_path = explicit_path
            .or_else(|| active_tab.and_then(|tab| tab.path.clone()))
            .or_else(|| document_meta.as_ref().and_then(|meta| meta.path.clone()));
        let active_dirty = explicit_dirty
            .unwrap_or_else(|| active_tab.is_some_and(|tab| tab.dirty) || document_dirty);
        let active_icon = active_tab.and_then(|tab| tab.icon.clone());
        let active_icon_tint = active_tab.and_then(|tab| tab.icon_tint);

        let mut tabs = if source_tabs.is_empty() {
            Vec::new()
        } else {
            source_tabs
                .iter()
                .map(|tab| {
                    let selected = active_id.as_ref().is_some_and(|id| id == &tab.id);
                    ResolvedEditorPaneTab {
                        id: tab.id.clone(),
                        title: if selected {
                            explicit_title.clone().unwrap_or_else(|| tab.title.clone())
                        } else {
                            tab.title.clone()
                        },
                        selected,
                        dirty: if selected {
                            explicit_dirty.unwrap_or(tab.dirty || document_dirty)
                        } else {
                            tab.dirty
                        },
                        kind: tab.kind,
                        icon: tab.icon.clone(),
                        icon_tint: tab.icon_tint,
                    }
                })
                .collect()
        };

        if !tabs.iter().any(|tab| tab.selected) {
            if let Some(first) = tabs.first_mut() {
                first.selected = true;
            }
        }

        EditorPaneChromeModel {
            tabs,
            active_title,
            #[cfg(feature = "files")]
            breadcrumb_uri: active_path.as_deref().and_then(breadcrumb_uri_from_path),
            active_path,
            active_dirty,
            active_icon,
            active_icon_tint,
        }
    }

    /// Commits bound selection, invokes callbacks in order, and requests paint.
    fn emit_select(&self, ctx: &mut EventCtx<A>, id: String) {
        if let Some(active) = &self.bound_active_tab {
            active.set(id.clone());
        }
        if let Some(handler) = &self.on_select_tab {
            handler(ctx, id.clone());
        }
        if let Some(handler) = &self.on_action {
            handler(ctx, EditorPaneAction::SelectTab(id));
        }
        ctx.request_repaint();
    }

    /// Invokes specialized then aggregate close callbacks and requests paint.
    fn emit_close(&self, ctx: &mut EventCtx<A>, id: String) {
        if let Some(handler) = &self.on_close_tab {
            handler(ctx, id.clone());
        }
        if let Some(handler) = &self.on_action {
            handler(ctx, EditorPaneAction::CloseTab(id));
        }
        ctx.request_repaint();
    }
}

#[cfg(feature = "files")]
/// Paint-only breadcrumb whose URI follows pane title/path precedence.
struct EditorPaneBreadcrumbWidget {
    tabs: Vec<EditorPaneTab>,
    bound_tabs: Option<Signal<Vec<EditorPaneTab>>>,
    active_tab: Option<Binding<String>>,
    active_path: Option<Binding<String>>,
    active_document: Option<Signal<Document>>,
    style: FileBreadcrumbStyle,
}

#[cfg(feature = "files")]
impl EditorPaneBreadcrumbWidget {
    /// Resolves the active path and converts it to a breadcrumb-compatible URI.
    fn breadcrumb_uri(&self) -> Option<FileUri> {
        let document = self.active_document.as_ref().map(Signal::read);
        let document_meta = document.as_ref().and_then(document_title_path);
        let source_tabs = self
            .bound_tabs
            .as_ref()
            .map(Signal::read)
            .unwrap_or_else(|| self.tabs.clone());
        let active_id = self
            .active_tab
            .as_ref()
            .map(Binding::read)
            .filter(|id| !id.is_empty())
            .or_else(|| source_tabs.first().map(|tab| tab.id.clone()));
        let active_tab = active_id
            .as_ref()
            .and_then(|id| source_tabs.iter().find(|tab| &tab.id == id));
        let active_path = self
            .active_path
            .as_ref()
            .map(Binding::read)
            .filter(|path| !path.is_empty())
            .or_else(|| active_tab.and_then(|tab| tab.path.clone()))
            .or_else(|| document_meta.as_ref().and_then(|meta| meta.path.clone()));
        active_path.as_deref().and_then(breadcrumb_uri_from_path)
    }
}

#[cfg(feature = "files")]
impl<A: 'static> Widget<A> for EditorPaneBreadcrumbWidget {
    fn debug_name(&self) -> &'static str {
        "EditorPaneBreadcrumb"
    }

    fn layout(
        &self,
        _engine: &mut ailloli_ui_runtime::layout::LayoutEngine<'_, A>,
        _ctx: &mut LayoutCtx<'_>,
        _children: &mut [LayoutChild],
        constraints: Constraints,
    ) -> LayoutResult {
        let size = constraints.constrain(Size::new(constraints.max_w, constraints.max_h));
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

    fn paint(&self, ctx: &mut PaintCtx<'_>, bounds: Rect, _layout: &LayoutResult) {
        let Some(uri) = self.breadcrumb_uri() else {
            return;
        };
        let segments = breadcrumb_segments(&uri, None, None);
        let mut x = bounds.x;
        let baseline = bounds.y + (bounds.h * 0.5 + 4.0).min(bounds.h);
        let mut cmds = Vec::new();
        {
            let Some(text_system) = ctx.text_system.as_deref_mut() else {
                return;
            };
            for (idx, segment) in segments.into_iter().enumerate() {
                if idx > 0 {
                    let (cmd, separator_w) =
                        breadcrumb_text_cmd(text_system, x, baseline, self.style.separator, ">");
                    cmds.push(cmd);
                    x += separator_w + self.style.gap;
                }
                let text_style = if segment.last {
                    self.style.active_text
                } else {
                    self.style.text
                };
                let (cmd, label_w) =
                    breadcrumb_text_cmd(text_system, x, baseline, text_style, &segment.label);
                cmds.push(cmd);
                x += label_w + self.style.gap;
            }
        }
        for cmd in cmds {
            ctx.push(cmd);
        }
    }

    fn event(&self, _ctx: &mut EventCtx<A>, _event: &Event, _bounds: Rect, _layout: &LayoutResult) {
    }
}

#[cfg(feature = "files")]
/// Measures and builds one no-wrap breadcrumb label draw command.
fn breadcrumb_text_cmd(
    text_system: &mut TextSystem,
    x: f32,
    baseline: f32,
    style: TextStyle,
    value: &str,
) -> (DrawCmd, f32) {
    let layout = text_system.layout_cached(TextLayoutParams {
        text: value,
        style,
        max_width: None,
        wrap_mode: WrapMode::NoWrap,
    });
    let width = layout.metrics.width;
    (
        DrawCmd::Text(DrawText {
            pos: [x, baseline],
            color: style.color,
            decoration: ailloli_ui_core::TextDecoration::None,
            layout,
        }),
        width,
    )
}

/// Reports whether file-enabled chrome will paint a breadcrumb child.
fn model_has_breadcrumb(model: &EditorPaneChromeModel) -> bool {
    #[cfg(feature = "files")]
    {
        model.breadcrumb_uri.is_some()
    }
    #[cfg(not(feature = "files"))]
    {
        let _ = model;
        false
    }
}

/// Header title and optional display path derived from a document source.
struct DocumentMeta {
    title: String,
    path: Option<String>,
}

/// Resolves legacy `path` first, then local/URI source; memory-only gives none.
fn document_title_path(document: &Document) -> Option<DocumentMeta> {
    if let Some(path) = document.path.as_deref() {
        return Some(DocumentMeta {
            title: path_file_name(path).unwrap_or_else(|| "Untitled".to_string()),
            path: Some(path.display().to_string()),
        });
    }
    match &document.source {
        DocumentSource::Memory => None,
        DocumentSource::LocalPath(path) => Some(DocumentMeta {
            title: path_file_name(path).unwrap_or_else(|| "Untitled".to_string()),
            path: Some(path.display().to_string()),
        }),
        DocumentSource::Uri(uri) => Some(DocumentMeta {
            title: uri.file_name().unwrap_or("Untitled").to_string(),
            path: Some(uri.to_string()),
        }),
    }
}

/// Returns an owned UTF-8 filename, or `None` for missing/non-UTF-8 names.
fn path_file_name(path: &Path) -> Option<String> {
    path.file_name()
        .and_then(|name| name.to_str())
        .map(ToOwned::to_owned)
}

#[derive(Clone)]
/// Internal tab projection consumed by the shared tab-bar renderer.
struct ResolvedEditorPaneTab {
    id: String,
    title: String,
    selected: bool,
    dirty: bool,
    kind: EditorPaneTabKind,
    icon: Option<IconId>,
    icon_tint: Option<Color>,
}

impl TabsItem for ResolvedEditorPaneTab {
    fn id(&self) -> &str {
        &self.id
    }

    fn title(&self) -> &str {
        &self.title
    }

    fn selected(&self) -> bool {
        self.selected
    }

    fn leading_icon(&self) -> Option<&IconId> {
        self.icon.as_ref()
    }

    fn leading_icon_tint(&self) -> Option<Color> {
        self.icon_tint
    }

    fn scope_kind(&self) -> &str {
        match self.kind {
            EditorPaneTabKind::Text => "task",
            EditorPaneTabKind::Code => "file",
            EditorPaneTabKind::Other => "",
        }
    }

    fn unread(&self) -> bool {
        self.dirty
    }
}

/// Immutable per-frame chrome values resolved from all static/reactive inputs.
struct EditorPaneChromeModel {
    tabs: Vec<ResolvedEditorPaneTab>,
    active_title: String,
    #[cfg(feature = "files")]
    breadcrumb_uri: Option<FileUri>,
    active_path: Option<String>,
    active_dirty: bool,
    active_icon: Option<IconId>,
    active_icon_tint: Option<Color>,
}

/// Reconstructs tab and close hit rectangles used by the shared painter.
///
/// Horizontal padding is 8 logical pixels, gaps are 6, visible tabs are
/// 120..=220 pixels wide, and each trailing close target is 22 pixels. Tabs
/// that cannot fit their 120-pixel minimum are omitted from hit testing.
fn tab_hit_layout(rect: Rect, tabs: &[ResolvedEditorPaneTab]) -> Vec<(String, Rect, Rect)> {
    let pad_x = 8.0;
    let pad_y = 4.0;
    let gap = 6.0;
    let mut x = rect.x + pad_x;
    let y = rect.y + pad_y;
    let h = rect.h - pad_y * 2.0;
    let x_end = x + (rect.w - pad_x * 2.0).max(0.0);
    let mut out = Vec::new();
    for tab in tabs {
        if x + 120.0 > x_end {
            break;
        }
        let w = 220.0_f32.min((x_end - x).max(120.0));
        let tab_r = Rect::new(x, y, w, h);
        out.push((
            tab.id.clone(),
            tab_r,
            Rect::new(tab_r.x + tab_r.w - 22.0, tab_r.y, 22.0, tab_r.h),
        ));
        x += w + gap;
    }
    out
}

/// Creates an unbounded no-wrap UI-font label in logical-pixel coordinates.
fn label_cmd(
    text: &mut TextSystem,
    pos: [f32; 2],
    color: Color,
    px_size: u16,
    value: &str,
) -> DrawCmd {
    DrawCmd::Text(DrawText {
        pos,
        color,
        decoration: ailloli_ui_core::TextDecoration::None,
        layout: text.layout_cached(TextLayoutParams {
            text: value,
            style: TextStyle::new(FontId::Ui, px_size, color),
            max_width: None,
            wrap_mode: WrapMode::NoWrap,
        }),
    })
}
