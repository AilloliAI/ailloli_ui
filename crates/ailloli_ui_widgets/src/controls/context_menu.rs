use std::rc::Rc;

use crate::layout::layout_ext::{apply_layout_size, finish_view_sized};
use ailloli_ui_core::event::pointer::{MouseButton, PointerEvent};
use ailloli_ui_core::event::{Event, Key, KeyState, NamedKey};
use ailloli_ui_core::geometry::{Constraints, Rect, Size};
use ailloli_ui_core::style::{Border, FlexItemStyle, LayoutSizeHint, LayoutStyle, Radius};
use ailloli_ui_core::{Color, FontId, IconId, Offset, Point, TextStyle, Theme};
use ailloli_ui_runtime::component::{
    Binding, ComponentNode, Context, IntoView, Signal, View, Widget,
};
use ailloli_ui_runtime::input::{ClickAction, EventCtx, FocusPolicy, IntoClickAction};
use ailloli_ui_runtime::layout::{ChildLayout, LayoutChild, LayoutCtx, LayoutResult};
use ailloli_ui_runtime::scene::PaintCtx;
use ailloli_ui_runtime::{DrawCmd, DrawImage, DrawRect};
use lucide_icons::Icon;

use super::popup::{
    apply_opacity, measure_text, paint_overlay_text_in_rect, paint_popup_border, paint_popup_shell,
    popup_rect_at_pointer,
};
use super::select::{SelectSize, SelectStyle};

#[derive(Clone, Debug, PartialEq)]
pub struct ContextMenuStyle {
    pub popup: SelectStyle,
    pub shortcut_text: TextStyle,
    pub separator: Color,
    pub row_height: f32,
    pub separator_height: f32,
    pub width: f32,
    pub submenu_width: f32,
    pub submenu_gap: f32,
    pub icon_size: f32,
    pub icon_gap: f32,
    pub padding_x: f32,
    pub disabled_opacity: f32,
}

impl Default for ContextMenuStyle {
    fn default() -> Self {
        Self::from_theme(Theme::default())
    }
}

impl ContextMenuStyle {
    pub fn from_theme(theme: Theme) -> Self {
        let palette = theme.palette();
        let mut popup = SelectStyle::from_theme(theme, SelectSize::Compact);
        popup.width = 252.0;
        popup.option_height = 28.0;
        popup.popup_max_height = 420.0;
        popup.radius = Radius::uniform(theme.radius().md);
        popup.popup_border = Border::new(1.0, palette.border);
        Self {
            shortcut_text: TextStyle::new(FontId::Mono, 11, palette.text_muted),
            separator: palette.border.with_alpha(0.86),
            row_height: 28.0,
            separator_height: 9.0,
            width: 252.0,
            submenu_width: 228.0,
            submenu_gap: 4.0,
            icon_size: 14.0,
            icon_gap: 8.0,
            padding_x: 10.0,
            disabled_opacity: 0.45,
            popup,
        }
    }
}

pub enum ContextMenuEntry<A = ()> {
    Item(ContextMenuItem<A>),
    Separator,
}

impl<A> Clone for ContextMenuEntry<A> {
    fn clone(&self) -> Self {
        match self {
            Self::Item(item) => Self::Item(item.clone()),
            Self::Separator => Self::Separator,
        }
    }
}

pub struct ContextMenuItem<A = ()> {
    label: String,
    shortcut: Option<String>,
    icon: Option<IconId>,
    disabled: Binding<bool>,
    action: Option<Rc<ClickAction<A>>>,
    submenu: Vec<ContextMenuEntry<A>>,
}

impl<A> Clone for ContextMenuItem<A> {
    fn clone(&self) -> Self {
        Self {
            label: self.label.clone(),
            shortcut: self.shortcut.clone(),
            icon: self.icon.clone(),
            disabled: self.disabled.clone(),
            action: self.action.clone(),
            submenu: self.submenu.clone(),
        }
    }
}

impl<A: 'static> ContextMenuItem<A> {
    pub fn new(label: impl Into<String>) -> Self {
        Self {
            label: label.into(),
            shortcut: None,
            icon: None,
            disabled: Binding::Static(false),
            action: None,
            submenu: Vec::new(),
        }
    }

    pub fn shortcut(mut self, shortcut: impl Into<String>) -> Self {
        self.shortcut = Some(shortcut.into());
        self
    }

    pub fn leading_icon(mut self, icon: IconId) -> Self {
        self.icon = Some(icon);
        self
    }

    pub fn disabled(mut self, disabled: impl Into<Binding<bool>>) -> Self {
        self.disabled = disabled.into();
        self
    }

    pub fn on_select(mut self, action: impl IntoClickAction<A>) -> Self {
        self.action = Some(Rc::new(action.into_click_action()));
        self
    }

    pub fn submenu(mut self, entries: impl IntoIterator<Item = ContextMenuEntry<A>>) -> Self {
        self.submenu = entries.into_iter().collect();
        self
    }

    pub fn is_disabled(&self) -> bool {
        self.disabled.read()
    }

    pub fn label(&self) -> &str {
        &self.label
    }

    pub fn shortcut_label(&self) -> Option<&str> {
        self.shortcut.as_deref()
    }

    pub fn submenu_entries(&self) -> &[ContextMenuEntry<A>] {
        &self.submenu
    }
}

pub struct ContextMenu<A = ()> {
    pub(crate) layout: LayoutStyle,
    pub(crate) flex_item: FlexItemStyle,
    open: Option<Binding<bool>>,
    bound_open: Option<Signal<bool>>,
    default_open: bool,
    anchor: Binding<Point>,
    entries: Binding<Vec<ContextMenuEntry<A>>>,
    disabled: Binding<bool>,
    style: ContextMenuStyle,
    child: Option<View<A>>,
}

crate::impl_layout_builders!(ContextMenu);

impl<A: 'static> Default for ContextMenu<A> {
    fn default() -> Self {
        Self::empty()
    }
}

impl<A: 'static> ContextMenu<A> {
    pub fn new(child: impl IntoView<A>) -> Self {
        Self::empty().child(child)
    }

    pub fn empty() -> Self {
        Self {
            layout: LayoutStyle::default(),
            flex_item: FlexItemStyle::default(),
            open: None,
            bound_open: None,
            default_open: false,
            anchor: Binding::Static(Point::default()),
            entries: Binding::Static(Vec::new()),
            disabled: Binding::Static(false),
            style: ContextMenuStyle::default(),
            child: None,
        }
    }

    pub fn open(mut self, open: impl Into<Binding<bool>>) -> Self {
        self.open = Some(open.into());
        self.bound_open = None;
        self
    }

    pub fn bind_open(mut self, open: impl Into<Signal<bool>>) -> Self {
        let signal = open.into();
        self.open = Some(Binding::Signal(signal.clone()));
        self.bound_open = Some(signal);
        self
    }

    pub fn default_open(mut self, open: bool) -> Self {
        self.default_open = open;
        self
    }

    pub fn anchor(mut self, anchor: impl Into<Binding<Point>>) -> Self {
        self.anchor = anchor.into();
        self
    }

    pub fn bind_anchor(mut self, anchor: impl Into<Signal<Point>>) -> Self {
        self.anchor = Binding::Signal(anchor.into());
        self
    }

    pub fn entries(mut self, entries: impl Into<Binding<Vec<ContextMenuEntry<A>>>>) -> Self {
        self.entries = entries.into();
        self
    }

    pub fn bind_entries(mut self, entries: impl Into<Signal<Vec<ContextMenuEntry<A>>>>) -> Self {
        self.entries = Binding::Signal(entries.into());
        self
    }

    pub fn disabled(mut self, disabled: impl Into<Binding<bool>>) -> Self {
        self.disabled = disabled.into();
        self
    }

    pub fn context_menu_style(mut self, style: ContextMenuStyle) -> Self {
        self.style = style;
        self
    }

    pub fn menu_width(mut self, width: f32) -> Self {
        self.style.width = width;
        self.style.popup.width = width;
        self
    }

    pub fn child(mut self, child: impl IntoView<A>) -> Self {
        self.child = Some(child.into_view());
        self
    }
}

struct ContextMenuComponent<A> {
    layout: LayoutStyle,
    open: Option<Binding<bool>>,
    bound_open: Option<Signal<bool>>,
    default_open: bool,
    anchor: Binding<Point>,
    entries: Binding<Vec<ContextMenuEntry<A>>>,
    disabled: Binding<bool>,
    style: ContextMenuStyle,
    child: Option<View<A>>,
}

impl<A: 'static> ComponentNode<A> for ContextMenuComponent<A> {
    fn build(&self, context: &mut Context<A>) -> View<A> {
        let mut children = Vec::new();
        if let Some(child) = self.child.clone() {
            children.push(child);
        }
        View::node(
            ContextMenuWidget {
                layout: self.layout,
                open: self.open.clone(),
                bound_open: self.bound_open.clone(),
                internal_open: context.signal(self.default_open),
                anchor: self.anchor.clone(),
                entries: self.entries.clone(),
                disabled: self.disabled.clone(),
                style: self.style.clone(),
                active_index: context.signal(None),
                submenu_parent_index: context.signal(None),
                submenu_active_index: context.signal(None),
                pressed_entry: context.signal(None),
            },
            children,
        )
    }
}

impl<A: 'static> IntoView<A> for ContextMenu<A> {
    fn into_view(self) -> View<A> {
        finish_view_sized(
            View::component(ContextMenuComponent {
                layout: self.layout,
                open: self.open,
                bound_open: self.bound_open,
                default_open: self.default_open,
                anchor: self.anchor,
                entries: self.entries,
                disabled: self.disabled,
                style: self.style,
                child: self.child,
            }),
            self.flex_item,
            LayoutSizeHint::from_layout(self.layout),
        )
    }
}

struct ContextMenuWidget<A> {
    layout: LayoutStyle,
    open: Option<Binding<bool>>,
    bound_open: Option<Signal<bool>>,
    internal_open: Signal<bool>,
    anchor: Binding<Point>,
    entries: Binding<Vec<ContextMenuEntry<A>>>,
    disabled: Binding<bool>,
    style: ContextMenuStyle,
    active_index: Signal<Option<usize>>,
    submenu_parent_index: Signal<Option<usize>>,
    submenu_active_index: Signal<Option<usize>>,
    pressed_entry: Signal<Option<ContextMenuPressedEntry>>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct ContextMenuPressedEntry {
    parent: Option<usize>,
    index: usize,
}

impl<A: 'static> Widget<A> for ContextMenuWidget<A> {
    fn debug_name(&self) -> &'static str {
        "ContextMenu"
    }

    fn layout(
        &self,
        engine: &mut ailloli_ui_runtime::layout::LayoutEngine<'_, A>,
        ctx: &mut LayoutCtx<'_>,
        children: &mut [LayoutChild],
        constraints: Constraints,
    ) -> LayoutResult {
        let size = host_slot_size(engine, ctx, children, constraints, self.layout);
        let mut child_layouts = Vec::new();
        if let Some(child) = children.first_mut() {
            let r = child.layout(engine, ctx, Constraints::tight(size.w, size.h));
            child_layouts.push(ChildLayout {
                offset: Offset::default(),
                size: r.size,
                paint_bounds: Rect::new(0.0, 0.0, r.size.w, r.size.h),
                visual_bounds: r.visual_bounds,
            });
        }

        let paint_bounds = Rect::new(0.0, 0.0, size.w, size.h);
        let overlay_hit_bounds = if self.is_open() && !self.disabled.read() {
            vec![paint_bounds]
        } else {
            Vec::new()
        };

        LayoutResult {
            size,
            children: child_layouts,
            paint_bounds,
            visual_bounds: paint_bounds,
            overlay_hit_bounds,
            clip: None,
            is_window_root_clip: false,
            artifact: None,
        }
    }

    fn paint(&self, _ctx: &mut PaintCtx<'_>, _bounds: Rect, _layout: &LayoutResult) {}

    fn paint_overlay(&self, ctx: &mut PaintCtx<'_>, bounds: Rect, _layout: &LayoutResult) {
        if !self.is_open() || self.disabled.read() {
            return;
        }
        let entries = self.entries.read();
        if entries.is_empty() {
            return;
        }
        self.paint_menu(ctx, bounds, &entries);
    }

    fn event(&self, ctx: &mut EventCtx<A>, event: &Event, bounds: Rect, _layout: &LayoutResult) {
        if !self.is_open() || self.disabled.read() {
            return;
        }
        match event {
            Event::Focus(focus) if !focus.focused => {
                self.close();
                ctx.request_repaint();
            }
            Event::Keyboard(key) if key.state == KeyState::Pressed => {
                self.handle_keyboard(ctx, &key.key);
                ctx.stop_propagation();
            }
            Event::Pointer(PointerEvent::Moved { pos, .. }) => {
                self.handle_pointer_move(ctx, bounds, *pos);
                ctx.stop_propagation();
            }
            Event::Pointer(PointerEvent::Button {
                pos,
                button: MouseButton::Left | MouseButton::Right,
                pressed: true,
                ..
            }) => {
                self.handle_pointer_press(ctx, bounds, *pos);
                ctx.stop_propagation();
            }
            Event::Pointer(PointerEvent::Button {
                pos,
                button: MouseButton::Left,
                pressed: false,
                ..
            }) => {
                self.handle_pointer_release(ctx, bounds, *pos, MouseButton::Left);
                ctx.stop_propagation();
            }
            Event::Pointer(PointerEvent::Button {
                pos,
                button: MouseButton::Right,
                pressed: false,
                ..
            }) => {
                self.handle_pointer_release(ctx, bounds, *pos, MouseButton::Right);
                ctx.stop_propagation();
            }
            _ => {}
        }
    }

    fn focus_policy(&self) -> FocusPolicy {
        if self.is_open() && !self.disabled.read() {
            FocusPolicy::Focusable
        } else {
            FocusPolicy::NotFocusable
        }
    }
}

impl<A: 'static> ContextMenuWidget<A> {
    fn is_open(&self) -> bool {
        self.open
            .as_ref()
            .map(Binding::read)
            .unwrap_or_else(|| self.internal_open.read())
    }

    fn close(&self) {
        if let Some(bound) = &self.bound_open {
            bound.set(false);
        } else if self.open.is_none() {
            self.internal_open.set(false);
        }
        self.active_index.set(None);
        self.submenu_parent_index.set(None);
        self.submenu_active_index.set(None);
        self.pressed_entry.set(None);
    }

    fn menu_rect(&self, bounds: Rect, entries: &[ContextMenuEntry<A>]) -> Rect {
        popup_rect_at_pointer(
            self.anchor.read().x,
            self.anchor.read().y,
            self.style.width,
            self.menu_height(entries)
                .min(self.style.popup.popup_max_height),
            bounds,
        )
    }

    fn submenu_rect(
        &self,
        bounds: Rect,
        menu: Rect,
        parent_row: Rect,
        entries: &[ContextMenuEntry<A>],
    ) -> Rect {
        let desired = Rect::new(
            menu.right() + self.style.submenu_gap,
            parent_row.y,
            self.style.submenu_width,
            self.menu_height(entries)
                .min(self.style.popup.popup_max_height),
        );
        super::popup::clamp_rect_to_bounds(desired, bounds)
    }

    fn menu_height(&self, entries: &[ContextMenuEntry<A>]) -> f32 {
        entries.iter().map(|entry| self.entry_height(entry)).sum()
    }

    fn entry_height(&self, entry: &ContextMenuEntry<A>) -> f32 {
        match entry {
            ContextMenuEntry::Item(_) => self.style.row_height,
            ContextMenuEntry::Separator => self.style.separator_height,
        }
    }

    fn entry_row(&self, menu: Rect, entries: &[ContextMenuEntry<A>], index: usize) -> Option<Rect> {
        if index >= entries.len() {
            return None;
        }
        let mut y = menu.y;
        for entry in &entries[..index] {
            y += self.entry_height(entry);
        }
        Some(Rect::new(
            menu.x,
            y,
            menu.w,
            self.entry_height(&entries[index]),
        ))
    }

    fn item_index_at(
        &self,
        menu: Rect,
        entries: &[ContextMenuEntry<A>],
        pos: Point,
    ) -> Option<usize> {
        if !menu.contains(pos.x, pos.y) {
            return None;
        }
        let mut y = menu.y;
        for (idx, entry) in entries.iter().enumerate() {
            let height = self.entry_height(entry);
            let row = Rect::new(menu.x, y, menu.w, height);
            if row.contains(pos.x, pos.y) {
                return matches!(entry, ContextMenuEntry::Item(_)).then_some(idx);
            }
            y += height;
        }
        None
    }

    fn first_enabled_index(&self, entries: &[ContextMenuEntry<A>]) -> Option<usize> {
        entries.iter().enumerate().find_map(|(idx, entry)| {
            let ContextMenuEntry::Item(item) = entry else {
                return None;
            };
            (!item.is_disabled()).then_some(idx)
        })
    }

    fn last_enabled_index(&self, entries: &[ContextMenuEntry<A>]) -> Option<usize> {
        entries.iter().enumerate().rev().find_map(|(idx, entry)| {
            let ContextMenuEntry::Item(item) = entry else {
                return None;
            };
            (!item.is_disabled()).then_some(idx)
        })
    }

    fn next_enabled_index(
        &self,
        entries: &[ContextMenuEntry<A>],
        current: Option<usize>,
    ) -> Option<usize> {
        let start = current.unwrap_or(usize::MAX);
        entries
            .iter()
            .enumerate()
            .cycle()
            .skip_while(|(idx, _)| *idx != start)
            .skip(1)
            .take(entries.len())
            .find_map(|(idx, entry)| {
                let ContextMenuEntry::Item(item) = entry else {
                    return None;
                };
                (!item.is_disabled()).then_some(idx)
            })
            .or_else(|| self.first_enabled_index(entries))
    }

    fn previous_enabled_index(
        &self,
        entries: &[ContextMenuEntry<A>],
        current: Option<usize>,
    ) -> Option<usize> {
        let enabled: Vec<usize> = entries
            .iter()
            .enumerate()
            .filter_map(|(idx, entry)| {
                let ContextMenuEntry::Item(item) = entry else {
                    return None;
                };
                (!item.is_disabled()).then_some(idx)
            })
            .collect();
        if enabled.is_empty() {
            return None;
        }
        let pos = current
            .and_then(|current| enabled.iter().position(|idx| *idx == current))
            .unwrap_or(0);
        Some(enabled[(pos + enabled.len() - 1) % enabled.len()])
    }

    fn handle_keyboard(&self, ctx: &mut EventCtx<A>, key: &Key) {
        let entries = self.entries.read();
        match key {
            Key::Named(NamedKey::Escape) => {
                self.close();
                ctx.request_repaint();
                ctx.stop_propagation();
            }
            Key::Named(NamedKey::ArrowDown) => {
                self.set_active(
                    ctx,
                    self.next_enabled_index(&entries, self.active_index.read()),
                );
            }
            Key::Named(NamedKey::ArrowUp) => {
                self.set_active(
                    ctx,
                    self.previous_enabled_index(&entries, self.active_index.read()),
                );
            }
            Key::Named(NamedKey::Home) => {
                self.set_active(ctx, self.first_enabled_index(&entries));
            }
            Key::Named(NamedKey::End) => {
                self.set_active(ctx, self.last_enabled_index(&entries));
            }
            Key::Named(NamedKey::ArrowRight) => {
                if let Some(index) = self.active_index.read() {
                    self.open_submenu(ctx, &entries, index);
                }
            }
            Key::Named(NamedKey::ArrowLeft) => {
                self.submenu_parent_index.set(None);
                self.submenu_active_index.set(None);
                ctx.request_repaint();
                ctx.stop_propagation();
            }
            Key::Named(NamedKey::Enter) | Key::Named(NamedKey::Space) => {
                if let Some(parent) = self.submenu_parent_index.read() {
                    if let Some(sub) = self.submenu_active_index.read() {
                        if let Some(ContextMenuEntry::Item(item)) = entries.get(parent) {
                            self.activate_entry(ctx, &item.submenu, sub);
                            return;
                        }
                    }
                }
                if let Some(index) = self
                    .active_index
                    .read()
                    .or_else(|| self.first_enabled_index(&entries))
                {
                    self.activate_entry(ctx, &entries, index);
                }
            }
            _ => {}
        }
    }

    fn handle_pointer_move(&self, ctx: &mut EventCtx<A>, bounds: Rect, pos: Point) {
        let entries = self.entries.read();
        let menu = self.menu_rect(bounds, &entries);
        if let Some(index) = self.item_index_at(menu, &entries, pos) {
            self.active_index.set(Some(index));
            if self.has_submenu(&entries, index) {
                self.submenu_parent_index.set(Some(index));
                self.submenu_active_index.set(None);
            } else {
                self.submenu_parent_index.set(None);
                self.submenu_active_index.set(None);
            }
            ctx.request_repaint();
            return;
        }
        if let Some((parent, submenu, rect)) = self.open_submenu_geometry(bounds, menu, &entries) {
            if let Some(index) = self.item_index_at(rect, submenu, pos) {
                self.submenu_parent_index.set(Some(parent));
                self.submenu_active_index.set(Some(index));
                ctx.request_repaint();
            }
        }
    }

    fn handle_pointer_press(&self, ctx: &mut EventCtx<A>, bounds: Rect, pos: Point) {
        let entries = self.entries.read();
        let menu = self.menu_rect(bounds, &entries);
        self.pressed_entry.set(None);
        if let Some((parent, submenu, rect)) = self.open_submenu_geometry(bounds, menu, &entries) {
            if let Some(index) = self.item_index_at(rect, submenu, pos) {
                self.submenu_parent_index.set(Some(parent));
                self.submenu_active_index.set(Some(index));
                self.pressed_entry.set(Some(ContextMenuPressedEntry {
                    parent: Some(parent),
                    index,
                }));
                ctx.request_repaint();
                return;
            }
        }
        if let Some(index) = self.item_index_at(menu, &entries, pos) {
            if self.has_submenu(&entries, index) {
                self.open_submenu(ctx, &entries, index);
            } else {
                self.active_index.set(Some(index));
                self.pressed_entry.set(Some(ContextMenuPressedEntry {
                    parent: None,
                    index,
                }));
                ctx.request_repaint();
            }
            return;
        }
        self.close();
        ctx.request_repaint();
        ctx.stop_propagation();
    }

    fn handle_pointer_release(
        &self,
        ctx: &mut EventCtx<A>,
        bounds: Rect,
        pos: Point,
        button: MouseButton,
    ) {
        let pressed = self.pressed_entry.read();
        self.pressed_entry.set(None);
        if button != MouseButton::Left {
            ctx.request_repaint();
            return;
        }
        let entries = self.entries.read();
        let menu = self.menu_rect(bounds, &entries);
        let release = if let Some((parent, submenu, rect)) =
            self.open_submenu_geometry(bounds, menu, &entries)
        {
            self.item_index_at(rect, submenu, pos)
                .map(|index| (Some(parent), index))
        } else {
            None
        }
        .or_else(|| {
            self.item_index_at(menu, &entries, pos)
                .map(|index| (None, index))
        });
        let Some(pressed) = pressed else {
            ctx.request_repaint();
            return;
        };
        if release != Some((pressed.parent, pressed.index)) {
            ctx.request_repaint();
            return;
        }
        match pressed.parent {
            Some(parent) => {
                if let Some(ContextMenuEntry::Item(item)) = entries.get(parent) {
                    self.activate_entry(ctx, &item.submenu, pressed.index);
                }
            }
            None => {
                if self.has_submenu(&entries, pressed.index) {
                    self.open_submenu(ctx, &entries, pressed.index);
                } else {
                    self.activate_entry(ctx, &entries, pressed.index);
                }
            }
        }
    }

    fn activate_entry(&self, ctx: &mut EventCtx<A>, entries: &[ContextMenuEntry<A>], index: usize) {
        let Some(ContextMenuEntry::Item(item)) = entries.get(index) else {
            return;
        };
        if item.is_disabled() {
            ctx.stop_propagation();
            return;
        }
        if !item.submenu.is_empty() {
            self.open_submenu(ctx, entries, index);
            return;
        }
        if let Some(action) = &item.action {
            action.run(ctx);
        }
        self.close();
        ctx.request_repaint();
        ctx.stop_propagation();
    }

    fn open_submenu(&self, ctx: &mut EventCtx<A>, entries: &[ContextMenuEntry<A>], index: usize) {
        if !self.has_submenu(entries, index) {
            return;
        }
        self.active_index.set(Some(index));
        self.submenu_parent_index.set(Some(index));
        if let Some(ContextMenuEntry::Item(item)) = entries.get(index) {
            self.submenu_active_index
                .set(self.first_enabled_index(&item.submenu));
        }
        ctx.request_repaint();
        ctx.stop_propagation();
    }

    fn has_submenu(&self, entries: &[ContextMenuEntry<A>], index: usize) -> bool {
        matches!(entries.get(index), Some(ContextMenuEntry::Item(item)) if !item.submenu.is_empty())
    }

    fn set_active(&self, ctx: &mut EventCtx<A>, next: Option<usize>) {
        self.active_index.set(next);
        self.submenu_parent_index.set(None);
        self.submenu_active_index.set(None);
        ctx.request_repaint();
        ctx.stop_propagation();
    }

    fn open_submenu_geometry<'a>(
        &self,
        bounds: Rect,
        menu: Rect,
        entries: &'a [ContextMenuEntry<A>],
    ) -> Option<(usize, &'a [ContextMenuEntry<A>], Rect)> {
        let parent = self.submenu_parent_index.read()?;
        let ContextMenuEntry::Item(item) = entries.get(parent)? else {
            return None;
        };
        if item.submenu.is_empty() {
            return None;
        }
        let row = self.entry_row(menu, entries, parent)?;
        Some((
            parent,
            item.submenu.as_slice(),
            self.submenu_rect(bounds, menu, row, &item.submenu),
        ))
    }

    fn paint_menu(&self, ctx: &mut PaintCtx<'_>, bounds: Rect, entries: &[ContextMenuEntry<A>]) {
        let menu = self.menu_rect(bounds, entries);
        self.paint_entries(ctx, menu, entries, self.active_index.read());
        if let Some((_, submenu, rect)) = self.open_submenu_geometry(bounds, menu, entries) {
            self.paint_entries(ctx, rect, submenu, self.submenu_active_index.read());
        }
    }

    fn paint_entries(
        &self,
        ctx: &mut PaintCtx<'_>,
        menu: Rect,
        entries: &[ContextMenuEntry<A>],
        active: Option<usize>,
    ) {
        paint_popup_shell(ctx, menu, &self.style.popup);
        ctx.with_overlay_clip(menu, |ctx| {
            let mut y = menu.y;
            for (idx, entry) in entries.iter().enumerate() {
                let height = self.entry_height(entry);
                let row = Rect::new(menu.x, y, menu.w, height);
                match entry {
                    ContextMenuEntry::Item(item) => {
                        self.paint_item(ctx, row, item, active == Some(idx));
                    }
                    ContextMenuEntry::Separator => {
                        let line = Rect::new(
                            row.x + self.style.padding_x,
                            row.y + (row.h - 1.0) * 0.5,
                            (row.w - self.style.padding_x * 2.0).max(0.0),
                            1.0,
                        );
                        ctx.push_overlay(DrawCmd::Rect(DrawRect {
                            rect: line,
                            color: self.style.separator,
                        }));
                    }
                }
                y += height;
            }
        });
        paint_popup_border(ctx, menu, &self.style.popup);
    }

    fn paint_item(
        &self,
        ctx: &mut PaintCtx<'_>,
        row: Rect,
        item: &ContextMenuItem<A>,
        active: bool,
    ) {
        let disabled = item.is_disabled();
        let opacity = if disabled {
            self.style.disabled_opacity
        } else {
            1.0
        };
        if active {
            ctx.push_overlay(DrawCmd::Rect(DrawRect {
                rect: row,
                color: apply_opacity(self.style.popup.option_active, opacity),
            }));
        }

        let mut x = row.x + self.style.padding_x;
        if let Some(icon) = &item.icon {
            ctx.push_overlay(DrawCmd::Image(DrawImage {
                rect: Rect::new(
                    x,
                    row.y + (row.h - self.style.icon_size) * 0.5,
                    self.style.icon_size,
                    self.style.icon_size,
                ),
                icon: icon.clone(),
                tint: apply_opacity(
                    if disabled {
                        self.style.popup.disabled_icon_tint
                    } else {
                        self.style.popup.icon_tint
                    },
                    opacity,
                ),
                rotation_rad: 0.0,
            }));
            x += self.style.icon_size + self.style.icon_gap;
        }

        let trailing_icon_w = if item.submenu.is_empty() {
            0.0
        } else {
            self.style.icon_size + self.style.icon_gap
        };
        let shortcut_w = item
            .shortcut
            .as_ref()
            .map(|shortcut| {
                measure_text(
                    ctx.text_system.as_deref_mut(),
                    shortcut,
                    self.style.shortcut_text,
                )
                .w
            })
            .unwrap_or(0.0);
        let text_style = if disabled {
            self.style.popup.disabled_text
        } else {
            self.style.popup.text
        };
        let text_right =
            row.right() - self.style.padding_x - shortcut_w - self.style.icon_gap - trailing_icon_w;
        paint_overlay_text_in_rect(
            ctx,
            &item.label,
            text_style,
            Rect::new(x, row.y, (text_right - x).max(0.0), row.h),
            opacity,
        );
        if let Some(shortcut) = &item.shortcut {
            paint_overlay_text_in_rect(
                ctx,
                shortcut,
                self.style.shortcut_text,
                Rect::new(
                    row.right() - self.style.padding_x - shortcut_w - trailing_icon_w,
                    row.y,
                    shortcut_w,
                    row.h,
                ),
                opacity,
            );
        }
        if !item.submenu.is_empty() {
            ctx.push_overlay(DrawCmd::Image(DrawImage {
                rect: Rect::new(
                    row.right() - self.style.padding_x - self.style.icon_size,
                    row.y + (row.h - self.style.icon_size) * 0.5,
                    self.style.icon_size,
                    self.style.icon_size,
                ),
                icon: IconId::Lucide(Icon::ChevronRight),
                tint: apply_opacity(self.style.popup.icon_tint, opacity),
                rotation_rad: 0.0,
            }));
        }
    }
}

fn host_slot_size<A: 'static>(
    engine: &mut ailloli_ui_runtime::layout::LayoutEngine<'_, A>,
    ctx: &mut LayoutCtx<'_>,
    children: &mut [LayoutChild],
    constraints: Constraints,
    layout: LayoutStyle,
) -> Size {
    let intrinsic = if let Some(child) = children.first_mut() {
        child.layout(engine, ctx, constraints.loosen()).size
    } else {
        Size::new(
            finite_or(constraints.max_w, 0.0),
            finite_or(constraints.max_h, 0.0),
        )
    };
    apply_layout_size(intrinsic, layout, constraints)
}

fn finite_or(value: f32, fallback: f32) -> f32 {
    if value.is_finite() {
        value
    } else {
        fallback
    }
}
