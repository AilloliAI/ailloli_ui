//! Interactive resize without OS decorations: logical border + `Window::drag_resize_window`.

use ailloli_ui_runtime::input::ResizeEdge;
use winit::window::ResizeDirection;

/// Pointer-sensitive frame thickness (logical coords; hit test is DPR-neutral).
pub const CLIENT_RESIZE_BORDER_LOGICAL_PX: f32 = 5.0;

/// Maps runtime resize edges to winit [`ResizeDirection`].
pub fn resize_edge_to_winit(edge: ResizeEdge) -> ResizeDirection {
    match edge {
        ResizeEdge::N => ResizeDirection::North,
        ResizeEdge::S => ResizeDirection::South,
        ResizeEdge::E => ResizeDirection::East,
        ResizeEdge::W => ResizeDirection::West,
        ResizeEdge::NE => ResizeDirection::NorthEast,
        ResizeEdge::NW => ResizeDirection::NorthWest,
        ResizeEdge::SE => ResizeDirection::SouthEast,
        ResizeEdge::SW => ResizeDirection::SouthWest,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn maps_cardinals_to_winit() {
        assert!(matches!(
            resize_edge_to_winit(ResizeEdge::N),
            ResizeDirection::North
        ));
        assert!(matches!(
            resize_edge_to_winit(ResizeEdge::SE),
            ResizeDirection::SouthEast
        ));
    }
}
