//! Headless clip-stack intersection scenario for retained UI layout.

use ailloli_ui_core::{ClipShape, Rect};
use ailloli_ui_runtime::scene::ClipStack;

#[test]
fn clip_stack_snapshot_is_intersection() {
    let mut stack = ClipStack::new();
    stack.push(ClipShape::Rect(Rect::new(0.0, 0.0, 10.0, 10.0)), false);
    stack.push(ClipShape::Rect(Rect::new(5.0, 2.0, 10.0, 4.0)), false);
    assert_eq!(
        stack.current(),
        Some(ClipShape::Rect(Rect::new(5.0, 2.0, 5.0, 4.0)))
    );
}
