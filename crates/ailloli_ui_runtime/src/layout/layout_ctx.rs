//! Mutable layout context shared while traversing retained nodes.

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

/// Mutable services and scoped hints available during one layout traversal.
///
/// Geometry is expressed in logical pixels. The context borrows its optional
/// text system exclusively, so it is local to the current thread and layout
/// pass rather than a shareable global service.
///
/// # Examples
///
/// ```
/// use ailloli_ui_core::Scale;
/// use ailloli_ui_runtime::layout::LayoutContext;
///
/// let ctx = LayoutContext::new(Scale::new(2.0));
/// assert_eq!(ctx.scale.dpr, 2.0);
/// assert!(ctx.text_system.is_none());
/// ```
pub struct LayoutContext<'a> {
    /// Device-pixel ratio used for snapping and layout-cache identity.
    pub scale: Scale,
    /// Shared text layout engine (optional for text-free tests).
    pub text_system: Option<&'a mut TextSystem>,
    /// Optional content-local viewport propagated by a virtualizing ancestor.
    virtual_viewport: Option<VirtualViewport>,
    #[cfg(feature = "devtools")]
    /// Latest developer-tooling layout record for each element in this context.
    pub debug_layouts: HashMap<ElementId, LayoutDebugInfo>,
}

/// Viewport made available to widgets whose content is larger than the
/// visible scroll container. Coordinates are local to the content.
///
/// Values use logical pixels and are stored verbatim. In particular, negative
/// or non-finite overscan is not clamped here; consumers must define how such
/// values affect virtualization.
///
/// # Examples
///
/// ```
/// use ailloli_ui_core::Rect;
/// use ailloli_ui_runtime::layout::VirtualViewport;
///
/// let viewport = VirtualViewport::new(Rect::new(0.0, 120.0, 640.0, 480.0), 32.0);
/// assert_eq!(viewport.overscan, 32.0);
/// ```
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct VirtualViewport {
    /// Visible rectangle in content-local logical pixels.
    pub rect: Rect,
    /// Extra logical pixels requested around every visible edge.
    pub overscan: f32,
}

/// Provides the operations defined for VirtualViewport.
impl VirtualViewport {
    /// Creates a viewport without validating its rectangle or overscan.
    ///
    /// # Examples
    ///
    /// ```
    /// use ailloli_ui_core::Rect;
    /// use ailloli_ui_runtime::layout::VirtualViewport;
    ///
    /// let viewport = VirtualViewport::new(Rect::new(10.0, 20.0, 30.0, 40.0), 8.0);
    /// assert_eq!(viewport.rect.y, 20.0);
    /// ```
    pub const fn new(rect: Rect, overscan: f32) -> Self {
        Self { rect, overscan }
    }
}

/// Short alias for [`LayoutContext`].
///
/// # Examples
///
/// ```
/// use ailloli_ui_core::Scale;
/// use ailloli_ui_runtime::layout::LayoutCtx;
///
/// let ctx = LayoutCtx::new(Scale::new(1.0));
/// assert_eq!(ctx.scale.dpr, 1.0);
/// ```
pub type LayoutCtx<'a> = LayoutContext<'a>;

/// Provides the operations defined for `LayoutContext<'a>`.
impl<'a> LayoutContext<'a> {
    /// Creates a context without a text system or virtual viewport.
    ///
    /// With `devtools`, the debug-record map also starts empty.
    ///
    /// # Examples
    ///
    /// ```
    /// use ailloli_ui_core::Scale;
    /// use ailloli_ui_runtime::layout::LayoutContext;
    ///
    /// let ctx = LayoutContext::new(Scale::new(1.25));
    /// assert!(ctx.virtual_viewport().is_none());
    /// ```
    pub fn new(scale: Scale) -> Self {
        Self {
            scale,
            text_system: None,
            virtual_viewport: None,
            #[cfg(feature = "devtools")]
            debug_layouts: HashMap::new(),
        }
    }

    /// Creates a context borrowing `text_system` for this layout pass.
    ///
    /// The virtual viewport starts as `None`; with `devtools`, debug records
    /// start empty.
    ///
    /// # Examples
    ///
    /// ```
    /// use ailloli_ui_core::Scale;
    /// use ailloli_ui_runtime::layout::LayoutContext;
    /// use ailloli_ui_text::TextSystem;
    ///
    /// let mut text = TextSystem::new();
    /// let ctx = LayoutContext::with_text_system(Scale::new(1.0), &mut text);
    /// assert!(ctx.text_system.is_some());
    /// ```
    pub fn with_text_system(scale: Scale, text_system: &'a mut TextSystem) -> Self {
        Self {
            scale,
            text_system: Some(text_system),
            virtual_viewport: None,
            #[cfg(feature = "devtools")]
            debug_layouts: HashMap::new(),
        }
    }

    /// Returns the currently scoped content viewport.
    ///
    /// `None` means widgets should lay out without a virtualization hint.
    ///
    /// # Examples
    ///
    /// ```
    /// use ailloli_ui_core::{Rect, Scale};
    /// use ailloli_ui_runtime::layout::{LayoutCtx, VirtualViewport};
    ///
    /// let mut ctx = LayoutCtx::new(Scale::new(1.0));
    /// ctx.replace_virtual_viewport(Some(VirtualViewport::new(
    ///     Rect::new(0.0, 50.0, 200.0, 100.0), 12.0,
    /// )));
    /// assert_eq!(ctx.virtual_viewport().unwrap().rect.y, 50.0);
    /// ```
    pub const fn virtual_viewport(&self) -> Option<VirtualViewport> {
        self.virtual_viewport
    }

    /// Replaces the current content viewport and returns the previous value.
    /// Layout containers use this to scope the hint to their child traversal.
    /// Passing `None` clears the hint. The caller should restore the returned
    /// value after laying out a scoped child.
    ///
    /// # Examples
    ///
    /// ```
    /// use ailloli_ui_core::{Rect, Scale};
    /// use ailloli_ui_runtime::layout::{LayoutCtx, VirtualViewport};
    ///
    /// let mut ctx = LayoutCtx::new(Scale::new(1.0));
    /// let viewport = VirtualViewport::new(Rect::new(0.0, 0.0, 80.0, 60.0), 4.0);
    /// assert!(ctx.replace_virtual_viewport(Some(viewport)).is_none());
    /// assert_eq!(ctx.replace_virtual_viewport(None), Some(viewport));
    /// ```
    pub fn replace_virtual_viewport(
        &mut self,
        viewport: Option<VirtualViewport>,
    ) -> Option<VirtualViewport> {
        std::mem::replace(&mut self.virtual_viewport, viewport)
    }

    #[cfg(feature = "devtools")]
    /// Records the latest layout and returns the stored debug snapshot.
    ///
    /// The first call for an element fixes `constraints_in`. Every call sets
    /// `constraints_final` to `Some(constraints)` and replaces `layout_size`.
    /// The map grows by unique element ID for the lifetime of this context.
    ///
    /// # Examples
    ///
    /// ```
    /// use ailloli_ui_core::{Constraints, ElementId, Scale, Size};
    /// use ailloli_ui_runtime::layout::LayoutCtx;
    ///
    /// let mut ctx = LayoutCtx::new(Scale::new(1.0));
    /// let info = ctx.record_debug_layout(
    ///     ElementId(7), Constraints::tight(20.0, 10.0), Size::new(20.0, 10.0),
    /// );
    /// assert_eq!(info.layout_size, Size::new(20.0, 10.0));
    /// assert!(info.constraints_final.is_some());
    /// ```
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

/// Handle used by widgets to lay out one retained direct child.
///
/// # Examples
///
/// ```
/// use ailloli_ui_core::ElementId;
/// use ailloli_ui_runtime::layout::LayoutChild;
///
/// let child = LayoutChild { element_id: ElementId(3) };
/// assert_eq!(child.element_id, ElementId(3));
/// ```
pub struct LayoutChild {
    /// Retained-tree identity delegated to [`LayoutEngine`].
    pub element_id: ElementId,
}

/// Provides the operations defined for LayoutChild.
impl LayoutChild {
    /// Lays out this child with the supplied logical-pixel constraints.
    ///
    /// A stale or unknown `element_id` produces [`LayoutResult::zero`] and no
    /// diagnostic cache event. Other behavior, including widget panics, is
    /// delegated to [`LayoutEngine::layout_element`].
    ///
    /// # Examples
    ///
    /// ```
    /// use ailloli_ui_core::{Constraints, ElementId, Scale, Size};
    /// use ailloli_ui_runtime::element::ElementTree;
    /// use ailloli_ui_runtime::layout::{LayoutChild, LayoutCtx, LayoutEngine};
    ///
    /// let mut tree = ElementTree::<()>::new();
    /// let mut engine = LayoutEngine::new(&mut tree);
    /// let mut ctx = LayoutCtx::new(Scale::new(1.0));
    /// let mut child = LayoutChild { element_id: ElementId(99) };
    /// assert_eq!(
    ///     child.layout(&mut engine, &mut ctx, Constraints::tight(10.0, 20.0)).size,
    ///     Size::default(),
    /// );
    /// ```
    pub fn layout<A: 'static>(
        &mut self,
        engine: &mut LayoutEngine<'_, A>,
        ctx: &mut LayoutCtx<'_>,
        constraints: Constraints,
    ) -> LayoutResult {
        engine.layout_element(ctx, self.element_id, constraints)
    }
}
