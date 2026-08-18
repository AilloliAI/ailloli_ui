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

type ActionHandler<A> = Rc<dyn Fn(&mut EventCtx<A>, EditorPaneAction)>;
type TabHandler<A> = Rc<dyn Fn(&mut EventCtx<A>, String)>;

const DEFAULT_PANE_WIDTH: f32 = 640.0;
const DEFAULT_PANE_HEIGHT: f32 = 420.0;

#[derive(Clone, Debug, PartialEq)]
pub struct EditorPaneTab {
    pub id: String,
    pub title: String,
    pub path: Option<String>,
    pub dirty: bool,
    pub kind: EditorPaneTabKind,
    pub icon: Option<IconId>,
    pub icon_tint: Option<Color>,
}

impl EditorPaneTab {
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

    pub fn text(id: impl Into<String>, title: impl Into<String>) -> Self {
        Self::new(id, title).kind(EditorPaneTabKind::Text)
    }

    pub fn code(id: impl Into<String>, title: impl Into<String>) -> Self {
        Self::new(id, title).kind(EditorPaneTabKind::Code)
    }

    pub fn path(mut self, path: impl Into<String>) -> Self {
        self.path = Some(path.into());
        self
    }

    pub fn dirty(mut self, dirty: bool) -> Self {
        self.dirty = dirty;
        self
    }

    pub fn kind(mut self, kind: EditorPaneTabKind) -> Self {
        self.kind = kind;
        self
    }

    pub fn icon(mut self, icon: IconId) -> Self {
        self.icon = Some(icon);
        self
    }

    pub fn icon_tint(mut self, color: Color) -> Self {
        self.icon_tint = Some(color);
        self
    }
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum EditorPaneTabKind {
    Text,
    Code,
    #[default]
    Other,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum EditorPaneAction {
    SelectTab(String),
    CloseTab(String),
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum EditorPaneSize {
    Compact,
    #[default]
    Default,
}

#[derive(Clone, Debug)]
pub struct EditorPaneStyle {
    pub background: Color,
    pub border: Color,
    pub header_bg: Color,
    pub header_border: Color,
    pub title_fg: Color,
    pub path_fg: Color,
    pub dirty: Color,
    pub radius: f32,
    pub tabs_height: f32,
    pub header_height: f32,
    pub tabs: TabsStyle,
}

impl Default for EditorPaneStyle {
    fn default() -> Self {
        Self::from_theme(Theme::default(), EditorPaneSize::Default)
    }
}

impl EditorPaneStyle {
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

pub struct EditorPane<A = ()> {
    pub(crate) layout: LayoutStyle,
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

    pub fn text(buffer: impl Into<Signal<TextBuffer>>) -> Self {
        Self::new(Editor::new(buffer).fill())
    }

    pub fn code(document: impl Into<Signal<Document>>) -> Self {
        let document = document.into();
        Self::new(CodeEditor::<A>::new(document.clone()).fill()).with_active_document(document)
    }

    pub fn tabs(mut self, tabs: impl IntoIterator<Item = EditorPaneTab>) -> Self {
        self.tabs = tabs.into_iter().collect();
        self.bound_tabs = None;
        self
    }

    pub fn bind_tabs(mut self, tabs: impl Into<Signal<Vec<EditorPaneTab>>>) -> Self {
        self.bound_tabs = Some(tabs.into());
        self
    }

    pub fn active_tab(mut self, active_tab: impl Into<Binding<String>>) -> Self {
        self.active_tab = Some(active_tab.into());
        self.bound_active_tab = None;
        self
    }

    pub fn bind_active_tab(mut self, active_tab: impl Into<Signal<String>>) -> Self {
        let signal = active_tab.into();
        self.active_tab = Some(Binding::Signal(signal.clone()));
        self.bound_active_tab = Some(signal);
        self
    }

    pub fn active_title(mut self, title: impl Into<Binding<String>>) -> Self {
        self.active_title = Some(title.into());
        self
    }

    pub fn active_path(mut self, path: impl Into<Binding<String>>) -> Self {
        self.active_path = Some(path.into());
        self
    }

    pub fn dirty(mut self, dirty: impl Into<Binding<bool>>) -> Self {
        self.dirty = Some(dirty.into());
        self
    }

    pub fn pane_style(mut self, style: EditorPaneStyle) -> Self {
        self.style = style;
        self
    }

    pub fn pane_size(mut self, size: EditorPaneSize) -> Self {
        self.style = EditorPaneStyle::from_theme(Theme::default(), size);
        self
    }

    pub fn on_select_tab(mut self, f: impl Fn(String) -> A + 'static) -> Self {
        self.on_select_tab = Some(Rc::new(move |ctx, id| ctx.dispatch(f(id))));
        self
    }

    pub fn on_select_tab_ctx(mut self, f: impl Fn(&mut EventCtx<A>, String) + 'static) -> Self {
        self.on_select_tab = Some(Rc::new(f));
        self
    }

    pub fn on_close_tab(mut self, f: impl Fn(String) -> A + 'static) -> Self {
        self.on_close_tab = Some(Rc::new(move |ctx, id| ctx.dispatch(f(id))));
        self
    }

    pub fn on_close_tab_ctx(mut self, f: impl Fn(&mut EventCtx<A>, String) + 'static) -> Self {
        self.on_close_tab = Some(Rc::new(f));
        self
    }

    pub fn on_action(mut self, f: impl Fn(EditorPaneAction) -> A + 'static) -> Self {
        self.on_action = Some(Rc::new(move |ctx, action| ctx.dispatch(f(action))));
        self
    }

    pub fn on_action_ctx(
        mut self,
        f: impl Fn(&mut EventCtx<A>, EditorPaneAction) + 'static,
    ) -> Self {
        self.on_action = Some(Rc::new(f));
        self
    }

    fn with_active_document(mut self, document: Signal<Document>) -> Self {
        self.active_document = Some(document);
        self
    }
}

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

struct EditorPaneFrameWidget {
    layout: LayoutStyle,
    style: EditorPaneStyle,
}

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
    fn header_text_x(&self, width: f32, has_icon: bool) -> f32 {
        let mut x: f32 = 14.0;
        if has_icon {
            x += 20.0;
        }
        x.min(width)
    }
}

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
            layout,
        }),
        width,
    )
}

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

struct DocumentMeta {
    title: String,
    path: Option<String>,
}

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

fn path_file_name(path: &Path) -> Option<String> {
    path.file_name()
        .and_then(|name| name.to_str())
        .map(ToOwned::to_owned)
}

#[derive(Clone)]
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
        layout: text.layout_cached(TextLayoutParams {
            text: value,
            style: TextStyle::new(FontId::Ui, px_size, color),
            max_width: None,
            wrap_mode: WrapMode::NoWrap,
        }),
    })
}
