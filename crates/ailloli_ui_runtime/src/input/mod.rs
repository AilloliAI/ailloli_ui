//! Input routing: hit-test, focus, pointer/keyboard, and text editing helpers.

/// Chrome implementation details.
pub mod chrome;
/// Click action implementation details.
pub mod click_action;
/// Element event router implementation details.
pub mod element_event_router;
/// Event ctx implementation details.
pub mod event_ctx;
/// Event envelope implementation details.
pub mod event_envelope;
/// Focus implementation details.
pub mod focus;
/// Hit test implementation details.
pub mod hit_test;
/// Input router implementation details.
pub mod input_router;
/// Text edit implementation details.
pub mod text_edit;

pub use chrome::{ChromeAction, CursorStyle, ResizeEdge};
pub use click_action::{ClickAction, DeferredAction, IntoClickAction};
pub use element_event_router::{
    absolute_paint_bounds, collect_hit_rects, dispatch_event_bubbling,
    dispatch_event_envelope_bubbling, dispatch_event_envelope_to_target, dispatch_event_to_target,
    hit_test_overlay_target, hit_test_target,
};
pub use event_ctx::{EventContext, EventCtx};
pub use event_envelope::{EventEnvelope, EventId, EventMeta, EventTimestamp};
pub use focus::{ActivationPolicy, FocusManager, FocusPolicy, HoverCursorRole, InputRole};
pub use hit_test::HitTestEngine;
pub use input_router::{Action, InputInteraction, InputRouter, InputSnapshot, RouteOutcome};
pub use text_edit::{EditCmd, Selection};
