use std::rc::Rc;

use ailloli_ui_core::style::{FlexItemStyle, LayoutSizeHint};
use ailloli_ui_core::{ElementId, WidgetId};

use super::{DirtyFlags, Key};
use crate::component::{ComponentNode, Widget};
#[cfg(feature = "devtools")]
use crate::layout::LayoutDebugInfo;
use crate::layout::LayoutResult;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct LayoutCacheKey {
    pub constraints: [u32; 4],
    pub scale: u32,
    pub text_metrics_revision: u64,
    pub layout_revision: u64,
    pub topology_revision: u64,
    pub virtual_viewport: Option<[u32; 5]>,
}

pub enum ElementKind<A> {
    Empty,
    Widget(Rc<dyn Widget<A>>),
    Component(Rc<dyn ComponentNode<A>>),
}

pub struct Element<A> {
    pub id: ElementId,
    pub widget_id: WidgetId,
    pub key: Option<Key>,
    pub kind: ElementKind<A>,
    pub dirty: DirtyFlags,
    pub parent: Option<ElementId>,
    pub children: Vec<ElementId>,
    pub layout: Option<LayoutResult>,
    pub(crate) layout_cache_key: Option<LayoutCacheKey>,
    pub(crate) layout_revision: u64,
    pub(crate) topology_revision: u64,
    pub(crate) layout_changed: bool,
    pub(crate) commit_dirty: bool,
    pub(crate) committed_bounds: Option<ailloli_ui_core::Rect>,
    pub flex_item: FlexItemStyle,
    pub size_hint: LayoutSizeHint,
    #[cfg(feature = "devtools")]
    pub layout_debug: Option<LayoutDebugInfo>,
}

impl<A> Clone for ElementKind<A> {
    fn clone(&self) -> Self {
        match self {
            Self::Empty => Self::Empty,
            Self::Widget(w) => Self::Widget(w.clone()),
            Self::Component(c) => Self::Component(c.clone()),
        }
    }
}

impl<A> Clone for Element<A> {
    fn clone(&self) -> Self {
        Self {
            id: self.id,
            widget_id: self.widget_id,
            key: self.key.clone(),
            kind: self.kind.clone(),
            dirty: self.dirty,
            parent: self.parent,
            children: self.children.clone(),
            layout: self.layout.clone(),
            layout_cache_key: self.layout_cache_key,
            layout_revision: self.layout_revision,
            topology_revision: self.topology_revision,
            layout_changed: self.layout_changed,
            commit_dirty: self.commit_dirty,
            committed_bounds: self.committed_bounds,
            flex_item: self.flex_item,
            size_hint: self.size_hint,
            #[cfg(feature = "devtools")]
            layout_debug: self.layout_debug.clone(),
        }
    }
}
