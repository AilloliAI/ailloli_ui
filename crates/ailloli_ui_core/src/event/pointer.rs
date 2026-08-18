use crate::Point;

use super::keyboard::Modifiers;

/// Mouse or pen button identifier.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MouseButton {
    Left,
    Middle,
    Right,
    /// Additional button index from the platform.
    Other(u16),
}

/// Scroll wheel delta in lines or pixels.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum WheelDelta {
    /// Abstract line units (platform-defined).
    LineDelta { x: f32, y: f32 },
    /// Logical pixel delta.
    PixelDelta { x: f32, y: f32 },
}

/// Pointer move, button, or wheel event.
#[derive(Debug, Clone, PartialEq)]
pub enum PointerEvent {
    /// Cursor moved to `pos`.
    Moved { pos: Point, modifiers: Modifiers },
    /// Button pressed or released at `pos`.
    Button {
        pos: Point,
        button: MouseButton,
        pressed: bool,
        modifiers: Modifiers,
    },
    /// Scroll wheel at `pos`.
    Wheel {
        pos: Point,
        delta: WheelDelta,
        modifiers: Modifiers,
        /// High-resolution trackpad scroll when supported.
        precise: bool,
    },
}
