use ailloli_ui_core::event::pointer::{MouseButton, PointerEvent};
use ailloli_ui_core::event::FocusEvent;
use ailloli_ui_core::{ElementId, Event, Point, Rect};

use super::{
    absolute_paint_bounds, dispatch_event_bubbling, dispatch_event_to_target, hit_test_target,
    FocusManager, FocusPolicy, HitTestEngine, HoverCursorRole, InputRole,
};
use crate::app::RuntimeHandle;
use crate::element::{ElementKind, ElementTree, Key};
use crate::input::ChromeAction;

/// Whether `element_id` should show pointer interaction when hit target is `target`.
///
/// Avoids lighting all sibling buttons when the hit is an empty flex `Row`/`Column` gap.
fn widget_paint_pointer_match<A: 'static>(
    tree: &ElementTree<A>,
    element_id: ElementId,
    target: ElementId,
) -> bool {
    if element_id == target {
        return true;
    }
    if tree.is_ancestor_of(element_id, target) {
        return true;
    }
    if tree.parent_of(element_id) == Some(target) {
        return !flex_row_or_column_widget(tree, target);
    }
    false
}

fn flex_row_or_column_widget<A: 'static>(tree: &ElementTree<A>, id: ElementId) -> bool {
    tree.get(id).is_some_and(|el| {
        matches!(
            &el.kind,
            ElementKind::Widget(w) if matches!(w.debug_name(), "Row" | "Column")
        )
    })
}

#[derive(Debug, Clone, PartialEq)]
pub enum Action {
    PointerTargetChanged {
        old: Option<ElementId>,
        new: Option<ElementId>,
    },
    PointerButton {
        target: Option<ElementId>,
        pos: Point,
        pressed: bool,
    },
    Chrome(ChromeAction),
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct InputInteraction {
    pub focused: bool,
    pub hovered: bool,
    pub pressed: bool,
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct InputSnapshot {
    pub focused: Option<ElementId>,
    pub hovered: Option<ElementId>,
    pub pressed: Option<ElementId>,
}

impl InputSnapshot {
    pub fn interaction_for(self, id: ElementId) -> InputInteraction {
        InputInteraction {
            focused: self.focused == Some(id),
            hovered: self.hovered == Some(id),
            pressed: self.pressed == Some(id),
        }
    }

    /// Interaction state for paint: walks the hit-test chain without false sibling hovers.
    ///
    /// - Parent (e.g. `Button`) hovers when the pointer hits a child (`Icon`).
    /// - Child hovers when the hit is on a non-flex parent (button padding).
    /// - Hit on empty `Row`/`Column` gaps does not hover all sibling buttons.
    ///
    /// Focus remains strict on the focused element only.
    pub fn interaction_for_widget_paint<A: 'static>(
        self,
        tree: &crate::element::ElementTree<A>,
        id: ElementId,
    ) -> InputInteraction {
        let mut out = self.interaction_for(id);
        if let Some(h) = self.hovered {
            out.hovered = widget_paint_pointer_match(tree, id, h);
        }
        if let Some(p) = self.pressed {
            out.pressed = widget_paint_pointer_match(tree, id, p);
        }
        out
    }
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct RouteOutcome {
    pub interaction_changed: bool,
    pub event_dispatched: bool,
}

impl RouteOutcome {
    pub fn needs_redraw(&self) -> bool {
        self.interaction_changed
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct TargetSignature {
    key: Option<Key>,
    kind: TargetKind,
}

#[derive(Debug, Clone, PartialEq, Eq)]
enum TargetKind {
    Empty,
    Component,
    Widget {
        debug_name: &'static str,
        focus_policy: FocusPolicy,
    },
}

/// Per-window input state: hit-test, hover, press, focus, and event dispatch.
#[derive(Debug, Default, Clone)]
pub struct InputRouter {
    pub hit_test: HitTestEngine,
    pub focus: FocusManager,
    hovered: Option<ElementId>,
    pressed: Option<ElementId>,
    pointer_capture: Option<ElementId>,
    focused_signature: Option<TargetSignature>,
    hovered_signature: Option<TargetSignature>,
    pressed_signature: Option<TargetSignature>,
    pointer_capture_signature: Option<TargetSignature>,
    focused_input_role: InputRole,
    hovered_input_role: InputRole,
    pressed_input_role: InputRole,
    pointer_capture_input_role: InputRole,
}

impl InputRouter {
    pub fn hovered(&self) -> Option<ElementId> {
        self.hovered
    }

    pub fn focused(&self) -> Option<ElementId> {
        self.focus.focused()
    }

    pub fn snapshot(&self) -> InputSnapshot {
        InputSnapshot {
            focused: self.focused(),
            hovered: self.hovered,
            pressed: self.pressed,
        }
    }

    pub fn clear_pointer_state(&mut self) -> bool {
        let changed =
            self.hovered.is_some() || self.pressed.is_some() || self.pointer_capture.is_some();
        self.hovered = None;
        self.pressed = None;
        self.pointer_capture = None;
        self.hovered_signature = None;
        self.pressed_signature = None;
        self.pointer_capture_signature = None;
        self.hovered_input_role = InputRole::None;
        self.pressed_input_role = InputRole::None;
        self.pointer_capture_input_role = InputRole::None;
        changed
    }

    pub fn clear_focus(&mut self) -> bool {
        let changed = self.focused().is_some();
        self.focus.set_focused(None);
        self.focused_signature = None;
        self.focused_input_role = InputRole::None;
        changed
    }

    pub fn focused_input_role<A: 'static>(&self, tree: &ElementTree<A>) -> InputRole {
        self.focused()
            .and_then(|id| input_role(tree, id))
            .unwrap_or(InputRole::None)
    }

    pub fn hovered_input_role<A: 'static>(&self, tree: &ElementTree<A>) -> InputRole {
        self.hovered()
            .and_then(|id| input_role(tree, id))
            .unwrap_or(InputRole::None)
    }

    pub fn hovered_cursor_role<A: 'static>(&self, tree: &ElementTree<A>) -> HoverCursorRole {
        self.hovered_cursor_role_impl(tree, None)
    }

    pub fn hovered_cursor_role_at<A: 'static>(
        &self,
        tree: &ElementTree<A>,
        pos: Point,
    ) -> HoverCursorRole {
        self.hovered_cursor_role_impl(tree, Some(pos))
    }

    fn hovered_cursor_role_impl<A: 'static>(
        &self,
        tree: &ElementTree<A>,
        pos: Option<Point>,
    ) -> HoverCursorRole {
        let Some(mut id) = self.hovered() else {
            return HoverCursorRole::Default;
        };
        loop {
            match hover_cursor_role(tree, id, pos).unwrap_or(HoverCursorRole::Inherit) {
                HoverCursorRole::Inherit => {
                    let Some(parent) = tree.parent_of(id) else {
                        return HoverCursorRole::Default;
                    };
                    id = parent;
                }
                role @ (HoverCursorRole::Default
                | HoverCursorRole::Text
                | HoverCursorRole::ResizeX
                | HoverCursorRole::ResizeY) => return role,
            }
        }
    }

    pub fn focused_ime_cursor_rect<A: 'static>(&self, tree: &ElementTree<A>) -> Option<Rect> {
        let id = self.focused()?;
        let el = tree.get(id)?;
        let layout = el.layout.as_ref()?;
        let bounds = super::absolute_paint_bounds(tree, id).unwrap_or(layout.paint_bounds);
        match &el.kind {
            ElementKind::Widget(widget) => widget.ime_cursor_rect(bounds, layout),
            _ => None,
        }
    }

    pub fn route_event<A: 'static>(
        &mut self,
        tree: &ElementTree<A>,
        runtime: RuntimeHandle<A>,
        event: &Event,
    ) -> RouteOutcome {
        let stale_state_changed = self.retain_existing_targets(tree);
        let input_role_changed = self.refresh_target_input_roles(tree);
        let focus_request_changed = self.apply_pending_focus_request(tree, runtime.clone());
        let mut outcome = match event {
            Event::Pointer(PointerEvent::Moved { pos, .. }) => {
                self.route_pointer_move(tree, runtime.clone(), event, *pos)
            }
            Event::Pointer(PointerEvent::Button {
                pos,
                button,
                pressed,
                ..
            }) => self.route_pointer_button(tree, runtime.clone(), event, *pos, *button, *pressed),
            Event::Pointer(PointerEvent::Wheel { pos, .. }) => {
                let target = self.pointer_capture.or_else(|| self.hit(tree, *pos));
                self.dispatch_to_optional_target(tree, runtime.clone(), event, target, false)
            }
            Event::Keyboard(_) | Event::Ime(_) => {
                let target = self.focused();
                self.dispatch_to_optional_target(tree, runtime.clone(), event, target, false)
            }
            Event::File(file) => match file.pos() {
                Some(pos) => {
                    let target = self.pointer_capture.or_else(|| self.hit(tree, pos));
                    self.dispatch_to_optional_target(tree, runtime.clone(), event, target, false)
                }
                None => {
                    let target = self.hovered.or_else(|| self.focused());
                    self.dispatch_to_optional_target(tree, runtime.clone(), event, target, false)
                }
            },
            Event::Window(ailloli_ui_core::event::WindowEvent::Focused { focused: false }) => {
                let pointer_changed = self.clear_pointer_state();
                let focus_changed = self.set_focus(tree, runtime.clone(), None);
                let interaction_changed = pointer_changed | focus_changed;
                RouteOutcome {
                    interaction_changed,
                    event_dispatched: focus_changed,
                }
            }
            Event::Focus(_) => RouteOutcome::default(),
            Event::Window(_) => RouteOutcome::default(),
        };
        outcome.interaction_changed |= stale_state_changed;
        outcome.interaction_changed |= input_role_changed;
        outcome.interaction_changed |= focus_request_changed;
        outcome.interaction_changed |= self.apply_pending_focus_request(tree, runtime);
        outcome.interaction_changed |= self.refresh_target_input_roles(tree);
        outcome
    }

    fn apply_pending_focus_request<A: 'static>(
        &mut self,
        tree: &ElementTree<A>,
        runtime: RuntimeHandle<A>,
    ) -> bool {
        let Some(key) = runtime.take_focus_key_request() else {
            return false;
        };
        let Ok(id) = tree.resolve_element_by_view_key(&key) else {
            return false;
        };
        let focus = focus_target_for_key(tree, id);
        self.set_focus(tree, runtime, focus)
    }

    fn route_pointer_move<A: 'static>(
        &mut self,
        tree: &ElementTree<A>,
        runtime: RuntimeHandle<A>,
        event: &Event,
        pos: Point,
    ) -> RouteOutcome {
        let hit = self.hit(tree, pos);
        let old_hover = self.hovered;
        self.hovered = hit;
        self.hovered_signature = hit.and_then(|id| target_signature(tree, id));
        self.hovered_input_role = target_input_role(tree, hit);
        let target = self.pointer_capture.or(hit);
        let mut outcome = self.dispatch_to_optional_target(
            tree,
            runtime,
            event,
            target,
            old_hover != self.hovered,
        );
        outcome.interaction_changed |= old_hover != self.hovered;
        outcome
    }

    fn route_pointer_button<A: 'static>(
        &mut self,
        tree: &ElementTree<A>,
        runtime: RuntimeHandle<A>,
        event: &Event,
        pos: Point,
        button: MouseButton,
        pressed: bool,
    ) -> RouteOutcome {
        let hit = self.hit(tree, pos);
        let mut interaction_changed = false;
        let target = if pressed {
            if button == MouseButton::Left {
                let new_focus = hit.and_then(|id| nearest_focusable(tree, id));
                interaction_changed |= self.set_focus(tree, runtime.clone(), new_focus);
            }
            self.pressed = hit;
            self.pointer_capture = hit;
            self.pressed_signature = hit.and_then(|id| target_signature(tree, id));
            self.pointer_capture_signature = self.pressed_signature.clone();
            self.pressed_input_role = target_input_role(tree, hit);
            self.pointer_capture_input_role = self.pressed_input_role;
            interaction_changed |= hit.is_some();
            hit
        } else {
            let target = self.pointer_capture.or(hit);
            if self.pressed.is_some() || self.pointer_capture.is_some() {
                interaction_changed = true;
            }
            self.pressed = None;
            self.pointer_capture = None;
            self.pressed_signature = None;
            self.pointer_capture_signature = None;
            self.pressed_input_role = InputRole::None;
            self.pointer_capture_input_role = InputRole::None;
            target
        };

        self.dispatch_to_optional_target(tree, runtime, event, target, interaction_changed)
    }

    fn dispatch_to_optional_target<A: 'static>(
        &self,
        tree: &ElementTree<A>,
        runtime: RuntimeHandle<A>,
        event: &Event,
        target: Option<ElementId>,
        interaction_changed: bool,
    ) -> RouteOutcome {
        if let Some(target) = target {
            dispatch_event_bubbling(tree, runtime, target, event);
            RouteOutcome {
                interaction_changed,
                event_dispatched: true,
            }
        } else {
            RouteOutcome {
                interaction_changed,
                event_dispatched: false,
            }
        }
    }

    fn hit<A>(&self, tree: &ElementTree<A>, pos: Point) -> Option<ElementId> {
        hit_test_target(tree, &self.hit_test, pos, None)
    }

    fn set_focus<A: 'static>(
        &mut self,
        tree: &ElementTree<A>,
        runtime: RuntimeHandle<A>,
        new_focus: Option<ElementId>,
    ) -> bool {
        let old_focus = self.focused();
        if old_focus == new_focus {
            return false;
        }

        if let Some(old) = old_focus.filter(|id| tree.get(*id).is_some()) {
            dispatch_event_to_target(
                tree,
                runtime.clone(),
                old,
                &Event::Focus(FocusEvent::new(false)),
            );
        }

        self.focus.set_focused(new_focus);
        self.focused_signature = new_focus.and_then(|id| target_signature(tree, id));
        self.focused_input_role = target_input_role(tree, new_focus);

        if let Some(new) = new_focus.filter(|id| tree.get(*id).is_some()) {
            dispatch_event_to_target(tree, runtime, new, &Event::Focus(FocusEvent::new(true)));
        }

        true
    }

    fn retain_existing_targets<A: 'static>(&mut self, tree: &ElementTree<A>) -> bool {
        let mut changed = false;
        if target_stale(tree, self.focused(), &self.focused_signature) {
            self.focus.set_focused(None);
            self.focused_signature = None;
            self.focused_input_role = InputRole::None;
            changed = true;
        }
        if target_stale(tree, self.hovered, &self.hovered_signature) {
            self.hovered = None;
            self.hovered_signature = None;
            self.hovered_input_role = InputRole::None;
            changed = true;
        }
        if target_stale(tree, self.pressed, &self.pressed_signature) {
            self.pressed = None;
            self.pressed_signature = None;
            self.pressed_input_role = InputRole::None;
            changed = true;
        }
        if target_stale(tree, self.pointer_capture, &self.pointer_capture_signature) {
            self.pointer_capture = None;
            self.pointer_capture_signature = None;
            self.pointer_capture_input_role = InputRole::None;
            changed = true;
        }
        changed
    }

    fn refresh_target_input_roles<A: 'static>(&mut self, tree: &ElementTree<A>) -> bool {
        let focused_changed =
            refresh_target_input_role(tree, self.focused(), &mut self.focused_input_role);
        let hovered_changed =
            refresh_target_input_role(tree, self.hovered, &mut self.hovered_input_role);
        let pressed_changed =
            refresh_target_input_role(tree, self.pressed, &mut self.pressed_input_role);
        let capture_changed = refresh_target_input_role(
            tree,
            self.pointer_capture,
            &mut self.pointer_capture_input_role,
        );
        focused_changed || hovered_changed || pressed_changed || capture_changed
    }
}

fn target_stale<A: 'static>(
    tree: &ElementTree<A>,
    target: Option<ElementId>,
    signature: &Option<TargetSignature>,
) -> bool {
    let Some(id) = target else {
        return false;
    };
    let Some(current) = target_signature(tree, id) else {
        return true;
    };
    signature.as_ref().is_some_and(|old| old != &current)
}

fn target_signature<A: 'static>(tree: &ElementTree<A>, id: ElementId) -> Option<TargetSignature> {
    let el = tree.get(id)?;
    let kind = match &el.kind {
        ElementKind::Empty => TargetKind::Empty,
        ElementKind::Component(_) => TargetKind::Component,
        ElementKind::Widget(widget) => TargetKind::Widget {
            debug_name: widget.debug_name(),
            focus_policy: widget.focus_policy(),
        },
    };
    Some(TargetSignature {
        key: el.key.clone(),
        kind,
    })
}

fn refresh_target_input_role<A: 'static>(
    tree: &ElementTree<A>,
    target: Option<ElementId>,
    stored: &mut InputRole,
) -> bool {
    let next = target_input_role(tree, target);
    if *stored == next {
        return false;
    }
    *stored = next;
    true
}

fn target_input_role<A: 'static>(tree: &ElementTree<A>, target: Option<ElementId>) -> InputRole {
    target
        .and_then(|id| input_role(tree, id))
        .unwrap_or(InputRole::None)
}

fn focus_policy<A: 'static>(tree: &ElementTree<A>, id: ElementId) -> Option<FocusPolicy> {
    let el = tree.get(id)?;
    match &el.kind {
        ElementKind::Widget(widget) => Some(widget.focus_policy()),
        _ => Some(FocusPolicy::NotFocusable),
    }
}

fn input_role<A: 'static>(tree: &ElementTree<A>, id: ElementId) -> Option<InputRole> {
    let el = tree.get(id)?;
    match &el.kind {
        ElementKind::Widget(widget) => Some(widget.input_role()),
        _ => Some(InputRole::None),
    }
}

fn hover_cursor_role<A: 'static>(
    tree: &ElementTree<A>,
    id: ElementId,
    pos: Option<Point>,
) -> Option<HoverCursorRole> {
    let el = tree.get(id)?;
    match &el.kind {
        ElementKind::Widget(widget) => {
            let Some(pos) = pos else {
                return Some(widget.hover_cursor_role());
            };
            let layout = el.layout.as_ref()?;
            let bounds = absolute_paint_bounds(tree, id).unwrap_or(layout.paint_bounds);
            Some(widget.hover_cursor_role_at(bounds, layout, pos))
        }
        ElementKind::Empty | ElementKind::Component(_) => Some(HoverCursorRole::Inherit),
    }
}

fn nearest_focusable<A: 'static>(tree: &ElementTree<A>, mut id: ElementId) -> Option<ElementId> {
    loop {
        if focus_policy(tree, id) == Some(FocusPolicy::Focusable) {
            return Some(id);
        }
        id = tree.parent_of(id)?;
    }
}

fn focus_target_for_key<A: 'static>(tree: &ElementTree<A>, id: ElementId) -> Option<ElementId> {
    if focus_policy(tree, id) == Some(FocusPolicy::Focusable) {
        return Some(id);
    }
    focusable_descendant(tree, id).or_else(|| nearest_focusable(tree, id))
}

fn focusable_descendant<A: 'static>(tree: &ElementTree<A>, id: ElementId) -> Option<ElementId> {
    for child in tree.children_of(id) {
        if focus_policy(tree, *child) == Some(FocusPolicy::Focusable) {
            return Some(*child);
        }
        if let Some(found) = focusable_descendant(tree, *child) {
            return Some(found);
        }
    }
    None
}
