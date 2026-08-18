use ailloli_ui_core::geometry::Constraints;
#[cfg(feature = "devtools")]
use ailloli_ui_core::geometry::Size;
use ailloli_ui_core::ids::ElementId;
use ailloli_ui_core::math::Scale;
use ailloli_ui_text::TextSystem;

use super::layout_engine::LayoutEngine;
#[cfg(feature = "devtools")]
use super::layout_result::LayoutDebugInfo;
use super::layout_result::LayoutResult;

#[cfg(feature = "devtools")]
use std::collections::HashMap;

pub struct LayoutContext<'a> {
    pub scale: Scale,
    /// Shared text layout engine (optional for text-free tests).
    pub text_system: Option<&'a mut TextSystem>,
    #[cfg(feature = "devtools")]
    pub debug_layouts: HashMap<ElementId, LayoutDebugInfo>,
}

pub type LayoutCtx<'a> = LayoutContext<'a>;

impl<'a> LayoutContext<'a> {
    pub fn new(scale: Scale) -> Self {
        Self {
            scale,
            text_system: None,
            #[cfg(feature = "devtools")]
            debug_layouts: HashMap::new(),
        }
    }

    pub fn with_text_system(scale: Scale, text_system: &'a mut TextSystem) -> Self {
        Self {
            scale,
            text_system: Some(text_system),
            #[cfg(feature = "devtools")]
            debug_layouts: HashMap::new(),
        }
    }

    #[cfg(feature = "devtools")]
    pub fn record_debug_layout(
        &mut self,
        element_id: ElementId,
        constraints: Constraints,
        size: Size,
    ) -> LayoutDebugInfo {
        let entry = self
            .debug_layouts
            .entry(element_id)
            .or_insert_with(|| LayoutDebugInfo {
                constraints_in: constraints,
                constraints_final: None,
                layout_size: size,
            });
        entry.constraints_final = Some(constraints);
        entry.layout_size = size;
        entry.clone()
    }
}

pub struct LayoutChild {
    pub element_id: ElementId,
}

impl LayoutChild {
    pub fn layout<A: 'static>(
        &mut self,
        engine: &mut LayoutEngine<'_, A>,
        ctx: &mut LayoutCtx<'_>,
        constraints: Constraints,
    ) -> LayoutResult {
        engine.layout_element(ctx, self.element_id, constraints)
    }
}
