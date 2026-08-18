//! Client window chrome: default title bar and hit-test regions for drag/resize.

pub mod ailloli_ui_titlebar;
pub mod resize_region;
pub mod window_affordance;
pub mod window_drag_region;

pub use ailloli_ui_titlebar::{
    ailloli_ui_default_titlebar, ailloli_ui_default_titlebar_with_icon, application_icon,
};
pub use resize_region::hit_resize_frame;
pub use window_affordance::{
    classify_window_affordance_hit, WindowAffordanceDragPhase, WindowAffordanceEvent,
    WindowAffordanceFrame, WindowAffordanceKind, WindowAffordanceState, WindowAffordanceStyle,
};
pub use window_drag_region::hit_window_drag_region;
