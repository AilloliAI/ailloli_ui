//! Native visual proof for the Winit host architecture host boundary and retained lifecycle.
//!
//! The three windows share one event loop. The recovery window receives a
//! deterministic `SurfaceError::Lost` equivalent before its first captured
//! frame, so the resulting PNG is produced by a reattached presentation.

#![cfg(feature = "test_support")]

use std::collections::HashSet;
use std::path::PathBuf;
use std::rc::Rc;
use std::sync::{Arc, Mutex};
use std::time::Duration;

use ailloli_ui::prelude::*;
use ailloli_ui::runtime::component::{component, Context};
use ailloli_ui_core::event::{
    Event, ImeEvent, Key, KeyEvent, KeyState, Modifiers, MouseButton, NamedKey, PointerEvent,
};
use ailloli_ui_core::{LogicalWindowId, Point};
use ailloli_ui_render_wgpu::CapturedFrame;
use ailloli_ui_runtime::app::{MemoryExternalUrlOpener, PresentationGeneration};
use ailloli_ui_runtime::input::{EventEnvelope, EventId, EventMeta, EventTimestamp};
use ailloli_ui_runtime::popup::{PopupId, PopupMountPolicy, PopupRole};
use ailloli_ui_winit::{
    run_winit_host, NoopHostDriver, PresentationTestFault, UiApp, WindowOptions, WinitHost,
};

/// Stable logical identity of the provider-neutral input window.
const INPUT_WINDOW: &str = "winit_host_architecture-input";
/// Stable logical identity of the retained-popup window.
const POPUP_WINDOW: &str = "winit_host_architecture-popup";
/// Stable logical identity of the surface-recovery window.
const RECOVERY_WINDOW: &str = "winit_host_architecture-recovery";
/// Text inserted before surface loss to prove retained editor preservation.
const RECOVERY_EDITOR_MARKER: &str = "/* edited before Lost */";

/// Resolves the repository-local directory used for Winit host architecture captures.
fn captures_dir() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../..")
        .join("artifacts")
        .join("captures")
}

/// Builds undecorated options with an explicit logical size in logical pixels.
fn window_options(id: &str, title: &str, width: f32, height: f32) -> WindowOptions {
    WindowOptions {
        logical_window_id: id.to_owned(),
        title: title.to_owned(),
        decorations: false,
        ..Default::default()
    }
    .with_logical_inner_size(Size::new(width, height))
}

#[derive(Clone)]
/// Bound input and editor state rendered in the input proof window.
struct InputSceneProps {
    /// Single-line input value mutated through synthetic IME events.
    input: State<String>,
    /// Editor buffer mutated through synthetic key events.
    editor: State<TextBuffer>,
}

/// Builds the focused input/editor scene plus link and button routing targets.
fn input_scene(ctx: &mut Context<()>, props: InputSceneProps) -> View<()> {
    ctx.runtime()
        .request_focus_key("winit_host_architecture-focused-input");
    let palette = Theme::default().palette();
    let input_style = TextInputStyle {
        // Keep both the caret and restored selection visible in the one-shot
        // input proof instead of sampling the off half of the blink cycle.
        caret_blink_ms: i64::MAX,
        ..TextInputStyle::default()
    };
    Container::new()
        .fill()
        .background(palette.background)
        .padding(24.0)
        .clip_children(true)
        .window_root_clip(true)
        .child(
            Column::new()
                .fill_width()
                .gap(16.0)
                .child(Text::new("Winit host architecture: provider-neutral input").size(22.0))
                .child(
                    Row::new()
                        .gap(14.0)
                        .align_items(AlignItems::Center)
                        .child(
                            Button::with_label("Action button")
                                .on_click(())
                                .into_view()
                                .key("winit_host_architecture-action-button"),
                        )
                        .child(
                            Link::with_label("Documentation")
                                .href("https://docs.ailloli.ai")
                                .into_view()
                                .key("winit_host_architecture-documentation-link"),
                        )
                        .child(
                            Link::with_label("Disabled link")
                                .href("https://example.com/disabled")
                                .disabled(true),
                        ),
                )
                .child(
                    TextInput::<()>::new()
                        .bind(props.input)
                        .input_style(input_style)
                        .fill_width()
                        .into_view()
                        .key("winit_host_architecture-focused-input"),
                )
                .child(
                    Editor::new(props.editor)
                        .height(190.0)
                        .fill_width()
                        .into_view()
                        .key("winit_host_architecture-editor"),
                ),
        )
        .into_view()
        .key("winit_host_architecture-input-scene")
}

/// Builds select, dropdown, combo-box, autocomplete, menu, and tooltip fixtures.
fn popup_scene(_ctx: &mut Context<()>, _props: ()) -> View<()> {
    let selected = State::new("Development".to_owned());
    let combo = State::new("Ailloli UI".to_owned());
    let search = State::new(String::new());
    let palette = Theme::default().palette();

    Container::new()
        .fill()
        .background(palette.background)
        .padding(24.0)
        // Intentionally omit `window_root_clip`: retained popup placement
        // must use the native host viewport, including for radius-zero apps.
        .clip_children(true)
        .child(
            Column::new()
                .fill_width()
                .gap(20.0)
                .child(
                    Text::new("Winit host architecture: retained popup portal + overlay fallback")
                        .size(22.0),
                )
                .child(
                    Row::new()
                        .gap(260.0)
                        .align_items(AlignItems::Start)
                        .child(
                            Select::<String>::new()
                                .bind(selected)
                                .option("Development".to_owned(), "Development")
                                .option("Staging".to_owned(), "Staging")
                                .option("Production".to_owned(), "Production")
                                .width(230.0)
                                .into_view()
                                .key("winit_host_architecture-select"),
                        )
                        .child(
                            Dropdown::<()>::new("Actions")
                                .item("Refresh", ())
                                .item("Duplicate", ())
                                .dropdown_item(
                                    DropdownItem::new("Unavailable")
                                        .disabled(true)
                                        .on_select(()),
                                )
                                .width(250.0)
                                .into_view()
                                .key("winit_host_architecture-dropdown"),
                        ),
                )
                .child(
                    Row::new()
                        .gap(240.0)
                        .align_items(AlignItems::Start)
                        .child(
                            ComboBox::<String>::new()
                                .bind(combo)
                                .option("Ailloli UI".to_owned(), "Ailloli UI")
                                .option("Sandbox".to_owned(), "Sandbox")
                                .option("Documentation".to_owned(), "Documentation")
                                .width(250.0)
                                .into_view()
                                .key("winit_host_architecture-combo-box"),
                        )
                        .child(
                            Autocomplete::<()>::new()
                                .bind(search)
                                .placeholder("Search components")
                                .suggestion("Button")
                                .suggestion("Link")
                                .suggestion("TextInput")
                                .suggestion("Tooltip")
                                .width(250.0)
                                .into_view()
                                .key("winit_host_architecture-autocomplete"),
                        ),
                )
                .child(Container::<()>::new().height(78.0).fill_width())
                .child(
                    Row::new()
                        .gap(460.0)
                        .align_items(AlignItems::Start)
                        .child(
                            ContextMenu::<()>::new(Button::with_label("Context menu owner"))
                                .entries(vec![
                                    ContextMenuEntry::Item(
                                        ContextMenuItem::new("Open")
                                            .shortcut("Enter")
                                            .on_select(()),
                                    ),
                                    ContextMenuEntry::Item(ContextMenuItem::new("More").submenu([
                                        ContextMenuEntry::Item(
                                            ContextMenuItem::new("Documentation").on_select(()),
                                        ),
                                        ContextMenuEntry::Item(
                                            ContextMenuItem::new("Inspect").on_select(()),
                                        ),
                                    ])),
                                    ContextMenuEntry::Separator,
                                    ContextMenuEntry::Item(
                                        ContextMenuItem::new("Unavailable").disabled(true),
                                    ),
                                ])
                                .into_view()
                                .key("winit_host_architecture-context-menu"),
                        )
                        .child(
                            Tooltip::<()>::with_label(
                                "Hover opens this Tooltip through the shared portal",
                            )
                            .placement(PopupPlacement::Bottom)
                            .open_delay(Duration::ZERO)
                            .child(Button::with_label("Hovered tooltip trigger"))
                            .into_view()
                            .key("winit_host_architecture-tooltip-trigger"),
                        ),
                ),
        )
        .into_view()
        .key("winit_host_architecture-popup-scene")
}

#[derive(Clone)]
/// Bound state expected to survive native surface detachment and reattachment.
struct RecoverySceneProps {
    /// Single-line retained value and caret state.
    input: State<String>,
    /// Multiline retained buffer, selection, and revision.
    editor: State<TextBuffer>,
}

/// Builds the recovery proof scene and requests focus for its retained input.
fn recovery_scene(ctx: &mut Context<()>, props: RecoverySceneProps) -> View<()> {
    ctx.runtime()
        .request_focus_key("winit_host_architecture-recovery-input");
    let palette = Theme::default().palette();
    Container::new()
        .fill()
        .background(palette.background)
        .padding(24.0)
        .clip_children(true)
        .window_root_clip(true)
        .child(
            Column::new()
                .fill_width()
                .gap(16.0)
                .child(Text::new("Winit host architecture: recovered native presentation").size(22.0))
                .child(Text::new(
                    "This retained tree was detached after a simulated surface loss and then rebound.",
                ))
                .child(
                    TextInput::<()>::new()
                        .bind(props.input)
                        .input_style(TextInputStyle {
                            // Keep the retained caret visible in the one-shot
                            // recovery proof instead of sampling the off half
                            // of the normal 500 ms blink cycle.
                            caret_blink_ms: i64::MAX,
                            ..TextInputStyle::default()
                        })
                        .fill_width()
                        .into_view()
                        .key("winit_host_architecture-recovery-input"),
                )
                .child(
                    Editor::new(props.editor)
                        .height(125.0)
                        .fill_width()
                        .into_view()
                        .key("winit_host_architecture-recovery-editor"),
                )
                .child(
                    ContextMenu::<()>::new(Button::with_label("Popup still operational"))
                        .entries(vec![
                            ContextMenuEntry::Item(
                                ContextMenuItem::new("Retained action").on_select(()),
                            ),
                            ContextMenuEntry::Item(
                                ContextMenuItem::new("Generation refreshed").disabled(true),
                            ),
                        ])
                        .into_view()
                        .key("winit_host_architecture-recovery-context-menu"),
                )
                .child(
                    Tooltip::<()>::with_label("Retained popup mounted after surface recovery")
                        .placement(PopupPlacement::Bottom)
                        .open_delay(Duration::ZERO)
                        .child(Button::with_label("Recovered tooltip trigger"))
                        .into_view()
                        .key("winit_host_architecture-recovery-tooltip"),
                ),
        )
        .into_view()
        .key("winit_host_architecture-recovery-scene")
}

/// Verifies frame extent, PNG data, color diversity, and scenario-specific regions.
fn assert_visual_frame(frame: &CapturedFrame, label: &str) {
    assert!(frame.width >= 700, "{label}: width={}", frame.width);
    assert!(frame.height >= 420, "{label}: height={}", frame.height);
    assert!(
        !frame.png_data.as_ref().expect("PNG bytes").is_empty(),
        "{label}: empty PNG"
    );
    let distinct = frame
        .rgba
        .chunks_exact(4)
        .step_by(24)
        .map(|pixel| [pixel[0], pixel[1], pixel[2], pixel[3]])
        .collect::<HashSet<_>>()
        .len();
    let visible = frame
        .rgba
        .chunks_exact(4)
        .filter(|pixel| pixel[3] > 180 && (pixel[0] > 90 || pixel[1] > 90 || pixel[2] > 90))
        .count();
    assert!(distinct > 18, "{label}: distinct sampled colors={distinct}");
    assert!(visible > 500, "{label}: visible pixels={visible}");

    let required_regions: &[(u32, u32, u32, u32)] = match label {
        "ui_bundle_winit_host_architecture_input_regression.png" => {
            &[(20, 66, 360, 44), (20, 118, 860, 40), (20, 166, 860, 198)]
        }
        "ui_bundle_winit_host_architecture_popup_fallback.png" => &[
            (20, 70, 240, 145),
            (20, 286, 500, 138),
            (570, 286, 285, 100),
        ],
        "ui_bundle_winit_host_architecture_surface_recovery.png" => {
            &[(20, 101, 820, 40), (20, 145, 820, 135), (20, 286, 320, 160)]
        }
        _ => &[],
    };
    for &(x, y, width, height) in required_regions {
        assert_visual_region(frame, label, x, y, width, height);
    }
}

/// Requires sampled color and bright-pixel diversity inside a clipped region.
fn assert_visual_region(
    frame: &CapturedFrame,
    label: &str,
    x: u32,
    y: u32,
    width: u32,
    height: u32,
) {
    let x_end = x.saturating_add(width).min(frame.width);
    let y_end = y.saturating_add(height).min(frame.height);
    let mut colors = HashSet::new();
    let mut bright_pixels = 0_usize;
    for sample_y in (y..y_end).step_by(3) {
        for sample_x in (x..x_end).step_by(3) {
            let offset = ((sample_y * frame.width + sample_x) * 4) as usize;
            let pixel = &frame.rgba[offset..offset + 4];
            colors.insert([pixel[0], pixel[1], pixel[2], pixel[3]]);
            bright_pixels +=
                usize::from(pixel[3] > 180 && (pixel[0] > 90 || pixel[1] > 90 || pixel[2] > 90));
        }
    }
    assert!(
        colors.len() >= 5 && bright_pixels >= 8,
        "{label}: expected rendered UI in region ({x}, {y}, {width}, {height}), colors={}, bright_pixels={bright_pixels}",
        colors.len()
    );
}

/// Writes a frame's required PNG payload beneath the Winit host architecture capture directory.
fn write_frame(name: &str, frame: &CapturedFrame) {
    let directory = captures_dir();
    std::fs::create_dir_all(&directory).expect("create Winit host architecture capture directory");
    std::fs::write(
        directory.join(name),
        frame.png_data.as_ref().expect("PNG bytes"),
    )
    .expect("write Winit host architecture capture");
}

/// Builds a pointer-move envelope with millisecond timestamp derived from `event_id`.
fn pointer_move(
    event_id: u64,
    logical_window_id: &str,
    generation: PresentationGeneration,
    position: Point,
) -> EventEnvelope {
    EventEnvelope::new(
        EventMeta::new(
            EventId::new(event_id),
            EventTimestamp::new(Duration::from_millis(event_id)),
            logical_window_id,
            generation,
        ),
        Event::Pointer(PointerEvent::Moved {
            pos: position,
            modifiers: Modifiers::default(),
        }),
    )
}

/// Builds a left-button envelope using the shared pointer-event constructor.
fn pointer_button(
    event_id: u64,
    logical_window_id: &str,
    generation: PresentationGeneration,
    position: Point,
    pressed: bool,
) -> EventEnvelope {
    pointer_button_with(
        event_id,
        logical_window_id,
        generation,
        position,
        MouseButton::Left,
        pressed,
    )
}

/// Builds a press/release envelope for an explicit mouse button and presentation generation.
fn pointer_button_with(
    event_id: u64,
    logical_window_id: &str,
    generation: PresentationGeneration,
    position: Point,
    button: MouseButton,
    pressed: bool,
) -> EventEnvelope {
    EventEnvelope::new(
        EventMeta::new(
            EventId::new(event_id),
            EventTimestamp::new(Duration::from_millis(event_id)),
            logical_window_id,
            generation,
        ),
        Event::Pointer(PointerEvent::Button {
            pos: position,
            button,
            pressed,
            modifiers: Modifiers::default(),
        }),
    )
}

/// Builds an IME commit envelope targeted at one logical window generation.
fn window_ime_commit(
    event_id: u64,
    logical_window_id: &str,
    generation: PresentationGeneration,
    text: &str,
) -> EventEnvelope {
    EventEnvelope::new(
        EventMeta::new(
            EventId::new(event_id),
            EventTimestamp::new(Duration::from_millis(event_id)),
            logical_window_id,
            generation,
        ),
        Event::Ime(ImeEvent::commit(text)),
    )
}

/// Builds a non-repeating pressed character-key envelope with explicit modifiers.
fn window_character_key(
    event_id: u64,
    logical_window_id: &str,
    generation: PresentationGeneration,
    character: &str,
    modifiers: Modifiers,
) -> EventEnvelope {
    EventEnvelope::new(
        EventMeta::new(
            EventId::new(event_id),
            EventTimestamp::new(Duration::from_millis(event_id)),
            logical_window_id,
            generation,
        ),
        Event::Keyboard(KeyEvent {
            state: KeyState::Pressed,
            key: Key::Character(character.to_owned()),
            modifiers,
            repeat: false,
            pointer_pos: None,
            text: None,
        }),
    )
}

/// Returns Control on non-macOS targets and Command on macOS.
fn primary_modifier() -> Modifiers {
    Modifiers {
        ctrl: !cfg!(target_os = "macos"),
        meta: cfg!(target_os = "macos"),
        ..Modifiers::default()
    }
}

/// Builds a non-repeating pressed named-key envelope with no modifiers.
fn window_key(
    event_id: u64,
    logical_window_id: &str,
    generation: PresentationGeneration,
    key: NamedKey,
) -> EventEnvelope {
    EventEnvelope::new(
        EventMeta::new(
            EventId::new(event_id),
            EventTimestamp::new(Duration::from_millis(event_id)),
            logical_window_id,
            generation,
        ),
        Event::Keyboard(KeyEvent {
            state: KeyState::Pressed,
            key: Key::Named(key),
            modifiers: Modifiers::default(),
            repeat: false,
            pointer_pos: None,
            text: None,
        }),
    )
}

/// Builds a named-key envelope targeted at the popup fixture window.
fn popup_key(event_id: u64, generation: PresentationGeneration, key: NamedKey) -> EventEnvelope {
    window_key(event_id, POPUP_WINDOW, generation, key)
}

/// Builds an Escape-key envelope targeted at the popup fixture window.
fn popup_escape(event_id: u64, generation: PresentationGeneration) -> EventEnvelope {
    popup_key(event_id, generation, NamedKey::Escape)
}

#[test]
#[ignore = "requires one native event loop, compositor and WGPU adapter"]
fn ui_bundle_winit_host_architecture_winit_architecture_capture() {
    let capture = CaptureHandle::new();
    capture.set_exit_after_all_captures(true);
    let capture_ids = Arc::new(Mutex::new(None));

    let input_props = InputSceneProps {
        input: State::new("Caret and focus survive provider translation".to_owned()),
        editor: State::new(TextBuffer::from_string(
            "fn retained_ui() {\n    // Mouse, touch and IME share EventEnvelope metadata.\n}\n",
        )),
    };
    let input_value = input_props.input.clone();
    let editor_value = input_props.editor.clone();
    let recovery_props = RecoverySceneProps {
        input: State::new("Retained value after surface recovery".to_owned()),
        editor: State::new(TextBuffer::from_string(
            "Recovery editor value\nSecond retained line\n",
        )),
    };
    let recovered_value = recovery_props.input.clone();
    let recovered_editor_value = recovery_props.editor.clone();
    let url_opener = MemoryExternalUrlOpener::new();

    let ui = UiApp::new()
        .capture_handle(capture.clone())
        .window(
            window_options(INPUT_WINDOW, "Winit host architecture input", 900.0, 520.0),
            component(input_props, input_scene),
        )
        .window(
            window_options(POPUP_WINDOW, "Winit host architecture popups", 980.0, 620.0),
            component((), popup_scene),
        )
        .window(
            window_options(
                RECOVERY_WINDOW,
                "Winit host architecture recovery",
                860.0,
                500.0,
            ),
            component(recovery_props, recovery_scene),
        );
    ui.runtime()
        .set_external_url_opener(Rc::new(url_opener.clone()));

    let input_id = LogicalWindowId::new(INPUT_WINDOW);
    let popup_id = LogicalWindowId::new(POPUP_WINDOW);
    let recovery_id = LogicalWindowId::new(RECOVERY_WINDOW);

    let capture_for_service = capture.clone();
    let capture_ids_for_service = capture_ids.clone();
    let input_id_for_service = input_id.clone();
    let popup_id_for_service = popup_id.clone();
    let recovery_id_for_service = recovery_id.clone();
    let mut recovery_fault_queued = false;
    let mut recovery_frame_count_before_fault = 0;
    let mut recovery_editor_text_before_fault = None;
    let mut recovery_editor_revision_before_fault = None;
    let mut warmup_frame_counts = None;
    let mut recovery_popup_stage = 0_u8;
    let mut recovery_menu_id: Option<PopupId> = None;
    let mut popup_exercise_stage = 0_u8;
    let mut exercised_popup_ids = HashSet::new();
    let mut closing_popup_id: Option<PopupId> = None;
    let mut opened_menu_id: Option<PopupId> = None;
    let mut popup_visual_stage = 0_u8;
    let mut state_injected = false;
    let input_value_for_service = input_value.clone();
    let editor_value_for_service = editor_value.clone();
    let recovered_editor_value_for_service = recovered_editor_value.clone();
    let url_opener_for_service = url_opener.clone();
    let mut host = WinitHost::new(ui, NoopHostDriver).test_service(move |ui| {
        if state_injected {
            return;
        }
        let Some(input) = ui.presentation_test_state(&input_id_for_service) else {
            return;
        };
        let Some(popup) = ui.presentation_test_state(&popup_id_for_service) else {
            return;
        };
        let Some(recovery) = ui.presentation_test_state(&recovery_id_for_service) else {
            return;
        };

        if !recovery_fault_queued {
            if recovery.generation != PresentationGeneration::new(1)
                || recovery.rendered_frame_count < 1
            {
                return;
            }
            let recovery_bounds = ui
                .presentation_test_element_bounds(
                    &recovery_id_for_service,
                    "winit_host_architecture-recovery-input",
                )
                .expect("recovery TextInput bounds before Lost fault");
            let recovery_focus_point = Point::new(
                recovery_bounds.x + recovery_bounds.w * 0.45,
                recovery_bounds.y + recovery_bounds.h * 0.5,
            );
            assert!(ui.inject_event_envelope(pointer_button(
                1,
                RECOVERY_WINDOW,
                recovery.generation,
                recovery_focus_point,
                true,
            )));
            assert!(ui.inject_event_envelope(pointer_button(
                2,
                RECOVERY_WINDOW,
                recovery.generation,
                recovery_focus_point,
                false,
            )));

            let recovery_editor_bounds = ui
                .presentation_test_element_bounds(
                    &recovery_id_for_service,
                    "winit_host_architecture-recovery-editor",
                )
                .expect("recovery Editor bounds before Lost fault");
            let recovery_editor_point = Point::new(
                recovery_editor_bounds.x + recovery_editor_bounds.w * 0.35,
                recovery_editor_bounds.y + recovery_editor_bounds.h * 0.35,
            );
            let editor_revision = recovered_editor_value_for_service.read().revision();
            for (event_id, pressed) in [(3, true), (4, false)] {
                assert!(ui.inject_event_envelope(pointer_button(
                    event_id,
                    RECOVERY_WINDOW,
                    recovery.generation,
                    recovery_editor_point,
                    pressed,
                )));
            }
            assert_eq!(
                ui.presentation_test_focus_within_key(
                    &recovery_id_for_service,
                    "winit_host_architecture-recovery-editor"
                ),
                Some(true),
                "recovery Editor must receive a real pointer focus before Lost"
            );
            assert!(ui.inject_event_envelope(window_ime_commit(
                5,
                RECOVERY_WINDOW,
                recovery.generation,
                RECOVERY_EDITOR_MARKER,
            )));
            let editor_before_fault = recovered_editor_value_for_service.read();
            assert!(
                editor_before_fault.revision() > editor_revision,
                "IME must mutate the recovery Editor before Lost"
            );
            let editor_text = editor_before_fault.as_str();
            assert!(
                editor_text.contains(RECOVERY_EDITOR_MARKER),
                "recovery Editor must contain the pre-Lost edit"
            );
            recovery_editor_revision_before_fault = Some(editor_before_fault.revision());
            recovery_editor_text_before_fault = Some(editor_text);

            // Restore the TextInput as the logical focus owner before Lost so
            // the same recovery frame proves focus retention and Editor state.
            for (event_id, pressed) in [(6, true), (7, false)] {
                assert!(ui.inject_event_envelope(pointer_button(
                    event_id,
                    RECOVERY_WINDOW,
                    recovery.generation,
                    recovery_focus_point,
                    pressed,
                )));
            }
            assert_eq!(
                ui.presentation_test_focus_within_key(
                    &recovery_id_for_service,
                    "winit_host_architecture-recovery-input"
                ),
                Some(true),
                "recovery TextInput must own focus before the Lost fault"
            );
            recovery_frame_count_before_fault = recovery.rendered_frame_count;
            assert!(
                ui.inject_presentation_fault(&recovery_id_for_service, PresentationTestFault::Lost)
            );
            recovery_fault_queued = true;
            return;
        }

        let recovered = recovery.generation.get() >= 2
            && recovery.lost_count == 1
            && recovery.recovery_count == 1
            && recovery
                .gpu_context_reuse_count
                .saturating_add(recovery.gpu_context_rebuild_count)
                == 1;

        if warmup_frame_counts.is_none() {
            if input.rendered_frame_count >= 1
                && popup.rendered_frame_count >= 1
                && recovery.rendered_frame_count > recovery_frame_count_before_fault
                && recovered
            {
                assert_eq!(
                    ui.presentation_test_focus_within_key(
                        &recovery_id_for_service,
                        "winit_host_architecture-recovery-input"
                    ),
                    Some(true),
                    "recovery must retain the pre-fault logical focus in generation 2"
                );
                let recovered_editor = recovered_editor_value_for_service.read();
                assert_eq!(
                    recovered_editor.as_str(),
                    recovery_editor_text_before_fault
                        .as_ref()
                        .expect("recovery Editor text captured before Lost")
                        .as_str(),
                    "recovery must retain the exact Editor value across generation 2"
                );
                assert_eq!(
                    recovered_editor.revision(),
                    recovery_editor_revision_before_fault
                        .expect("recovery Editor revision captured before Lost"),
                    "surface recovery must not reconstruct or rewrite the Editor buffer"
                );
                // ContextMenu publishes viewport geometry while painting. One
                // subsequent layout makes the complete menu/submenu bounds
                // available to the real input hit-test path.
                warmup_frame_counts = Some([
                    input.rendered_frame_count,
                    popup.rendered_frame_count,
                    recovery.rendered_frame_count,
                ]);
                ui.request_redraw_all();
            }
            return;
        }

        let [input_warmup, popup_warmup, recovery_warmup] =
            warmup_frame_counts.expect("warmup counts initialized above");
        if input.rendered_frame_count <= input_warmup
            || popup.rendered_frame_count <= popup_warmup
            || recovery.rendered_frame_count <= recovery_warmup
            || !recovered
        {
            return;
        }
        if recovery_popup_stage == 0 || recovery_popup_stage >= 3 {
            assert_eq!(
                ui.presentation_test_focus_within_key(
                    &recovery_id_for_service,
                    "winit_host_architecture-recovery-input"
                ),
                Some(true),
                "recovery must retain focus except while its menu focus scope is active"
            );
        }

        if recovery_popup_stage == 0 {
            let trigger = ui
                .presentation_test_element_bounds(
                    &recovery_id_for_service,
                    "winit_host_architecture-recovery-context-menu",
                )
                .expect("recovery ContextMenu trigger bounds after generation 2");
            let point = Point::new(trigger.x + trigger.w * 0.5, trigger.y + trigger.h * 0.5);
            for (event_id, pressed) in [(8, true), (9, false)] {
                assert!(ui.inject_event_envelope(pointer_button_with(
                    event_id,
                    RECOVERY_WINDOW,
                    recovery.generation,
                    point,
                    MouseButton::Right,
                    pressed,
                )));
            }
            let portal = ui.runtime().popup_portal();
            let opened = {
                let portal = portal.borrow();
                let found = portal.open_ids().rev().find(|popup_id| {
                    portal.request(*popup_id).is_some_and(|request| {
                        request
                            .owner()
                            .belongs_to(&recovery_id_for_service, recovery.generation)
                            && request.parent().is_none()
                            && request.semantics().role() == PopupRole::Menu
                            && request.mount_policy() == PopupMountPolicy::RetainedOverlay
                    })
                });
                found
            };
            recovery_menu_id =
                Some(opened.expect("right-click after recovery must open a retained ContextMenu"));
            recovery_popup_stage = 1;
            warmup_frame_counts = Some([
                input.rendered_frame_count,
                popup.rendered_frame_count,
                recovery.rendered_frame_count,
            ]);
            ui.request_redraw_all();
            return;
        }

        if recovery_popup_stage == 1 {
            let menu_id = recovery_menu_id.expect("recovery menu id stored at stage zero");
            assert!(
                ui.runtime().popup_is_open(menu_id),
                "the recovered retained menu must survive at least one rendered frame"
            );
            let point = Point::new(820.0, 440.0);
            let menu_bounds = ui
                .runtime()
                .popup_portal()
                .borrow()
                .bounds(menu_id)
                .expect("recovered retained menu has published bounds");
            assert!(
                !menu_bounds.contains(point.x, point.y),
                "dismissal probe must be outside the recovered menu: {menu_bounds:?}"
            );
            assert_eq!(
                ui.runtime().popup_portal().borrow().hit_test(
                    &recovery_id_for_service,
                    recovery.generation,
                    point,
                ),
                None,
                "portal authority must classify the dismissal probe as an outside press"
            );
            assert_eq!(
                ui.presentation_test_popup_mount_state(&recovery_id_for_service, menu_id),
                Some((true, true)),
                "the recovered menu must be mounted and focused before dismissal"
            );
            for (event_id, pressed) in [(10, true), (11, false)] {
                assert!(ui.inject_event_envelope(pointer_button(
                    event_id,
                    RECOVERY_WINDOW,
                    recovery.generation,
                    point,
                    pressed,
                )));
            }
            recovery_popup_stage = 2;
            warmup_frame_counts = Some([
                input.rendered_frame_count,
                popup.rendered_frame_count,
                recovery.rendered_frame_count,
            ]);
            ui.request_redraw_all();
            return;
        }

        if recovery_popup_stage == 2 {
            let menu_id = recovery_menu_id.expect("recovery menu id stored at stage zero");
            assert!(
                !ui.runtime().popup_is_open(menu_id),
                "outside press must dismiss the recovered retained menu by the next frame"
            );
            let recovery_input_bounds = ui
                .presentation_test_element_bounds(
                    &recovery_id_for_service,
                    "winit_host_architecture-recovery-input",
                )
                .expect("recovery TextInput bounds after menu dismissal");
            let point = Point::new(
                recovery_input_bounds.x + recovery_input_bounds.w * 0.45,
                recovery_input_bounds.y + recovery_input_bounds.h * 0.5,
            );
            for (event_id, pressed) in [(12, true), (13, false)] {
                assert!(ui.inject_event_envelope(pointer_button(
                    event_id,
                    RECOVERY_WINDOW,
                    recovery.generation,
                    point,
                    pressed,
                )));
            }
            assert_eq!(
                ui.presentation_test_focus_within_key(
                    &recovery_id_for_service,
                    "winit_host_architecture-recovery-input"
                ),
                Some(true),
                "a real post-dismiss click must refocus the pre-fault TextInput"
            );

            let tooltip = ui
                .presentation_test_element_bounds(
                    &recovery_id_for_service,
                    "winit_host_architecture-recovery-tooltip",
                )
                .expect("recovery Tooltip trigger bounds after generation 2");
            let point = Point::new(tooltip.x + tooltip.w * 0.5, tooltip.y + tooltip.h * 0.5);
            assert!(ui.inject_event_envelope(pointer_move(
                14,
                RECOVERY_WINDOW,
                recovery.generation,
                point,
            )));
            recovery_popup_stage = 3;
            warmup_frame_counts = Some([
                input.rendered_frame_count,
                popup.rendered_frame_count,
                recovery.rendered_frame_count,
            ]);
            ui.request_redraw_all();
            return;
        }
        if recovery_popup_stage == 3 {
            let retained_tooltip = {
                let portal = ui.runtime().popup_portal();
                let portal = portal.borrow();
                let found = portal.open_ids().rev().find(|popup_id| {
                    portal.request(*popup_id).is_some_and(|request| {
                        request
                            .owner()
                            .belongs_to(&recovery_id_for_service, recovery.generation)
                            && request.semantics().role() == PopupRole::Tooltip
                            && request.mount_policy() == PopupMountPolicy::RetainedOverlay
                    })
                });
                found
            };
            assert!(
                retained_tooltip.is_some(),
                "hover after recovery must mount a retained Tooltip in generation 2"
            );
            recovery_popup_stage = 4;
        }

        if let Some(closing) = closing_popup_id.take() {
            assert!(
                !ui.runtime().popup_is_open(closing),
                "Escape must close the previous retained popup before the next trigger"
            );
        }

        if popup_exercise_stage < 5 {
            let (key, button, down_id, up_id, expected_role) = match popup_exercise_stage {
                0 => (
                    "winit_host_architecture-select",
                    MouseButton::Left,
                    20,
                    21,
                    PopupRole::Listbox,
                ),
                1 => (
                    "winit_host_architecture-dropdown",
                    MouseButton::Left,
                    30,
                    31,
                    PopupRole::Menu,
                ),
                2 => (
                    "winit_host_architecture-combo-box",
                    MouseButton::Left,
                    40,
                    41,
                    PopupRole::Listbox,
                ),
                3 => (
                    "winit_host_architecture-autocomplete",
                    MouseButton::Left,
                    50,
                    51,
                    PopupRole::Listbox,
                ),
                4 => (
                    "winit_host_architecture-context-menu",
                    MouseButton::Right,
                    60,
                    61,
                    PopupRole::Menu,
                ),
                _ => unreachable!("popup exercise stage checked above"),
            };
            let trigger = ui
                .presentation_test_element_bounds(&popup_id_for_service, key)
                .unwrap_or_else(|| panic!("missing bounds for {key}"));
            let point = Point::new(trigger.x + trigger.w * 0.5, trigger.y + trigger.h * 0.5);
            assert!(ui.inject_event_envelope(pointer_button_with(
                down_id,
                POPUP_WINDOW,
                popup.generation,
                point,
                button,
                true,
            )));
            assert!(ui.inject_event_envelope(pointer_button_with(
                up_id,
                POPUP_WINDOW,
                popup.generation,
                point,
                button,
                false,
            )));

            let runtime = ui.runtime();
            let portal = runtime.popup_portal();
            let (opened, open_debug) = {
                let portal = portal.borrow();
                let opened = portal.open_ids().rev().find(|popup_id| {
                    portal.request(*popup_id).is_some_and(|request| {
                        request
                            .owner()
                            .belongs_to(&popup_id_for_service, popup.generation)
                            && request.semantics().role() == expected_role
                            && request.mount_policy() == PopupMountPolicy::RetainedOverlay
                    })
                });
                let open_debug = portal
                    .open_ids()
                    .filter_map(|popup_id| portal.request(popup_id))
                    .map(|request| format!("{request:?}"))
                    .collect::<Vec<_>>();
                (opened, open_debug)
            };
            let opened = opened.unwrap_or_else(|| {
                panic!("{key} did not open through PopupPortal; open={open_debug:#?}")
            });

            assert!(
                exercised_popup_ids.insert(opened),
                "each popup family must own a distinct retained registration"
            );
            if popup_exercise_stage == 4 {
                opened_menu_id = Some(opened);
            }

            popup_exercise_stage += 1;
            if popup_exercise_stage < 5 {
                assert!(ui.inject_event_envelope(popup_escape(up_id + 1, popup.generation,)));
                closing_popup_id = Some(opened);
            }
            warmup_frame_counts = Some([
                input.rendered_frame_count,
                popup.rendered_frame_count,
                recovery.rendered_frame_count,
            ]);
            ui.request_redraw_all();
            return;
        }

        let menu_id = opened_menu_id.expect("ContextMenu opened by right-click stage");
        assert!(
            ui.runtime().popup_is_open(menu_id),
            "ContextMenu must remain open for the visual proof"
        );
        if popup_visual_stage == 0 {
            // Navigate the real menu focus with key events and open the
            // submenu, then hover the Tooltip trigger without pressing
            // outside the menu. The retained Tooltip opens during the
            // resulting paint and is asserted on the next successful frame.
            for (event_id, key) in [
                (62, NamedKey::ArrowDown),
                (63, NamedKey::ArrowDown),
                (64, NamedKey::ArrowRight),
            ] {
                assert!(ui.inject_event_envelope(popup_key(event_id, popup.generation, key,)));
            }
            let tooltip_bounds = ui
                .presentation_test_element_bounds(
                    &popup_id_for_service,
                    "winit_host_architecture-tooltip-trigger",
                )
                .expect("Tooltip trigger bounds after warmup");
            let tooltip_center = Point::new(
                tooltip_bounds.x + tooltip_bounds.w * 0.5,
                tooltip_bounds.y + tooltip_bounds.h * 0.5,
            );
            assert!(ui.inject_event_envelope(pointer_move(
                65,
                POPUP_WINDOW,
                popup.generation,
                tooltip_center,
            )));
            popup_visual_stage = 1;
            warmup_frame_counts = Some([
                input.rendered_frame_count,
                popup.rendered_frame_count,
                recovery.rendered_frame_count,
            ]);
            ui.request_redraw_all();
            return;
        }
        let retained_submenu = {
            let portal = ui.runtime().popup_portal();
            let portal = portal.borrow();
            let found = portal.open_ids().rev().find(|popup_id| {
                portal.request(*popup_id).is_some_and(|request| {
                    request.parent() == Some(menu_id)
                        && request
                            .owner()
                            .belongs_to(&popup_id_for_service, popup.generation)
                        && request.semantics().role() == PopupRole::Menu
                        && request.mount_policy() == PopupMountPolicy::RetainedOverlay
                })
            });
            found
        };
        assert!(
            retained_submenu.is_some(),
            "ArrowRight must open ContextMenu submenu as a retained child PopupId"
        );
        let retained_tooltip = {
            let portal = ui.runtime().popup_portal();
            let portal = portal.borrow();
            let found = portal.open_ids().rev().find(|popup_id| {
                portal.request(*popup_id).is_some_and(|request| {
                    request.owner().logical_window_id() == &popup_id_for_service
                        && request.semantics().role() == PopupRole::Tooltip
                        && request.mount_policy() == PopupMountPolicy::RetainedOverlay
                })
            });
            found
        };
        assert!(
            retained_tooltip.is_some(),
            "hover must open Tooltip through the retained popup mount"
        );
        let action_bounds = ui
            .presentation_test_element_bounds(
                &input_id_for_service,
                "winit_host_architecture-action-button",
            )
            .expect("Action Button bounds after warmup");
        let action_point = Point::new(
            action_bounds.x + action_bounds.w * 0.5,
            action_bounds.y + action_bounds.h * 0.5,
        );
        for (event_id, pressed) in [(70, true), (71, false)] {
            assert!(ui.inject_event_envelope(pointer_button(
                event_id,
                INPUT_WINDOW,
                input.generation,
                action_point,
                pressed,
            )));
        }
        assert_eq!(
            ui.runtime().take_actions(),
            [()],
            "Button must dispatch exactly one action"
        );

        let link_bounds = ui
            .presentation_test_element_bounds(
                &input_id_for_service,
                "winit_host_architecture-documentation-link",
            )
            .expect("Documentation Link bounds after warmup");
        let link_point = Point::new(
            link_bounds.x + link_bounds.w * 0.5,
            link_bounds.y + link_bounds.h * 0.5,
        );
        for (event_id, pressed) in [(72, true), (73, false)] {
            assert!(ui.inject_event_envelope(pointer_button(
                event_id,
                INPUT_WINDOW,
                input.generation,
                link_point,
                pressed,
            )));
        }
        assert_eq!(
            url_opener_for_service.opened_urls(),
            ["https://docs.ailloli.ai"],
            "Link must use the injected memory opener exactly once"
        );

        let input_bounds = ui
            .presentation_test_element_bounds(
                &input_id_for_service,
                "winit_host_architecture-focused-input",
            )
            .expect("focused TextInput bounds after warmup");
        let input_focus_point = Point::new(
            input_bounds.x + input_bounds.w * 0.45,
            input_bounds.y + input_bounds.h * 0.5,
        );
        assert!(ui.inject_event_envelope(pointer_button(
            74,
            INPUT_WINDOW,
            input.generation,
            input_focus_point,
            true,
        )));
        assert!(ui.inject_event_envelope(pointer_button(
            75,
            INPUT_WINDOW,
            input.generation,
            input_focus_point,
            false,
        )));
        assert_eq!(
            ui.presentation_test_focus_within_key(
                &input_id_for_service,
                "winit_host_architecture-focused-input"
            ),
            Some(true),
            "pointer click must focus the Winit host architecture TextInput"
        );
        assert!(ui.inject_event_envelope(window_ime_commit(
            76,
            INPUT_WINDOW,
            input.generation,
            "✓",
        )));
        assert!(
            input_value_for_service.read().contains('✓'),
            "IME commit must mutate the focused TextInput before capture"
        );

        let editor_bounds = ui
            .presentation_test_element_bounds(
                &input_id_for_service,
                "winit_host_architecture-editor",
            )
            .expect("Editor bounds after warmup");
        let editor_point = Point::new(
            editor_bounds.x + editor_bounds.w * 0.35,
            editor_bounds.y + editor_bounds.h * 0.35,
        );
        let editor_revision = editor_value_for_service.read().revision();
        for (event_id, pressed) in [(77, true), (78, false)] {
            assert!(ui.inject_event_envelope(pointer_button(
                event_id,
                INPUT_WINDOW,
                input.generation,
                editor_point,
                pressed,
            )));
        }
        assert_eq!(
            ui.presentation_test_focus_within_key(
                &input_id_for_service,
                "winit_host_architecture-editor"
            ),
            Some(true),
            "pointer click must focus the Editor"
        );
        assert!(ui.inject_event_envelope(window_ime_commit(
            79,
            INPUT_WINDOW,
            input.generation,
            "/*ime*/",
        )));
        assert!(
            editor_value_for_service.read().revision() > editor_revision,
            "IME commit must edit the focused Editor before capture"
        );

        // Select a visible range with the real pointer capture path. Replacing
        // it proves that the drag produced a non-empty selection; Undo then
        // restores both the exact value and the selection snapshot used by the
        // final PNG, while keeping the caret/focus and prior IME proof.
        let input_before_selection = input_value_for_service.read();
        let selection_start = Point::new(input_bounds.x + 70.0, input_focus_point.y);
        let selection_end = Point::new(input_bounds.x + 330.0, input_focus_point.y);
        assert!(ui.inject_event_envelope(pointer_button(
            80,
            INPUT_WINDOW,
            input.generation,
            selection_start,
            true,
        )));
        assert!(ui.inject_event_envelope(pointer_move(
            81,
            INPUT_WINDOW,
            input.generation,
            selection_end,
        )));
        assert!(ui.inject_event_envelope(pointer_button(
            82,
            INPUT_WINDOW,
            input.generation,
            selection_end,
            false,
        )));
        assert!(ui.inject_event_envelope(window_ime_commit(
            83,
            INPUT_WINDOW,
            input.generation,
            "§",
        )));
        let replaced_selection = input_value_for_service.read();
        assert_ne!(
            replaced_selection, input_before_selection,
            "IME replacement must prove the pointer selection was non-empty"
        );
        assert!(
            replaced_selection.contains('§'),
            "IME replacement marker must reach the selected TextInput"
        );
        assert!(ui.inject_event_envelope(window_character_key(
            84,
            INPUT_WINDOW,
            input.generation,
            "z",
            primary_modifier(),
        )));
        assert_eq!(
            input_value_for_service.read(),
            input_before_selection,
            "Undo must restore the exact text and its non-empty selection snapshot"
        );
        // Re-run the drag after the state assertion so the captured frame is
        // guaranteed to contain a freshly painted, non-empty selection even
        // if reconciliation occurred while Undo restored the bound value.
        assert!(ui.inject_event_envelope(pointer_button(
            85,
            INPUT_WINDOW,
            input.generation,
            selection_start,
            true,
        )));
        assert!(ui.inject_event_envelope(pointer_move(
            86,
            INPUT_WINDOW,
            input.generation,
            selection_end,
        )));
        assert!(ui.inject_event_envelope(pointer_button(
            87,
            INPUT_WINDOW,
            input.generation,
            selection_end,
            false,
        )));
        assert_eq!(
            ui.presentation_test_focus_within_key(
                &input_id_for_service,
                "winit_host_architecture-focused-input"
            ),
            Some(true)
        );

        let ids = [
            capture_for_service.request_window(INPUT_WINDOW),
            capture_for_service.request_window(POPUP_WINDOW),
            capture_for_service.request_window(RECOVERY_WINDOW),
        ];
        *capture_ids_for_service
            .lock()
            .expect("capture id state lock") = Some(ids);
        ui.request_redraw_all();
        state_injected = true;
    });
    run_winit_host(&mut host).expect("run Winit host architecture native capture host");
    if let Some(error) = host.take_error() {
        panic!("Winit host architecture native capture failed: {error}");
    }

    let lifecycle = host
        .ui()
        .presentation_test_state(&recovery_id)
        .expect("recovery presentation state");
    assert!(
        lifecycle.attached,
        "recovered presentation must be attached"
    );
    assert!(lifecycle.generation.get() >= 2, "{lifecycle:?}");
    assert_eq!(lifecycle.lost_count, 1, "{lifecycle:?}");
    assert_eq!(lifecycle.recovery_count, 1, "{lifecycle:?}");
    assert_eq!(lifecycle.gpu_context_reuse_count, 1, "{lifecycle:?}");
    assert_eq!(lifecycle.gpu_context_rebuild_count, 0, "{lifecycle:?}");
    assert_eq!(
        recovered_value.read(),
        "Retained value after surface recovery",
        "the recovery TextInput value must remain retained"
    );
    assert!(
        recovered_editor_value
            .read()
            .as_str()
            .contains(RECOVERY_EDITOR_MARKER),
        "the pre-Lost Editor edit must remain after the recovered capture"
    );
    assert_eq!(
        host.ui().presentation_test_focus_within_key(
            &recovery_id,
            "winit_host_architecture-recovery-input"
        ),
        Some(true),
        "recovery focus must remain retained after the capture event loop exits"
    );
    let popup_generation = host
        .ui()
        .presentation_test_state(&popup_id)
        .expect("popup presentation state")
        .generation;
    let context_trigger = host
        .ui()
        .presentation_test_element_bounds(&popup_id, "winit_host_architecture-context-menu")
        .expect("captured ContextMenu trigger bounds");
    let context_click = Point::new(
        context_trigger.x + context_trigger.w * 0.5,
        context_trigger.y + context_trigger.h * 0.5,
    );
    let portal = host.ui().runtime().popup_portal();
    let portal = portal.borrow();
    let popup_root_menu = portal.open_ids().rev().find(|popup| {
        portal.request(*popup).is_some_and(|request| {
            request.owner().belongs_to(&popup_id, popup_generation)
                && request.parent().is_none()
                && request.semantics().role() == PopupRole::Menu
                && request.mount_policy() == PopupMountPolicy::RetainedOverlay
        })
    });
    let popup_root_menu = popup_root_menu.expect("captured retained ContextMenu root");
    let popup_request = portal
        .request(popup_root_menu)
        .expect("captured ContextMenu request");
    let anchor = popup_request
        .anchor()
        .expect("ContextMenu must publish its pointer anchor");
    let desired_size = popup_request
        .desired_size()
        .expect("ContextMenu must publish its desired size");
    let popup_bounds = portal
        .bounds(popup_root_menu)
        .expect("host-resolved ContextMenu bounds");
    assert!(
        (anchor.x - context_click.x).abs() <= 0.5
            && (anchor.y - context_click.y).abs() <= 0.5
            && anchor.w == 0.0
            && anchor.h == 0.0,
        "ContextMenu anchor must be the exact right-click point: anchor={anchor:?}, click={context_click:?}"
    );
    assert!(
        popup_bounds.w > context_trigger.w && popup_bounds.h > context_trigger.h,
        "ContextMenu must not be confined to its trigger: popup={popup_bounds:?}, trigger={context_trigger:?}"
    );
    assert!(
        (popup_bounds.x - context_click.x).abs() <= 0.5
            && (popup_bounds.y - context_click.y).abs() <= 0.5,
        "with room below/right, the ContextMenu top-left must start at the click: popup={popup_bounds:?}, click={context_click:?}"
    );
    assert!(
        (popup_bounds.w - desired_size.w).abs() <= 0.5
            && (popup_bounds.h - desired_size.h).abs() <= 0.5
            && popup_bounds.x >= 0.0
            && popup_bounds.y >= 0.0
            && popup_bounds.right() <= 980.0
            && popup_bounds.bottom() <= 620.0,
        "ContextMenu must keep its desired size and clamp only to the native viewport: popup={popup_bounds:?}, desired={desired_size:?}"
    );
    assert!(
        portal.open_ids().any(|popup| {
            portal.request(popup).is_some_and(|request| {
                request.owner().belongs_to(&popup_id, popup_generation)
                    && request.parent() == Some(popup_root_menu)
                    && request.semantics().role() == PopupRole::Menu
                    && request.mount_policy() == PopupMountPolicy::RetainedOverlay
            })
        }),
        "captured ContextMenu submenu must be a retained child PopupId"
    );
    assert!(
        portal.open_ids().any(|popup| {
            portal.request(popup).is_some_and(|request| {
                request.owner().belongs_to(&popup_id, popup_generation)
                    && request.semantics().role() == PopupRole::Tooltip
                    && request.mount_policy() == PopupMountPolicy::RetainedOverlay
            })
        }),
        "captured Tooltip must use the retained popup mount"
    );
    assert!(
        portal.open_ids().any(|popup| {
            portal.request(popup).is_some_and(|request| {
                request
                    .owner()
                    .belongs_to(&recovery_id, lifecycle.generation)
                    && request.semantics().role() == PopupRole::Tooltip
                    && request.mount_policy() == PopupMountPolicy::RetainedOverlay
            })
        }),
        "post-recovery Tooltip must remain mounted in the current generation"
    );
    assert!(
        !portal.open_ids().any(|popup| {
            portal.request(popup).is_some_and(|request| {
                request
                    .owner()
                    .belongs_to(&recovery_id, lifecycle.generation)
                    && request.semantics().role() == PopupRole::Menu
            })
        }),
        "post-recovery ContextMenu must have been dismissed before capture"
    );
    drop(portal);

    let [input_capture, popup_capture, recovery_capture] = capture_ids
        .lock()
        .expect("capture id state lock")
        .take()
        .expect("capture requests issued after warmup");
    for (id, name) in [
        (
            input_capture,
            "ui_bundle_winit_host_architecture_input_regression.png",
        ),
        (
            popup_capture,
            "ui_bundle_winit_host_architecture_popup_fallback.png",
        ),
        (
            recovery_capture,
            "ui_bundle_winit_host_architecture_surface_recovery.png",
        ),
    ] {
        let frame = capture
            .take_result(id)
            .expect("Winit host architecture capture slot")
            .expect("Winit host architecture capture result")
            .frame;
        assert_visual_frame(&frame, name);
        write_frame(name, &frame);
        eprintln!("wrote {}", captures_dir().join(name).display());
    }
}
