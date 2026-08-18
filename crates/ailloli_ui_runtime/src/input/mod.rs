//! Input routing: hit-test, focus, pointer/keyboard, and text editing helpers.

pub mod chrome;
pub mod click_action;
pub mod element_event_router;
pub mod event_ctx;
pub mod focus;
pub mod hit_test;
pub mod input_router;
pub mod text_edit;

pub use chrome::{ChromeAction, CursorStyle, ResizeEdge};
pub use click_action::{ClickAction, DeferredAction, IntoClickAction};
pub use element_event_router::{
    absolute_paint_bounds, collect_hit_rects, dispatch_event_bubbling, dispatch_event_to_target,
    hit_test_target,
};
pub use event_ctx::{EventContext, EventCtx};
pub use focus::{FocusManager, FocusPolicy, HoverCursorRole, InputRole};
pub use hit_test::HitTestEngine;
pub use input_router::{Action, InputInteraction, InputRouter, InputSnapshot, RouteOutcome};
pub use text_edit::{EditCmd, Selection};
