//! Render passes and CPU-side primitive builders for rects, text, and images.

pub mod primitives;
pub mod stroke;

pub use primitives::{
    apply_layer_scissor, make_tex_rect, make_tex_rect_scaled, now_ms, push_border_rrect_scaled,
    push_box_shadow_scaled, push_rect, push_rect_scaled, push_ring_progress_scaled, push_rrect,
    push_rrect_scaled, push_text, push_text_scaled, set_scissor_rect, set_scissor_rect_scaled,
    to_ndc,
};
pub use stroke::push_polyline_scaled;
