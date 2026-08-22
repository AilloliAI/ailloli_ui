//! Editor input geometry and selection primitives.

/// Caret rectangle helpers.
pub mod caret;
/// Neutral edit outcomes.
pub mod edit_action;
/// Text and code-zone hit testing.
pub mod hit_test;
/// IME preedit display-buffer projection.
pub mod ime;
/// Wrap-aware scrolling helpers.
pub mod scroll;
/// Selection ranges and paint geometry.
pub mod selection;

pub use edit_action::EditorInputOutcome;
pub use hit_test::{EditorHitTest, EditorHitZone, EditorZoneHitTest};
pub use selection::SelectionGranularity;
