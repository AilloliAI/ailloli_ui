//! Consumer-visible type, event, mailbox, popup, and retained-tree contracts.
//!
//! These integration scenarios compile against the facade rather than its
//! implementation crates so missing or accidentally platform-coupled reexports
//! fail at the intended boundary.

use std::num::NonZeroUsize;

use ailloli_ui::prelude::*;
use ailloli_ui::IntoView;

/// Compile-time assertion that a public cross-thread type is `Send + Sync`.
fn assert_send_sync<T: Send + Sync>() {}

#[test]
fn mailbox_and_popup_contracts_are_available_through_the_facade() {
    assert_send_sync::<RuntimeSender<u32>>();
    let (sender, inbox) = RuntimeInbox::channel(NonZeroUsize::new(8).unwrap());
    sender.dispatch(7_u32).unwrap();
    assert_eq!(sender.stats().current_depth, 1);
    drop(inbox);

    let tooltip: View<()> = Tooltip::with_label("Provider-neutral tooltip")
        .alignment(PopupAlignment::Center)
        .child(Button::with_label("Trigger"))
        .into_view();
    let context_menu: View<()> = ContextMenu::new(Button::with_label("Menu"))
        .entries(vec![ContextMenuEntry::Item(
            ContextMenuItem::new("Open").on_select(()),
        )])
        .into_view();

    let _ = (tooltip, context_menu);
}

#[test]
fn provider_neutral_event_types_are_constructible_without_winit() {
    use ailloli_ui::core::event::{
        ActivationKind, Event, Modifiers, PointerEvent, PointerId, PointerSample, PointerSource,
    };
    use ailloli_ui::core::{LogicalWindowId, Point};
    use ailloli_ui::runtime::app::PresentationGeneration;
    use ailloli_ui::runtime::input::{EventEnvelope, EventId, EventMeta, EventTimestamp};

    let position = Point::new(12.0, 18.0);
    let sample = PointerSample::new(PointerId::new(3), PointerSource::Touch, position)
        .unwrap()
        .with_primary(false)
        .with_pressure(0.5)
        .unwrap()
        .with_activation(ActivationKind::Normal);
    let meta = EventMeta::new(
        EventId::new(9),
        EventTimestamp::new(std::time::Duration::from_millis(4)),
        LogicalWindowId::new("main"),
        PresentationGeneration::new(2),
    )
    .with_pointer(sample);
    let envelope = EventEnvelope::new(
        meta,
        Event::Pointer(PointerEvent::Moved {
            pos: position,
            modifiers: Modifiers::default(),
        }),
    );

    assert_eq!(envelope.meta().logical_window_id().as_str(), "main");
    assert_eq!(envelope.meta().pointer().unwrap().id(), PointerId::new(3));
    assert_eq!(envelope.pointer_is_primary(), Some(false));
}

#[test]
fn provider_event_matches_keep_forward_compatible_fallbacks() {
    use ailloli_ui::core::event::{Event, WindowEvent};

    /// Classifies provider-neutral event variants while retaining a future fallback.
    fn event_family(event: &Event) -> &'static str {
        match event {
            Event::Window(window) => window_family(window),
            Event::Pointer(_) => "pointer",
            Event::Keyboard(_) => "keyboard",
            Event::Ime(_) => "ime",
            Event::Focus(_) => "focus",
            Event::File(_) => "file",
            _ => "future-event",
        }
    }

    /// Classifies window-event variants while retaining a future fallback.
    fn window_family(event: &WindowEvent) -> &'static str {
        match event {
            WindowEvent::Resized { .. } => "resized",
            WindowEvent::ScaleFactorChanged { .. } => "scale-factor",
            WindowEvent::Focused { .. } => "focused",
            WindowEvent::CloseRequested => "close-requested",
            WindowEvent::RedrawRequested => "redraw-requested",
            _ => "future-window-event",
        }
    }

    assert_eq!(
        event_family(&Event::Window(WindowEvent::CloseRequested)),
        "close-requested"
    );
}

#[test]
fn targeted_invalidation_and_retained_tree_are_available_through_the_facade() {
    assert_eq!(
        Invalidation::Paint.merge(Invalidation::Layout),
        Invalidation::Layout,
    );

    let model = TreeModelHandle::new(TreeModel::<u64>::new());
    model
        .apply(TreeMutation::Insert {
            parent: None,
            index: 0,
            item: TreeItem::branch(1, "root"),
        })
        .unwrap();
    model
        .apply(TreeMutation::SetExpanded {
            id: 1,
            expanded: true,
        })
        .unwrap();
    assert_eq!(model.read(TreeModel::visible_len), 1);
}
