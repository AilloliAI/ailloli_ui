#[cfg(feature = "devtools")]
use ailloli_ui_core::geometry::Size;
use ailloli_ui_core::geometry::{Constraints, Rect};
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
    virtual_viewport: Option<VirtualViewport>,
    #[cfg(feature = "devtools")]
    pub debug_layouts: HashMap<ElementId, LayoutDebugInfo>,
}

/// Viewport made available to widgets whose content is larger than the
/// visible scroll container. Coordinates are local to the content.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct VirtualViewport {
    pub rect: Rect,
    pub overscan: f32,
}

impl VirtualViewport {
    pub const fn new(rect: Rect, overscan: f32) -> Self {
        Self { rect, overscan }
    }
}

pub type LayoutCtx<'a> = LayoutContext<'a>;

impl<'a> LayoutContext<'a> {
    pub fn new(scale: Scale) -> Self {
        Self {
            scale,
            text_system: None,
            virtual_viewport: None,
            #[cfg(feature = "devtools")]
            debug_layouts: HashMap::new(),
        }
    }

    pub fn with_text_system(scale: Scale, text_system: &'a mut TextSystem) -> Self {
        Self {
            scale,
            text_system: Some(text_system),
            virtual_viewport: None,
            #[cfg(feature = "devtools")]
            debug_layouts: HashMap::new(),
        }
    }

    pub const fn virtual_viewport(&self) -> Option<VirtualViewport> {
        self.virtual_viewport
    }

    /// Replaces the current content viewport and returns the previous value.
    /// Layout containers use this to scope the hint to their child traversal.
    pub fn replace_virtual_viewport(
        &mut self,
        viewport: Option<VirtualViewport>,
    ) -> Option<VirtualViewport> {
        std::mem::replace(&mut self.virtual_viewport, viewport)
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
