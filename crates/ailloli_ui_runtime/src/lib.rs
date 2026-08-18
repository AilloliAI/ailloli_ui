//! Retained-mode UI engine: element tree, layout, paint, input, and reactivity.
//!
//! Pipeline for each frame (see `Runtime::render_root` on [`app::Runtime`]):
//!
//! 1. **Reconcile** — diff declarative [`component::View`] into an [`element::ElementTree`]
//! 2. **Layout** — measure and position nodes ([`layout::LayoutEngine`], [`layout::Widget`])
//! 3. **Paint** — emit [`scene::DrawCmd`] lists into a [`scene::Scene`]
//!
//! Input is routed separately via [`input::InputRouter`] and widget [`component::Widget::event`].
//!
//! # Modules
//!
//! | Module | Role |
//! |--------|------|
//! | [`app`] | `Runtime`, `RuntimeHandle`, scheduler, state store |
//! | [`component`] | `View`, `Signal`/`Memo`/`Binding`, component builders |
//! | [`element`] | Retained element tree, keys, reconciliation |
//! | [`input`] | Hit-test, focus, pointer/keyboard routing |
//! | [`layout`] | Layout context, results, retained `LayoutNode` |
//! | [`scene`] | Draw commands, layers, clip stack, paint context |

/// Application runtime, handles, and scheduling.
pub mod app;
/// Declarative views, widgets, and reactive state.
pub mod component;
/// Retained element tree and reconciliation.
pub mod element;
/// Input routing, focus, and hit-testing.
pub mod input;
/// Layout measurement and results.
pub mod layout;
/// Scene graph and draw commands.
pub mod scene;

#[cfg(feature = "devtools")]
pub use layout::LayoutDebugInfo;
pub use layout::{LayoutCtx, Widget};
pub use scene::{
    BlendMode, DrawBorder, DrawBoxShadow, DrawCmd, DrawImage, DrawPolyline, DrawRRect, DrawRect,
    DrawRingProgress, DrawText, IsolatedEffects, Layer, PaintCtx, Painter, Scene,
};
