//! Editor and code-editor adapter state, paint, input, IME, and scrolling scenarios.

use std::time::Duration;

use ailloli_ui_core::event::{
    Event, ImeEvent, ImePreedit, Key, KeyEvent, KeyState, Modifiers, MouseButton, NamedKey,
    PointerEvent, WheelDelta,
};
use ailloli_ui_core::geometry::Constraints;
use ailloli_ui_core::math::Scale;
use ailloli_ui_core::{ClipShape, Point, Rect};
use ailloli_ui_runtime::app::{PresentationGeneration, Runtime, RuntimeHandle};
use ailloli_ui_runtime::component::{IntoView, State, View, ViewKind};
use ailloli_ui_runtime::element::ElementKind;
use ailloli_ui_runtime::input::{
    EventEnvelope, EventId, EventMeta, EventTimestamp, InputRouter, InputSnapshot,
};
use ailloli_ui_runtime::layout::LayoutArtifact;
use ailloli_ui_runtime::scene::PaintCtx;
use ailloli_ui_runtime::DrawCmd;
use ailloli_ui_text::{TextBuffer, TextSystem};
use ailloli_ui_widgets::editor::{
    CodeEditor, CodeEditorFeatureFlags, CodeFileSummary, CodeTheme, Diagnostic, DiagnosticSeverity,
    DiagnosticSource, Document, DocumentId, DocumentVersion, Editor, EditorLanguage, EditorPane,
    EditorScrollbarStyle, EditorWrapMode, FoldRegion, SearchQuery,
};
use ailloli_ui_widgets::layout::{Column, Container, Row, ScrollView, SplitPane};
use ailloli_ui_widgets::text::Text;

#[test]
fn editor_accepts_public_state_binding() {
    let buffer = State::new(TextBuffer::from_string("hello"));
    let view: View<()> = Editor::new(buffer).into_view();

    assert!(matches!(view.kind, ViewKind::Component(_)));
}

#[test]
fn code_editor_accepts_public_document_binding() {
    let document = State::new(Document::new(
        DocumentId(1),
        TextBuffer::from_string("fn main() {}\n"),
    ));
    let view: View<()> = CodeEditor::new(document).into_view();

    assert!(matches!(view.kind, ViewKind::Component(_)));
}

#[test]
fn editor_disabled_ends_preedit() {
    let buffer = State::new(TextBuffer::from_string("stable"));
    let runtime: RuntimeHandle<()> = RuntimeHandle::new();
    let mut app = Runtime::new(runtime.clone());
    app.reconcile(Editor::new(buffer.clone()).into_view());

    assert_disabled_ends_editor_preedit(&mut app, runtime, Point::new(20.0, 20.0));
    assert_eq!(buffer.read().as_str(), "stable");
}

#[test]
fn code_editor_disabled_ends_preedit() {
    let document = State::new(Document::new(
        DocumentId(2),
        TextBuffer::from_string("stable"),
    ));
    let runtime: RuntimeHandle<()> = RuntimeHandle::new();
    let mut app = Runtime::new(runtime.clone());
    app.reconcile(CodeEditor::new(document.clone()).into_view());

    assert_disabled_ends_editor_preedit(&mut app, runtime, Point::new(80.0, 20.0));
    assert_eq!(document.read().buffer.as_str(), "stable");
}

fn assert_disabled_ends_editor_preedit(
    app: &mut Runtime<()>,
    runtime: RuntimeHandle<()>,
    focus_pos: Point,
) {
    let mut text_system = TextSystem::new();
    app.layout(
        Constraints::tight(320.0, 160.0),
        Scale::new(1.0),
        &mut text_system,
    );
    let mut router = InputRouter::default();
    router.route_event(
        &app.tree,
        runtime.clone(),
        &Event::Pointer(PointerEvent::Button {
            pos: focus_pos,
            button: MouseButton::Left,
            pressed: true,
            modifiers: Modifiers::default(),
        }),
    );
    router.route_event(
        &app.tree,
        runtime.clone(),
        &Event::Ime(ImeEvent::Preedit {
            preedit: ImePreedit::new("PREEDIT-MARKER"),
            pos: None,
        }),
    );
    app.layout(
        Constraints::tight(320.0, 160.0),
        Scale::new(1.0),
        &mut text_system,
    );
    let during_preedit = app.paint_with_input(&mut text_system, router.snapshot(), 0);
    assert!(scene_contains_text_fragment(
        &during_preedit,
        "PREEDIT-MARKER"
    ));

    router.route_event(&app.tree, runtime, &Event::Ime(ImeEvent::Disabled));
    app.layout(
        Constraints::tight(320.0, 160.0),
        Scale::new(1.0),
        &mut text_system,
    );
    let after_disabled = app.paint_with_input(&mut text_system, router.snapshot(), 0);
    assert!(!scene_contains_text_fragment(
        &after_disabled,
        "PREEDIT-MARKER"
    ));
}

fn scene_contains_text_fragment(scene: &ailloli_ui_runtime::Scene, needle: &str) -> bool {
    scene.layers.iter().any(|layer| {
        layer
            .cmds
            .iter()
            .any(|cmd| matches!(cmd, DrawCmd::Text(text) if text.layout.text().contains(needle)))
    })
}

#[test]
fn code_editor_builder_accepts_controlled_options_without_backend_types() {
    let document = State::new(Document::new(
        DocumentId(30),
        TextBuffer::from_string("fn main() {}\n"),
    ));
    let summary = CodeFileSummary {
        document_id: DocumentId(30),
        path: Some("src/main.rs".into()),
        language: EditorLanguage::Rust,
        version: DocumentVersion(0),
        symbols: Vec::new(),
        edges: Vec::new(),
    };
    let features = CodeEditorFeatureFlags {
        semantic_backends: false,
        ..CodeEditorFeatureFlags::default()
    };

    let view: View<()> = CodeEditor::new(document)
        .features(features)
        .search_query(SearchQuery::new("main"))
        .diagnostics(Vec::new())
        .fold_regions(Vec::new())
        .symbol_summary(summary)
        .into_view();

    assert!(matches!(view.kind, ViewKind::Component(_)));
}

#[test]
fn editor_layout_does_not_store_text_runs_artifact() {
    let buffer = State::new(TextBuffer::from_string("hello"));
    let runtime: RuntimeHandle<()> = RuntimeHandle::new();
    let mut app = Runtime::new(runtime.clone());
    app.reconcile(Editor::new(buffer).into_view());
    let mut text_system = TextSystem::new();

    app.layout(
        Constraints::tight(200.0, 120.0),
        Scale::new(1.0),
        &mut text_system,
    );

    let editor_layouts: Vec<_> = app
        .tree
        .iter_elements()
        .filter_map(|(_, el)| match &el.kind {
            ElementKind::Widget(widget) if widget.debug_name() == "Editor" => el.layout.as_ref(),
            _ => None,
        })
        .collect();

    assert_eq!(editor_layouts.len(), 1);
    assert!(editor_layouts[0].artifact.is_none());
}

#[test]
fn editor_layout_does_not_prepare_hit_test_frame() {
    let buffer = State::new(TextBuffer::from_string("hello"));
    let runtime: RuntimeHandle<()> = RuntimeHandle::new();
    let mut app = Runtime::new(runtime.clone());
    app.reconcile(Editor::new(buffer.clone()).into_view());
    let mut text_system = TextSystem::new();

    app.layout(
        Constraints::tight(240.0, 120.0),
        Scale::new(1.0),
        &mut text_system,
    );

    let mut router = InputRouter::default();
    router.route_event(
        &app.tree,
        runtime.clone(),
        &Event::Pointer(PointerEvent::Button {
            pos: Point::new(5.0, 5.0),
            button: MouseButton::Left,
            pressed: true,
            modifiers: Modifiers::default(),
        }),
    );
    router.route_event(
        &app.tree,
        runtime,
        &Event::Keyboard(KeyEvent {
            state: KeyState::Pressed,
            key: Key::Character("a".into()),
            modifiers: Modifiers::default(),
            repeat: false,
            pointer_pos: Some(Point::new(5.0, 5.0)),
            text: Some("a".into()),
        }),
    );

    assert_eq!(buffer.read().as_str(), "helloa");
}

#[test]
fn editor_paint_clips_text_layer_without_clipping_background() {
    let buffer = State::new(TextBuffer::from_string(
        "aaaaaaaaaaaa bbbbbbbbbbbb".to_string(),
    ));
    let runtime: RuntimeHandle<()> = RuntimeHandle::new();
    let mut app = Runtime::new(runtime);
    app.reconcile(Editor::new(buffer).into_view());
    let mut text_system = TextSystem::new();
    app.layout(
        Constraints::tight(110.0, 120.0),
        Scale::new(1.0),
        &mut text_system,
    );

    let scene = app.paint(&mut text_system);

    // Editor paints its background on the parent layer (no extra clip), then
    // wraps text/selection/caret in `with_clip(viewport.content_rect)` so the
    // scene has 2 non-empty layers.
    assert_eq!(scene.layers.len(), 2);

    let bg_layer = &scene.layers[0];
    assert!(bg_layer.clip.is_empty(), "background must not be clipped");
    assert!(
        bg_layer
            .cmds
            .iter()
            .any(|cmd| matches!(cmd, DrawCmd::Rect(_))),
        "background layer must contain the editor background rect"
    );

    let content_layer = &scene.layers[1];
    assert_eq!(content_layer.clip.entries().len(), 1);
    assert!(matches!(
        content_layer.clip.entries()[0].shape,
        ClipShape::Rect(_)
    ));
    assert!(!content_layer.clip.entries()[0].is_window_root);
    assert!(
        content_layer
            .cmds
            .iter()
            .any(|cmd| matches!(cmd, DrawCmd::Text(_))),
        "content layer must contain the editor text"
    );
}

#[test]
fn editor_adapter_blinks_caret_from_frame_time() {
    let buffer = State::new(TextBuffer::from_string("hello"));
    let runtime: RuntimeHandle<()> = RuntimeHandle::new();
    let mut app = Runtime::new(runtime);
    app.reconcile(Editor::new(buffer).into_view());
    let mut text_system = TextSystem::new();
    app.layout(
        Constraints::tight(220.0, 100.0),
        Scale::new(1.0),
        &mut text_system,
    );

    let focused = app
        .tree
        .iter_elements()
        .find_map(|(id, el)| match &el.kind {
            ElementKind::Widget(widget) if widget.debug_name() == "Editor" => Some(id),
            _ => None,
        })
        .expect("editor element");
    let input = InputSnapshot {
        focused: Some(focused),
        hovered: None,
        pressed: None,
    };

    let on_scene = app.paint_with_input(&mut text_system, input, 0);
    let off_scene = app.paint_with_input(&mut text_system, input, 500);

    assert!(
        on_scene
            .layers
            .iter()
            .flat_map(|layer| &layer.cmds)
            .any(|cmd| matches!(cmd, DrawCmd::RRect(_))),
        "caret should be painted during visible blink phase"
    );
    assert!(
        !off_scene
            .layers
            .iter()
            .flat_map(|layer| &layer.cmds)
            .any(|cmd| matches!(cmd, DrawCmd::RRect(_))),
        "caret should be omitted during hidden blink phase"
    );
}

#[test]
fn code_editor_paints_gutter_outside_text_clip() {
    let document = State::new(Document::new(
        DocumentId(2),
        TextBuffer::from_string("fn main() {}\nlet value = 1;\n"),
    ));
    let runtime: RuntimeHandle<()> = RuntimeHandle::new();
    let mut app = Runtime::new(runtime);
    app.reconcile(CodeEditor::new(document).into_view());
    let mut text_system = TextSystem::new();
    app.layout(
        Constraints::tight(240.0, 120.0),
        Scale::new(1.0),
        &mut text_system,
    );

    let scene = app.paint(&mut text_system);

    assert_eq!(scene.layers.len(), 3);
    let bg_layer = &scene.layers[0];
    assert!(bg_layer.clip.is_empty());
    assert!(
        !bg_layer
            .cmds
            .iter()
            .any(|cmd| matches!(cmd, DrawCmd::Text(_))),
        "background layer must not contain unclipped gutter text"
    );

    let gutter_layer = scene
        .layers
        .iter()
        .find(|layer| {
            layer.cmds.iter().any(|cmd| match cmd {
                DrawCmd::Text(text) => text.layout.text() == "1",
                _ => false,
            })
        })
        .expect("gutter line number layer");
    assert_eq!(gutter_layer.clip.entries().len(), 1);
    let gutter_clip = match gutter_layer.clip.entries()[0].shape {
        ClipShape::Rect(rect) => rect,
        _ => panic!("expected gutter rect clip"),
    };
    assert!(gutter_clip.x < 58.0);
    assert!(gutter_clip.w <= 48.0);
    let text_items: Vec<_> = gutter_layer
        .cmds
        .iter()
        .filter_map(|cmd| match cmd {
            DrawCmd::Text(text) => Some(text.layout.text().to_string()),
            _ => None,
        })
        .collect();
    assert!(
        text_items.iter().any(|text| text == "1"),
        "line number must be outside text clip: {text_items:?}"
    );

    let content_layer = scene
        .layers
        .iter()
        .find(|layer| {
            layer.cmds.iter().any(|cmd| match cmd {
                DrawCmd::Text(text) => text.layout.text() == "fn main() {}",
                _ => false,
            })
        })
        .expect("code editor content layer");
    assert_eq!(content_layer.clip.entries().len(), 1);
    let text_clip = match content_layer.clip.entries()[0].shape {
        ClipShape::Rect(rect) => rect,
        _ => panic!("expected text rect clip"),
    };
    assert!(text_clip.x > gutter_clip.x + gutter_clip.w - 0.5);
    assert!(
        content_layer.cmds.iter().any(|cmd| match cmd {
            DrawCmd::Text(text) => text.layout.text() == "fn main() {}",
            _ => false,
        }),
        "code text must be inside text clip"
    );
}

#[test]
fn code_editor_fractional_scroll_clips_partially_visible_line_numbers_to_gutter() {
    let text: String = (0..40)
        .map(|idx| format!("let value_{idx:02} = {idx};\n"))
        .collect();
    let document = State::new(Document::new(DocumentId(45), TextBuffer::from_string(text)));
    let runtime: RuntimeHandle<()> = RuntimeHandle::new();
    let mut app = Runtime::new(runtime);
    app.reconcile(
        CodeEditor::new(document)
            .initial_scroll(0.0, 10.0 * 18.0 + 15.0)
            .width(260.0)
            .height(130.0)
            .into_view(),
    );
    let mut text_system = TextSystem::new();
    app.layout(
        Constraints::tight(260.0, 130.0),
        Scale::new(1.0),
        &mut text_system,
    );

    let scene = app.paint(&mut text_system);
    let gutter_layer = scene
        .layers
        .iter()
        .find(|layer| {
            layer.cmds.iter().any(|cmd| match cmd {
                DrawCmd::Text(text) => text.layout.text() == "11",
                _ => false,
            })
        })
        .expect("gutter line number layer");
    let gutter_clip = match gutter_layer.clip.entries()[0].shape {
        ClipShape::Rect(rect) => rect,
        _ => panic!("expected gutter rect clip"),
    };
    let first_line_number_baseline = gutter_layer
        .cmds
        .iter()
        .filter_map(|cmd| match cmd {
            DrawCmd::Text(text) => Some(text.pos[1]),
            _ => None,
        })
        .fold(f32::INFINITY, f32::min);

    assert!(
        first_line_number_baseline < gutter_clip.y,
        "test must cover a partially visible number: baseline={first_line_number_baseline}, clip={gutter_clip:?}"
    );
    assert_eq!(gutter_layer.clip.entries().len(), 1);
    assert!(
        gutter_layer
            .cmds
            .iter()
            .all(|cmd| !matches!(cmd, DrawCmd::Text(text) if text.pos[0] >= gutter_clip.right())),
        "line numbers must remain within gutter x range"
    );
}

#[test]
fn code_editor_detects_rust_language_from_document_path() {
    let document = State::new(
        Document::new(
            DocumentId(8),
            TextBuffer::from_string("fn main() {\n    let value = 42;\n}\n"),
        )
        .with_path("src/path_detected.rs"),
    );
    let runtime: RuntimeHandle<()> = RuntimeHandle::new();
    let mut app = Runtime::new(runtime);
    app.reconcile(
        CodeEditor::new(document)
            .width(320.0)
            .height(140.0)
            .into_view(),
    );
    let mut text_system = TextSystem::new();
    app.layout(
        Constraints::tight(320.0, 140.0),
        Scale::new(1.0),
        &mut text_system,
    );

    let scene = app.paint(&mut text_system);

    assert!(
        scene
            .layers
            .iter()
            .flat_map(|layer| &layer.cmds)
            .any(|cmd| {
                matches!(
                    cmd,
                    DrawCmd::Text(text)
                    if text.layout.text() == "fn main() {"
                        && text.layout.glyphs().iter().any(|glyph| glyph.color.is_some())
                )
            }),
        "CodeEditor should syntax-highlight Rust when only .rs path is provided"
    );
}

#[test]
fn code_editor_recomputes_syntax_when_bound_document_changes() {
    let document = State::new(
        Document::new(
            DocumentId(801),
            TextBuffer::from_string("// stale comment token\n"),
        )
        .with_path("src/first.rs"),
    );
    let runtime: RuntimeHandle<()> = RuntimeHandle::new();
    let mut app = Runtime::new(runtime);
    app.reconcile(
        CodeEditor::new(document.clone())
            .width(360.0)
            .height(120.0)
            .into_view(),
    );
    let mut text_system = TextSystem::new();
    app.layout(
        Constraints::tight(360.0, 120.0),
        Scale::new(1.0),
        &mut text_system,
    );
    let theme = CodeTheme::default();
    let first_scene = app.paint(&mut text_system);
    assert!(
        first_scene
            .layers
            .iter()
            .flat_map(|layer| &layer.cmds)
            .any(|cmd| {
                matches!(
                    cmd,
                    DrawCmd::Text(text)
                    if text.layout.text() == "// stale comment token"
                        && text
                            .layout
                            .glyphs()
                            .iter()
                            .any(|glyph| glyph.color == Some(theme.syntax_comment))
                )
            }),
        "initial document should populate the syntax cache with comment tokens"
    );

    document.set(
        Document::new(
            DocumentId(802),
            TextBuffer::from_string("fn fresh_main() {}\n"),
        )
        .with_path("src/second.rs"),
    );
    app.reconcile(
        CodeEditor::new(document)
            .width(360.0)
            .height(120.0)
            .into_view(),
    );
    app.layout(
        Constraints::tight(360.0, 120.0),
        Scale::new(1.0),
        &mut text_system,
    );
    let second_scene = app.paint(&mut text_system);

    assert!(
        second_scene
            .layers
            .iter()
            .flat_map(|layer| &layer.cmds)
            .any(|cmd| {
                matches!(
                    cmd,
                    DrawCmd::Text(text)
                    if text.layout.text() == "fn fresh_main() {}"
                        && text
                            .layout
                            .glyphs()
                            .iter()
                            .any(|glyph| glyph.color == Some(theme.syntax_keyword))
                        && text
                            .layout
                            .glyphs()
                            .iter()
                            .any(|glyph| glyph.color == Some(theme.syntax_function))
                        && !text
                            .layout
                            .glyphs()
                            .iter()
                            .any(|glyph| glyph.color == Some(theme.syntax_comment))
                )
            }),
        "second document must be highlighted from its own tokens, not the previous document cache"
    );
}

#[test]
fn code_editor_language_builder_overrides_document_path_detection() {
    let document = State::new(
        Document::new(DocumentId(9), TextBuffer::from_string("fn main() {}\n"))
            .with_path("src/path_detected.rs"),
    );
    let runtime: RuntimeHandle<()> = RuntimeHandle::new();
    let mut app = Runtime::new(runtime);
    app.reconcile(
        CodeEditor::new(document)
            .language(EditorLanguage::PlainText)
            .width(320.0)
            .height(120.0)
            .into_view(),
    );
    let mut text_system = TextSystem::new();
    app.layout(
        Constraints::tight(320.0, 120.0),
        Scale::new(1.0),
        &mut text_system,
    );

    let scene = app.paint(&mut text_system);

    assert!(
        scene
            .layers
            .iter()
            .flat_map(|layer| &layer.cmds)
            .any(|cmd| {
                matches!(
                    cmd,
                    DrawCmd::Text(text)
                    if text.layout.text() == "fn main() {}"
                        && text.layout.glyphs().iter().all(|glyph| glyph.color.is_none())
                )
            }),
        "explicit .language(PlainText) should override .rs path detection"
    );
}

#[test]
fn code_editor_adapter_blinks_caret_without_hiding_text_or_gutter() {
    let document = State::new(Document::new(
        DocumentId(7),
        TextBuffer::from_string("fn main() {\n    let value = 1;\n}\n"),
    ));
    let runtime: RuntimeHandle<()> = RuntimeHandle::new();
    let mut app = Runtime::new(runtime);
    app.reconcile(
        CodeEditor::new(document)
            .width(320.0)
            .height(140.0)
            .into_view(),
    );
    let mut text_system = TextSystem::new();
    app.layout(
        Constraints::tight(320.0, 140.0),
        Scale::new(1.0),
        &mut text_system,
    );

    let focused = app
        .tree
        .iter_elements()
        .find_map(|(id, el)| match &el.kind {
            ElementKind::Widget(widget) if widget.debug_name() == "CodeEditor" => Some(id),
            _ => None,
        })
        .expect("code editor element");
    let input = InputSnapshot {
        focused: Some(focused),
        hovered: None,
        pressed: None,
    };

    let on_scene = app.paint_with_input(&mut text_system, input, 0);
    let off_scene = app.paint_with_input(&mut text_system, input, 500);

    assert!(
        on_scene
            .layers
            .iter()
            .flat_map(|layer| &layer.cmds)
            .any(|cmd| matches!(cmd, DrawCmd::RRect(_))),
        "caret should be painted during visible blink phase"
    );
    assert!(
        !off_scene
            .layers
            .iter()
            .flat_map(|layer| &layer.cmds)
            .any(|cmd| matches!(cmd, DrawCmd::RRect(_))),
        "caret should be omitted during hidden blink phase"
    );
    assert!(
        off_scene
            .layers
            .iter()
            .flat_map(|layer| &layer.cmds)
            .any(|cmd| {
                matches!(cmd, DrawCmd::Text(text) if text.layout.text() == "fn main() {")
            }),
        "code text should remain visible while caret is hidden"
    );
    assert!(
        off_scene
            .layers
            .iter()
            .flat_map(|layer| &layer.cmds)
            .any(|cmd| { matches!(cmd, DrawCmd::Text(text) if text.layout.text() == "1") }),
        "gutter line numbers should remain visible while caret is hidden"
    );
}

#[test]
fn code_editor_adapter_paints_active_line_before_text_when_focused() {
    let document = State::new(Document::new(
        DocumentId(6),
        TextBuffer::from_string("fn main() {\n    const test: &str = \"test\";\n}\n"),
    ));
    let runtime: RuntimeHandle<()> = RuntimeHandle::new();
    let mut app = Runtime::new(runtime.clone());
    app.reconcile(
        CodeEditor::new(document)
            .width(340.0)
            .height(140.0)
            .into_view(),
    );
    let mut text_system = TextSystem::new();
    app.layout(
        Constraints::tight(340.0, 140.0),
        Scale::new(1.0),
        &mut text_system,
    );

    let focused = app
        .tree
        .iter_elements()
        .find_map(|(id, el)| match &el.kind {
            ElementKind::Widget(widget) if widget.debug_name() == "CodeEditor" => Some(id),
            _ => None,
        })
        .expect("code editor element");
    let scene = app.paint_with_input(
        &mut text_system,
        InputSnapshot {
            focused: Some(focused),
            hovered: None,
            pressed: None,
        },
        0,
    );
    let content_layer = scene
        .layers
        .iter()
        .find(|layer| {
            layer.cmds.iter().any(|cmd| match cmd {
                DrawCmd::Text(text) => text.layout.text() == "fn main() {",
                _ => false,
            })
        })
        .expect("code editor content layer");
    let active_fill_index = content_layer
        .cmds
        .iter()
        .position(|cmd| {
            matches!(
                cmd,
                DrawCmd::Rect(rect)
                    if rect.rect.x > 40.0 && rect.rect.w > 250.0 && rect.rect.h > 10.0
            )
        })
        .expect("active line fill");
    let active_border_index = content_layer
        .cmds
        .iter()
        .position(|cmd| {
            matches!(
                cmd,
                DrawCmd::Border(border)
                    if border.rect.x > 40.0 && border.rect.w > 250.0 && border.rect.h > 8.0
            )
        })
        .expect("active line ring");
    let active_fill = match &content_layer.cmds[active_fill_index] {
        DrawCmd::Rect(rect) => rect.rect,
        _ => unreachable!(),
    };
    let active_border = match &content_layer.cmds[active_border_index] {
        DrawCmd::Border(border) => border.rect,
        _ => unreachable!(),
    };
    let text_index = content_layer
        .cmds
        .iter()
        .position(|cmd| match cmd {
            DrawCmd::Text(text) => text.layout.text() == "fn main() {",
            _ => false,
        })
        .expect("text");

    let clip_rect = match content_layer.clip.entries()[0].shape {
        ClipShape::Rect(rect) => rect,
        _ => panic!("expected text rect clip"),
    };

    assert!(active_fill_index < text_index);
    assert!(active_border_index < text_index);
    assert!(active_fill.x >= clip_rect.x);
    assert!(active_fill.x + active_fill.w <= clip_rect.x + clip_rect.w);
    assert!(active_border.x >= clip_rect.x);
    assert!(active_border.y >= clip_rect.y);
    assert!(active_border.x + active_border.w <= clip_rect.x + clip_rect.w);
    assert!(active_border.y + active_border.h <= clip_rect.y + clip_rect.h);
    assert!(active_border.y <= active_fill.y);
    assert!(active_border.y + active_border.h > active_fill.y + active_fill.h);
    assert!(active_border.h > active_fill.h);
}

#[test]
fn code_editor_search_query_paints_active_match_from_widget_props() {
    let document = State::new(Document::new(
        DocumentId(9),
        TextBuffer::from_string("value other value\n"),
    ));
    let runtime: RuntimeHandle<()> = RuntimeHandle::new();
    let mut app = Runtime::new(runtime);
    app.reconcile(
        CodeEditor::new(document)
            .search_query(SearchQuery::new("value"))
            .search_active_match(1)
            .width(280.0)
            .height(120.0)
            .into_view(),
    );
    let mut text_system = TextSystem::new();
    app.layout(
        Constraints::tight(280.0, 120.0),
        Scale::new(1.0),
        &mut text_system,
    );

    let scene = app.paint(&mut text_system);
    let content_layer = scene
        .layers
        .iter()
        .find(|layer| {
            layer.cmds.iter().any(|cmd| match cmd {
                DrawCmd::Text(text) => text.layout.text() == "value other value",
                _ => false,
            })
        })
        .expect("code editor content layer");
    let search_rects: Vec<_> = content_layer
        .cmds
        .iter()
        .filter_map(|cmd| match cmd {
            DrawCmd::Rect(rect)
                if rect.rect.x >= 58.0 && rect.rect.h > 8.0 && rect.color.a > 0.25 =>
            {
                Some(*rect)
            }
            _ => None,
        })
        .collect();

    assert!(
        search_rects.len() >= 2,
        "expected search and active search rects, got {search_rects:?}"
    );
    assert!(
        search_rects.iter().any(|rect| rect.color.a > 0.4),
        "active search match must use distinct stronger color: {search_rects:?}"
    );
    let text_index = content_layer
        .cmds
        .iter()
        .position(
            |cmd| matches!(cmd, DrawCmd::Text(text) if text.layout.text() == "value other value"),
        )
        .expect("text");
    let first_search_index = content_layer
        .cmds
        .iter()
        .position(|cmd| {
            matches!(cmd, DrawCmd::Rect(rect) if rect.rect.x >= 58.0 && rect.rect.h > 8.0 && rect.color.a > 0.25)
        })
        .expect("search rect");
    assert!(first_search_index < text_index);
}

#[test]
fn code_editor_diagnostics_props_paint_gutter_marker_and_active_underline() {
    let document = State::new(Document::new(
        DocumentId(10),
        TextBuffer::from_string("let value = 1;\nlet other = value;\n"),
    ));
    let runtime: RuntimeHandle<()> = RuntimeHandle::new();
    let mut app = Runtime::new(runtime);
    app.reconcile(
        CodeEditor::new(document)
            .diagnostics(vec![
                Diagnostic::new(4..9, DiagnosticSeverity::Error, "bad value"),
                Diagnostic::new(19..24, DiagnosticSeverity::Warning, "other"),
            ])
            .active_diagnostic(0)
            .width(280.0)
            .height(120.0)
            .into_view(),
    );
    let mut text_system = TextSystem::new();
    app.layout(
        Constraints::tight(280.0, 120.0),
        Scale::new(1.0),
        &mut text_system,
    );

    let scene = app.paint(&mut text_system);
    let has_gutter_marker = scene.layers.iter().any(|layer| {
        layer.cmds.iter().any(|cmd| {
            matches!(
                cmd,
                DrawCmd::Rect(rect)
                    if rect.rect.x < 20.0 && rect.rect.w <= 6.0 && rect.color.r > 0.5
            )
        })
    });
    let content_layer = scene
        .layers
        .iter()
        .find(|layer| {
            layer.cmds.iter().any(|cmd| match cmd {
                DrawCmd::Text(text) => text.layout.text() == "let value = 1;",
                _ => false,
            })
        })
        .expect("code editor content layer");
    let underline_count = content_layer
        .cmds
        .iter()
        .filter(
            |cmd| matches!(cmd, DrawCmd::Rect(rect) if rect.rect.x >= 58.0 && rect.rect.h <= 2.0),
        )
        .count();
    let active_bg_count = content_layer
        .cmds
        .iter()
        .filter(|cmd| {
            matches!(
                cmd,
                DrawCmd::Rect(rect)
                    if rect.rect.x >= 58.0 && rect.rect.h > 8.0 && rect.color.a > 0.1
            )
        })
        .count();

    assert!(
        has_gutter_marker,
        "diagnostic marker must be painted in gutter"
    );
    assert!(underline_count >= 2, "expected diagnostic underlines");
    assert!(
        active_bg_count >= 1,
        "expected active diagnostic background"
    );
}

#[test]
fn code_editor_accepts_lsp_sourced_diagnostics_without_adapter_backend_logic() {
    let document_data = Document::new(
        DocumentId(12),
        TextBuffer::from_string("fn main() {\n    value();\n}\n"),
    )
    .with_language(EditorLanguage::Rust);
    let version = document_data.version;
    let document = State::new(document_data);
    let diagnostic = Diagnostic::lsp(
        16..21,
        DiagnosticSeverity::Error,
        "unresolved function from LSP",
        version,
    );
    assert_eq!(diagnostic.source, DiagnosticSource::Lsp);
    let runtime: RuntimeHandle<()> = RuntimeHandle::new();
    let mut app = Runtime::new(runtime);
    app.reconcile(
        CodeEditor::new(document)
            .diagnostics(vec![diagnostic])
            .active_diagnostic(0)
            .width(300.0)
            .height(140.0)
            .into_view(),
    );
    let mut text_system = TextSystem::new();
    app.layout(
        Constraints::tight(300.0, 140.0),
        Scale::new(1.0),
        &mut text_system,
    );
    let scene = app.paint(&mut text_system);

    assert!(scene
        .layers
        .iter()
        .any(|layer| { layer.cmds.iter().any(|cmd| matches!(cmd, DrawCmd::Rect(_))) }));
}

#[test]
fn code_editor_fold_marker_toggles_collapsed_region_without_widget_folding_logic() {
    let document = State::new(Document::new(
        DocumentId(11),
        TextBuffer::from_string("fn main() {\nlet a = 1;\nlet b = 2;\n}\nfn next() {}\n"),
    ));
    let runtime: RuntimeHandle<()> = RuntimeHandle::new();
    let mut app = Runtime::new(runtime.clone());
    app.reconcile(
        CodeEditor::new(document)
            .fold_regions(vec![FoldRegion::new(0, 2)])
            .width(320.0)
            .height(160.0)
            .into_view(),
    );
    let mut text_system = TextSystem::new();
    app.layout(
        Constraints::tight(320.0, 160.0),
        Scale::new(1.0),
        &mut text_system,
    );
    let scene = app.paint(&mut text_system);
    let marker_rect = scene
        .layers
        .iter()
        .flat_map(|layer| &layer.cmds)
        .find_map(|cmd| match cmd {
            DrawCmd::Polyline(polyline) => {
                let min_x = polyline
                    .points
                    .iter()
                    .map(|point| point.x)
                    .fold(f32::INFINITY, f32::min);
                let max_x = polyline
                    .points
                    .iter()
                    .map(|point| point.x)
                    .fold(f32::NEG_INFINITY, f32::max);
                let min_y = polyline
                    .points
                    .iter()
                    .map(|point| point.y)
                    .fold(f32::INFINITY, f32::min);
                let max_y = polyline
                    .points
                    .iter()
                    .map(|point| point.y)
                    .fold(f32::NEG_INFINITY, f32::max);
                let rect = Rect::new(min_x, min_y, max_x - min_x, max_y - min_y);
                (rect.x < 48.0 && rect.w <= 14.0 && rect.h <= 14.0).then_some(rect)
            }
            _ => None,
        })
        .expect("fold marker chevron should be painted in the gutter");
    assert!(
        scene.layers.iter().any(|layer| {
            layer.cmds.iter().any(|cmd| match cmd {
                DrawCmd::Rect(rect) => {
                    rect.rect.x < 48.0 && rect.rect.w <= 2.0 && rect.rect.h > 1.0
                }
                _ => false,
            })
        }),
        "fold guide should be painted in the gutter"
    );

    let mut router = InputRouter::default();
    router.route_event(
        &app.tree,
        runtime,
        &Event::Pointer(PointerEvent::Button {
            pos: Point::new(
                marker_rect.x + marker_rect.w * 0.5,
                marker_rect.y + marker_rect.h * 0.5,
            ),
            button: MouseButton::Left,
            pressed: true,
            modifiers: Modifiers::default(),
        }),
    );
    let scene = app.paint(&mut text_system);
    let texts: Vec<_> = scene
        .layers
        .iter()
        .flat_map(|layer| &layer.cmds)
        .filter_map(|cmd| match cmd {
            DrawCmd::Text(text) => Some(text.layout.text().to_string()),
            _ => None,
        })
        .collect();

    assert!(!texts.iter().any(|text| text == "let a = 1;"));
    assert!(texts.iter().any(|text| text.contains("2 lines folded")));
    assert!(texts.iter().any(|text| text == "fn next() {}"));
}

#[test]
fn code_editor_keyboard_edit_synchronizes_public_document() {
    let document = State::new(
        Document::new(DocumentId(3), TextBuffer::new())
            .with_language(ailloli_ui_widgets::editor::EditorLanguage::Rust)
            .with_path("src/main.rs"),
    );
    let runtime: RuntimeHandle<()> = RuntimeHandle::new();
    let mut app = Runtime::new(runtime.clone());
    app.reconcile(CodeEditor::new(document.clone()).into_view());
    let mut text_system = TextSystem::new();
    app.layout(
        Constraints::tight(240.0, 120.0),
        Scale::new(1.0),
        &mut text_system,
    );
    let _ = app.paint(&mut text_system);

    let mut router = InputRouter::default();
    router.route_event(
        &app.tree,
        runtime.clone(),
        &Event::Pointer(PointerEvent::Button {
            pos: Point::new(70.0, 20.0),
            button: MouseButton::Left,
            pressed: true,
            modifiers: Modifiers::default(),
        }),
    );
    router.route_event(
        &app.tree,
        runtime,
        &Event::Keyboard(KeyEvent {
            state: KeyState::Pressed,
            key: Key::Character("a".into()),
            modifiers: Modifiers::default(),
            repeat: false,
            pointer_pos: Some(Point::new(70.0, 20.0)),
            text: Some("a".into()),
        }),
    );

    let updated = document.read();
    assert_eq!(updated.buffer.as_str(), "a");
    assert_eq!(updated.id, DocumentId(3));
    assert_eq!(
        updated.language,
        ailloli_ui_widgets::editor::EditorLanguage::Rust
    );
    assert_eq!(
        updated.path.as_ref().and_then(|path| path.to_str()),
        Some("src/main.rs")
    );
    assert!(updated.dirty);
    assert_eq!(updated.version.0, 1);
}

#[derive(Debug, Clone, PartialEq, Eq)]
enum CodeEditorCallbackTestAction {
    DocumentChanged(String),
}

#[test]
fn code_editor_document_change_callback_emits_updated_document() {
    let document =
        State::new(Document::new(DocumentId(301), TextBuffer::new()).with_path("src/main.rs"));
    let runtime: RuntimeHandle<CodeEditorCallbackTestAction> = RuntimeHandle::new();
    let mut app = Runtime::new(runtime.clone());
    app.reconcile(
        CodeEditor::new(document)
            .on_document_change(|document| {
                CodeEditorCallbackTestAction::DocumentChanged(document.buffer.as_str().to_string())
            })
            .into_view(),
    );
    let mut text_system = TextSystem::new();
    app.layout(
        Constraints::tight(240.0, 120.0),
        Scale::new(1.0),
        &mut text_system,
    );
    let _ = app.paint(&mut text_system);

    let mut router = InputRouter::default();
    router.route_event(
        &app.tree,
        runtime.clone(),
        &Event::Pointer(PointerEvent::Button {
            pos: Point::new(70.0, 20.0),
            button: MouseButton::Left,
            pressed: true,
            modifiers: Modifiers::default(),
        }),
    );
    router.route_event(
        &app.tree,
        runtime.clone(),
        &Event::Keyboard(KeyEvent {
            state: KeyState::Pressed,
            key: Key::Character("x".into()),
            modifiers: Modifiers::default(),
            repeat: false,
            pointer_pos: Some(Point::new(70.0, 20.0)),
            text: Some("x".into()),
        }),
    );

    assert_eq!(
        runtime.take_actions(),
        vec![CodeEditorCallbackTestAction::DocumentChanged("x".into())]
    );
}

#[test]
fn code_editor_wheel_updates_nowrap_scroll_x_without_moving_gutter() {
    let long_line = "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa";
    let document = State::new(Document::new(
        DocumentId(4),
        TextBuffer::from_string(format!("{long_line}\n")),
    ));
    let runtime: RuntimeHandle<()> = RuntimeHandle::new();
    let mut app = Runtime::new(runtime.clone());
    app.reconcile(
        CodeEditor::new(document)
            .width(180.0)
            .height(90.0)
            .into_view(),
    );
    let mut text_system = TextSystem::new();
    app.layout(
        Constraints::tight(180.0, 90.0),
        Scale::new(1.0),
        &mut text_system,
    );
    let initial_scene = app.paint(&mut text_system);
    let initial_line_number_x = first_text_x(&initial_scene, "1");

    let mut router = InputRouter::default();
    router.route_event(
        &app.tree,
        runtime,
        &Event::Pointer(PointerEvent::Wheel {
            pos: Point::new(70.0, 20.0),
            delta: WheelDelta::PixelDelta { x: -40.0, y: 0.0 },
            modifiers: Modifiers::default(),
            precise: true,
        }),
    );
    let scrolled_scene = app.paint(&mut text_system);
    let scrolled_line_number_x = first_text_x(&scrolled_scene, "1");
    let code_text_x = first_text_x(&scrolled_scene, long_line);

    assert_eq!(scrolled_line_number_x, initial_line_number_x);
    assert!(
        code_text_x < initial_line_number_x,
        "code text must move left while gutter stays fixed: text_x={code_text_x}, gutter_x={initial_line_number_x}"
    );
}

#[test]
fn code_editor_adapter_paints_scrollbars_for_overflowing_content() {
    let text: String = (0..80)
        .map(|idx| format!("let value_{idx} = \"{}\";\n", "x".repeat(180)))
        .collect();
    let document = State::new(Document::new(DocumentId(41), TextBuffer::from_string(text)));
    let runtime: RuntimeHandle<()> = RuntimeHandle::new();
    let mut app = Runtime::new(runtime);
    app.reconcile(
        CodeEditor::new(document)
            .width(260.0)
            .height(130.0)
            .into_view(),
    );
    let mut text_system = TextSystem::new();
    app.layout(
        Constraints::tight(260.0, 130.0),
        Scale::new(1.0),
        &mut text_system,
    );

    let scene = app.paint(&mut text_system);
    let rrects = rrects_in_text_layer(&scene);

    assert!(
        rrects.len() >= 4,
        "expected vertical + horizontal scrollbar track/thumb rrects, got {rrects:?}"
    );
    assert!(
        rrects.iter().any(|rrect| rrect.rect.h > rrect.rect.w),
        "expected vertical scrollbar rrect"
    );
    assert!(
        rrects.iter().any(|rrect| rrect.rect.w > rrect.rect.h),
        "expected horizontal scrollbar rrect"
    );
    assert!(
        rrects.iter().all(|rrect| rrect.rect.x >= 48.0),
        "scrollbars must be inside text rect, not the gutter: {rrects:?}"
    );
}

#[test]
fn code_editor_scrollbars_can_be_disabled_and_styled() {
    let text: String = (0..80)
        .map(|idx| format!("let value_{idx} = {idx};\n"))
        .collect();
    let document = State::new(Document::new(
        DocumentId(42),
        TextBuffer::from_string(text.clone()),
    ));
    let runtime: RuntimeHandle<()> = RuntimeHandle::new();
    let mut app = Runtime::new(runtime);
    app.reconcile(
        CodeEditor::new(document)
            .scrollbars(false)
            .width(260.0)
            .height(130.0)
            .into_view(),
    );
    let mut text_system = TextSystem::new();
    app.layout(
        Constraints::tight(260.0, 130.0),
        Scale::new(1.0),
        &mut text_system,
    );
    let disabled_scene = app.paint(&mut text_system);
    assert!(rrects_in_text_layer(&disabled_scene).is_empty());

    let track = ailloli_ui_core::Color::rgb(12, 34, 56);
    let thumb = ailloli_ui_core::Color::rgb(78, 90, 123);
    let style = EditorScrollbarStyle {
        track_color: track,
        thumb_color: thumb,
        thickness: 8.0,
        min_thumb_len: 28.0,
        inset: 4.0,
        radius: 4.0,
    };
    let document = State::new(Document::new(DocumentId(43), TextBuffer::from_string(text)));
    let runtime: RuntimeHandle<()> = RuntimeHandle::new();
    let mut app = Runtime::new(runtime);
    app.reconcile(
        CodeEditor::new(document)
            .scrollbar_style(style)
            .width(260.0)
            .height(130.0)
            .into_view(),
    );
    app.layout(
        Constraints::tight(260.0, 130.0),
        Scale::new(1.0),
        &mut text_system,
    );
    let styled_scene = app.paint(&mut text_system);
    let rrects = rrects_in_text_layer(&styled_scene);
    assert!(rrects.iter().any(|rrect| rrect.color == track));
    assert!(rrects.iter().any(|rrect| rrect.color == thumb));
    assert!(rrects
        .iter()
        .any(|rrect| (rrect.radius - style.radius).abs() < 0.001));
}

#[test]
fn code_editor_scrollbar_thumb_moves_on_wheel_without_moving_gutter() {
    let long_line = "a".repeat(260);
    let document = State::new(Document::new(
        DocumentId(44),
        TextBuffer::from_string(format!("{long_line}\n")),
    ));
    let runtime: RuntimeHandle<()> = RuntimeHandle::new();
    let mut app = Runtime::new(runtime.clone());
    app.reconcile(
        CodeEditor::new(document)
            .width(190.0)
            .height(90.0)
            .into_view(),
    );
    let mut text_system = TextSystem::new();
    app.layout(
        Constraints::tight(190.0, 90.0),
        Scale::new(1.0),
        &mut text_system,
    );
    let initial_scene = app.paint(&mut text_system);
    let initial_line_number_x = first_text_x(&initial_scene, "1");
    let thumb_color = EditorScrollbarStyle::default().thumb_color;
    let initial_thumb_x = rrects_in_text_layer(&initial_scene)
        .into_iter()
        .find(|rrect| rrect.color == thumb_color && rrect.rect.w > rrect.rect.h)
        .expect("initial horizontal thumb")
        .rect
        .x;

    let mut router = InputRouter::default();
    router.route_event(
        &app.tree,
        runtime,
        &Event::Pointer(PointerEvent::Wheel {
            pos: Point::new(80.0, 20.0),
            delta: WheelDelta::PixelDelta { x: -50.0, y: 0.0 },
            modifiers: Modifiers::default(),
            precise: true,
        }),
    );
    let scrolled_scene = app.paint(&mut text_system);
    let scrolled_line_number_x = first_text_x(&scrolled_scene, "1");
    let scrolled_thumb_x = rrects_in_text_layer(&scrolled_scene)
        .into_iter()
        .find(|rrect| rrect.color == thumb_color && rrect.rect.w > rrect.rect.h)
        .expect("scrolled horizontal thumb")
        .rect
        .x;

    assert_eq!(scrolled_line_number_x, initial_line_number_x);
    assert!(scrolled_thumb_x > initial_thumb_x);
}

#[test]
fn code_editor_scrollbar_drag_uses_capture_without_editing_document() {
    let source = (0..24)
        .map(|index| format!("let value_{index} = some_really_long_function_name(value_{index});"))
        .collect::<Vec<_>>()
        .join("\n");
    let document = State::new(Document::new(
        DocumentId(62),
        TextBuffer::from_string(source.clone()),
    ));
    let runtime: RuntimeHandle<()> = RuntimeHandle::new();
    let mut app = Runtime::new(runtime.clone());
    app.reconcile(
        CodeEditor::new(document.clone())
            .width(210.0)
            .height(100.0)
            .into_view(),
    );
    let mut text_system = TextSystem::new();
    let constraints = Constraints::tight(210.0, 100.0);
    app.layout(constraints, Scale::new(1.0), &mut text_system);

    let initial = app.paint(&mut text_system);
    let horizontal_thumb = rrects_in_text_layer(&initial)
        .into_iter()
        .filter(|rrect| rrect.rect.w > rrect.rect.h && rrect.rect.h <= 8.0)
        .min_by(|a, b| a.rect.w.total_cmp(&b.rect.w))
        .expect("horizontal scrollbar thumb");
    let initial_x = horizontal_thumb.rect.x;
    let press = Point::new(
        horizontal_thumb.rect.x + horizontal_thumb.rect.w * 0.5,
        horizontal_thumb.rect.y + horizontal_thumb.rect.h * 0.5,
    );
    let mut router = InputRouter::default();
    router.route_event(
        &app.tree,
        runtime.clone(),
        &Event::Pointer(PointerEvent::Button {
            pos: press,
            button: MouseButton::Left,
            pressed: true,
            modifiers: Modifiers::default(),
        }),
    );
    router.route_event(
        &app.tree,
        runtime.clone(),
        &Event::Pointer(PointerEvent::Moved {
            pos: Point::new(press.x + 4.0, press.y),
            modifiers: Modifiers::default(),
        }),
    );
    app.layout(constraints, Scale::new(1.0), &mut text_system);
    router.route_event(
        &app.tree,
        runtime.clone(),
        &Event::Pointer(PointerEvent::Moved {
            pos: Point::new(2_000.0, press.y),
            modifiers: Modifiers::default(),
        }),
    );
    app.layout(constraints, Scale::new(1.0), &mut text_system);
    router.route_event(
        &app.tree,
        runtime,
        &Event::Pointer(PointerEvent::Button {
            pos: Point::new(2_000.0, press.y),
            button: MouseButton::Left,
            pressed: false,
            modifiers: Modifiers::default(),
        }),
    );

    let dragged = app.paint(&mut text_system);
    let dragged_x = rrects_in_text_layer(&dragged)
        .into_iter()
        .filter(|rrect| rrect.rect.w > rrect.rect.h && rrect.rect.h <= 8.0)
        .min_by(|a, b| a.rect.w.total_cmp(&b.rect.w))
        .expect("dragged horizontal scrollbar thumb")
        .rect
        .x;
    assert!(
        dragged_x > initial_x,
        "initial={initial_x}, dragged={dragged_x}"
    );
    assert_eq!(document.read().buffer.as_str(), source);
}

#[test]
fn code_editor_initial_caret_reveals_long_nowrap_line() {
    let source =
        "let generated_value = call_with_a_very_long_argument_name_and_another_argument();";
    let document = State::new(Document::new(
        DocumentId(63),
        TextBuffer::from_string(source),
    ));
    let runtime: RuntimeHandle<()> = RuntimeHandle::new();
    let mut app = Runtime::new(runtime);
    app.reconcile(
        CodeEditor::new(document)
            .initial_selection(source.len(), source.len())
            .width(190.0)
            .height(90.0)
            .into_view(),
    );
    let mut text_system = TextSystem::new();
    app.layout(
        Constraints::tight(190.0, 90.0),
        Scale::new(1.0),
        &mut text_system,
    );

    let scene = app.paint(&mut text_system);
    let source_x = first_text_x(&scene, source);
    assert!(
        source_x < 58.0,
        "the no-wrap caret at line end must reveal by scrolling horizontally: x={source_x}"
    );
}

#[test]
fn code_editor_pointer_clicks_do_not_move_a_scrolled_viewport() {
    let source = scrolling_editor_source();
    let document = State::new(Document::new(
        DocumentId(64),
        TextBuffer::from_string(source),
    ));
    let runtime: RuntimeHandle<()> = RuntimeHandle::new();
    let mut app = Runtime::new(runtime.clone());
    app.reconcile(
        CodeEditor::new(document)
            .initial_scroll(180.0, 180.0)
            .width(260.0)
            .height(130.0)
            .into_view(),
    );
    let mut text_system = TextSystem::new();
    app.layout(
        Constraints::tight(260.0, 130.0),
        Scale::new(1.0),
        &mut text_system,
    );

    let before = code_editor_thumb_origins(&app.paint(&mut text_system));
    let mut router = InputRouter::default();
    let pos = Point::new(180.0, 100.0);
    click_left_at(&mut router, &app, runtime.clone(), pos, 100);
    app.layout(
        Constraints::tight(260.0, 130.0),
        Scale::new(1.0),
        &mut text_system,
    );
    let after_single = code_editor_thumb_origins(&app.paint(&mut text_system));

    click_left_at(&mut router, &app, runtime, pos, 300);
    app.layout(
        Constraints::tight(260.0, 130.0),
        Scale::new(1.0),
        &mut text_system,
    );
    let after_double = code_editor_thumb_origins(&app.paint(&mut text_system));

    assert_eq!(after_single, before, "simple click moved the viewport");
    assert_eq!(after_double, before, "double click moved the viewport");
}

#[test]
fn code_editor_arrow_down_keeps_a_visible_caret_inside_the_safe_region() {
    let source = numbered_editor_source(40);
    let document = State::new(Document::new(
        DocumentId(69),
        TextBuffer::from_string(source),
    ));
    let runtime: RuntimeHandle<()> = RuntimeHandle::new();
    let mut app = Runtime::new(runtime.clone());
    app.reconcile(
        CodeEditor::new(document)
            .width(260.0)
            .height(326.0)
            .into_view(),
    );
    let mut text_system = TextSystem::new();
    app.layout(
        Constraints::tight(260.0, 326.0),
        Scale::new(1.0),
        &mut text_system,
    );
    let mut router = InputRouter::default();
    click_left_at(
        &mut router,
        &app,
        runtime.clone(),
        Point::new(100.0, 163.0),
        100,
    );
    let initial_thumb = code_editor_vertical_thumb_y(&app.paint(&mut text_system));

    // Lines 9 -> 10 -> 11 -> 12 -> 13 -> 14 remain above the three-line
    // bottom safety margin and must not move the viewport at all.
    for _ in 0..5 {
        route_named_key(&mut router, &app, runtime.clone(), NamedKey::ArrowDown);
        app.layout(
            Constraints::tight(260.0, 326.0),
            Scale::new(1.0),
            &mut text_system,
        );
        assert_eq!(
            code_editor_vertical_thumb_y(&app.paint(&mut text_system)),
            initial_thumb,
            "a visible ArrowDown caret moved the viewport"
        );
    }

    route_named_key(&mut router, &app, runtime.clone(), NamedKey::ArrowDown);
    app.layout(
        Constraints::tight(260.0, 326.0),
        Scale::new(1.0),
        &mut text_system,
    );
    let first_crossing_scene = app.paint(&mut text_system);
    let first_crossing_thumb = code_editor_vertical_thumb_y(&first_crossing_scene);
    assert!(first_crossing_thumb > initial_thumb);
    assert!(
        first_text_y(&first_crossing_scene, "line 15") > 200.0,
        "crossing the margin aligned the caret line near the viewport top"
    );

    route_named_key(&mut router, &app, runtime, NamedKey::ArrowDown);
    app.layout(
        Constraints::tight(260.0, 326.0),
        Scale::new(1.0),
        &mut text_system,
    );
    let second_crossing_scene = app.paint(&mut text_system);
    assert!(code_editor_vertical_thumb_y(&second_crossing_scene) > first_crossing_thumb);
    assert!(
        first_text_y(&second_crossing_scene, "line 16") > 200.0,
        "continuous navigation repositioned the caret line near the top"
    );
}

#[test]
fn nested_flex_editor_pane_keeps_visible_arrow_navigation_stable() {
    let document = State::new(Document::new(
        DocumentId(73),
        TextBuffer::from_string(numbered_editor_source(40)),
    ));
    let runtime: RuntimeHandle<()> = RuntimeHandle::new();
    let mut app = Runtime::new(runtime.clone());
    app.reconcile(
        Row::new()
            .fill()
            .child(
                Column::new().fill().child(
                    SplitPane::columns(
                        Container::new().fill(),
                        EditorPane::new(CodeEditor::new(document).fill()).fill(),
                    )
                    .initial_position(96.0)
                    .fill(),
                ),
            )
            .into_view(),
    );
    let mut text_system = TextSystem::new();
    let constraints = Constraints::tight(640.0, 420.0);
    app.layout(constraints, Scale::new(1.0), &mut text_system);

    let scene = app.paint(&mut text_system);
    let line_nine = Point::new(
        first_text_x(&scene, "line 9") + 16.0,
        first_text_y(&scene, "line 9") + 6.0,
    );
    let mut router = InputRouter::default();
    click_left_at(&mut router, &app, runtime.clone(), line_nine, 100);
    let before = code_editor_vertical_thumb_y(&app.paint(&mut text_system));

    route_named_key(&mut router, &app, runtime, NamedKey::ArrowDown);
    app.layout(constraints, Scale::new(1.0), &mut text_system);
    let after_scene = app.paint(&mut text_system);

    assert_eq!(
        code_editor_vertical_thumb_y(&after_scene),
        before,
        "an intermediate nested-Flex measurement moved the committed viewport"
    );
    assert!(
        first_text_y(&after_scene, "line 10") > first_text_y(&after_scene, "line 1"),
        "ArrowDown aligned the already-visible caret line to the viewport top"
    );
}

#[test]
fn code_editor_upward_navigation_is_idle_until_the_caret_crosses_the_top() {
    let document = State::new(Document::new(
        DocumentId(70),
        TextBuffer::from_string(numbered_editor_source(40)),
    ));
    let runtime: RuntimeHandle<()> = RuntimeHandle::new();
    let mut app = Runtime::new(runtime.clone());
    app.reconcile(
        CodeEditor::new(document)
            .initial_scroll(0.0, 180.0)
            .width(260.0)
            .height(180.0)
            .into_view(),
    );
    let mut text_system = TextSystem::new();
    app.layout(
        Constraints::tight(260.0, 180.0),
        Scale::new(1.0),
        &mut text_system,
    );
    let mut router = InputRouter::default();
    click_left_at(
        &mut router,
        &app,
        runtime.clone(),
        Point::new(100.0, 90.0),
        100,
    );
    let initial_thumb = code_editor_vertical_thumb_y(&app.paint(&mut text_system));
    for _ in 0..3 {
        route_named_key(&mut router, &app, runtime.clone(), NamedKey::ArrowUp);
        app.layout(
            Constraints::tight(260.0, 180.0),
            Scale::new(1.0),
            &mut text_system,
        );
        assert_eq!(
            code_editor_vertical_thumb_y(&app.paint(&mut text_system)),
            initial_thumb
        );
    }

    route_named_key(&mut router, &app, runtime, NamedKey::PageUp);
    app.layout(
        Constraints::tight(260.0, 180.0),
        Scale::new(1.0),
        &mut text_system,
    );
    assert!(code_editor_vertical_thumb_y(&app.paint(&mut text_system)) < initial_thumb);
}

#[test]
fn code_editor_horizontal_navigation_scrolls_only_after_crossing_an_edge() {
    let source = "abcdefghijklmnopqrstuvwxyz".repeat(20);
    let document = State::new(Document::new(
        DocumentId(71),
        TextBuffer::from_string(source),
    ));
    let runtime: RuntimeHandle<()> = RuntimeHandle::new();
    let mut app = Runtime::new(runtime.clone());
    app.reconcile(
        CodeEditor::new(document)
            .initial_scroll(120.0, 0.0)
            .width(260.0)
            .height(90.0)
            .into_view(),
    );
    let mut text_system = TextSystem::new();
    app.layout(
        Constraints::tight(260.0, 90.0),
        Scale::new(1.0),
        &mut text_system,
    );
    let mut router = InputRouter::default();
    click_left_at(
        &mut router,
        &app,
        runtime.clone(),
        Point::new(140.0, 25.0),
        100,
    );
    let initial_thumb = code_editor_horizontal_thumb_x(&app.paint(&mut text_system));
    for _ in 0..5 {
        route_named_key(&mut router, &app, runtime.clone(), NamedKey::ArrowRight);
        app.layout(
            Constraints::tight(260.0, 90.0),
            Scale::new(1.0),
            &mut text_system,
        );
        assert_eq!(
            code_editor_horizontal_thumb_x(&app.paint(&mut text_system)),
            initial_thumb
        );
    }

    route_named_key(&mut router, &app, runtime, NamedKey::End);
    app.layout(
        Constraints::tight(260.0, 90.0),
        Scale::new(1.0),
        &mut text_system,
    );
    assert!(code_editor_horizontal_thumb_x(&app.paint(&mut text_system)) > initial_thumb);
}

#[test]
fn editor_pointer_click_does_not_move_a_scrolled_viewport() {
    let source = scrolling_editor_source();
    let buffer = State::new(TextBuffer::from_string(source));
    let runtime: RuntimeHandle<()> = RuntimeHandle::new();
    let mut app = Runtime::new(runtime.clone());
    app.reconcile(
        Editor::new(buffer)
            .wrap_mode(EditorWrapMode::NoWrap)
            .width(220.0)
            .height(120.0)
            .into_view(),
    );
    let mut text_system = TextSystem::new();
    app.layout(
        Constraints::tight(220.0, 120.0),
        Scale::new(1.0),
        &mut text_system,
    );
    let mut router = InputRouter::default();
    router.route_event(
        &app.tree,
        runtime.clone(),
        &Event::Pointer(PointerEvent::Wheel {
            pos: Point::new(100.0, 60.0),
            delta: WheelDelta::PixelDelta {
                x: -120.0,
                y: -120.0,
            },
            modifiers: Modifiers::default(),
            precise: true,
        }),
    );
    let before = text_draw_positions(&app.paint(&mut text_system));

    click_left_at(&mut router, &app, runtime, Point::new(170.0, 95.0), 100);
    app.layout(
        Constraints::tight(220.0, 120.0),
        Scale::new(1.0),
        &mut text_system,
    );
    let after = text_draw_positions(&app.paint(&mut text_system));

    assert_eq!(after, before, "pointer caret placement moved the viewport");
}

#[test]
fn code_editor_editing_margin_is_configurable_and_defaults_to_three_lines() {
    let (default_before, default_after) = vertical_thumb_around_edit(65, None);
    let (zero_before, zero_after) = vertical_thumb_around_edit(66, Some(0.0));

    assert_eq!(default_before, zero_before);
    assert!(
        default_after > zero_after,
        "the default edit margin must preserve more space than a zero-line margin"
    );
    assert!(
        zero_after >= zero_before,
        "newline insertion may reveal the caret but cannot scroll backward"
    );
}

#[test]
fn code_editor_editing_follows_the_caret_horizontally() {
    let document = State::new(Document::new(
        DocumentId(68),
        TextBuffer::from_string(scrolling_editor_source()),
    ));
    let runtime: RuntimeHandle<()> = RuntimeHandle::new();
    let mut app = Runtime::new(runtime.clone());
    app.reconcile(
        CodeEditor::new(document)
            .width(260.0)
            .height(130.0)
            .into_view(),
    );
    let mut text_system = TextSystem::new();
    app.layout(
        Constraints::tight(260.0, 130.0),
        Scale::new(1.0),
        &mut text_system,
    );
    let mut router = InputRouter::default();
    let pos = Point::new(180.0, 45.0);
    click_left_at(&mut router, &app, runtime.clone(), pos, 100);
    let before = code_editor_thumb_origins(&app.paint(&mut text_system)).x;
    let inserted = "x".repeat(120);
    type_char(&mut router, &app, runtime, pos, &inserted);
    app.layout(
        Constraints::tight(260.0, 130.0),
        Scale::new(1.0),
        &mut text_system,
    );
    let after = code_editor_thumb_origins(&app.paint(&mut text_system)).x;

    assert!(
        after > before,
        "editing did not reveal the caret on the x axis"
    );
}

#[test]
fn code_editor_pointer_selection_scrolls_only_after_leaving_the_viewport() {
    let source = scrolling_editor_source();
    let document = State::new(Document::new(
        DocumentId(67),
        TextBuffer::from_string(source),
    ));
    let runtime: RuntimeHandle<()> = RuntimeHandle::new();
    let mut app = Runtime::new(runtime.clone());
    app.reconcile(
        CodeEditor::new(document)
            .initial_scroll(120.0, 120.0)
            .width(260.0)
            .height(130.0)
            .into_view(),
    );
    let mut text_system = TextSystem::new();
    app.layout(
        Constraints::tight(260.0, 130.0),
        Scale::new(1.0),
        &mut text_system,
    );
    let before = code_editor_thumb_origins(&app.paint(&mut text_system));
    let mut router = InputRouter::default();
    router.route_event(
        &app.tree,
        runtime.clone(),
        &Event::Pointer(PointerEvent::Button {
            pos: Point::new(140.0, 65.0),
            button: MouseButton::Left,
            pressed: true,
            modifiers: Modifiers::default(),
        }),
    );
    router.route_event(
        &app.tree,
        runtime.clone(),
        &Event::Pointer(PointerEvent::Moved {
            pos: Point::new(180.0, 90.0),
            modifiers: Modifiers::default(),
        }),
    );
    let inside = code_editor_thumb_origins(&app.paint(&mut text_system));
    assert_eq!(inside, before, "in-viewport selection moved the viewport");

    router.route_event(
        &app.tree,
        runtime.clone(),
        &Event::Pointer(PointerEvent::Moved {
            pos: Point::new(400.0, 260.0),
            modifiers: Modifiers::default(),
        }),
    );
    let outside = code_editor_thumb_origins(&app.paint(&mut text_system));
    assert!(
        outside.x > inside.x,
        "horizontal selection did not auto-scroll"
    );
    assert!(
        outside.y > inside.y,
        "vertical selection did not auto-scroll"
    );

    router.route_event(
        &app.tree,
        runtime,
        &Event::Pointer(PointerEvent::Button {
            pos: Point::new(400.0, 260.0),
            button: MouseButton::Left,
            pressed: false,
            modifiers: Modifiers::default(),
        }),
    );
}

#[test]
fn code_editor_ime_cursor_rect_is_inside_text_rect_not_content_rect() {
    let document = State::new(Document::new(
        DocumentId(5),
        TextBuffer::from_string("fn main() {}\n"),
    ));
    let runtime: RuntimeHandle<()> = RuntimeHandle::new();
    let mut app = Runtime::new(runtime.clone());
    app.reconcile(
        CodeEditor::new(document)
            .width(240.0)
            .height(120.0)
            .into_view(),
    );
    let mut text_system = TextSystem::new();
    app.layout(
        Constraints::tight(240.0, 120.0),
        Scale::new(1.0),
        &mut text_system,
    );
    let _ = app.paint(&mut text_system);

    let mut router = InputRouter::default();
    router.route_event(
        &app.tree,
        runtime,
        &Event::Pointer(PointerEvent::Button {
            pos: Point::new(70.0, 20.0),
            button: MouseButton::Left,
            pressed: true,
            modifiers: Modifiers::default(),
        }),
    );

    let rect = router
        .focused_ime_cursor_rect(&app.tree)
        .expect("focused ime rect");

    // Default editor pad is 10px and default CodeEditor gutter is 48px.
    assert!(
        rect.x >= 58.0,
        "IME rect must be inside text_rect after gutter, got {rect:?}"
    );
    assert!(
        rect.x > 10.0,
        "IME rect must not use the pre-gutter content_rect origin: {rect:?}"
    );
}

#[test]
fn editor_paint_clip_does_not_leak_to_following_commands() {
    let buffer = State::new(TextBuffer::from_string(
        "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa".to_string(),
    ));
    let runtime: RuntimeHandle<()> = RuntimeHandle::new();
    let mut app = Runtime::new(runtime);
    app.reconcile(Editor::new(buffer).into_view());
    let mut text_system = TextSystem::new();
    app.layout(
        Constraints::tight(100.0, 80.0),
        Scale::new(1.0),
        &mut text_system,
    );

    let post_rect = Rect::new(0.0, 80.0, 80.0, 30.0);
    let post_color = ailloli_ui_core::Color::hex("#a5b4fc").expect("hex");

    let mut ctx =
        PaintCtx::with_text_system_and_input(&mut text_system, InputSnapshot::default(), 0);
    ailloli_ui_runtime::scene::paint_element(
        &app.tree,
        &mut ctx,
        app.root.expect("root id"),
        Default::default(),
    );
    ctx.push(DrawCmd::Rect(ailloli_ui_runtime::DrawRect {
        rect: post_rect,
        color: post_color,
    }));
    let scene = ctx.into_scene();

    // Editor builds 2 layers (bg without clip, content with clip rect); the
    // post-with_clip layer collects whatever is pushed after, which is our
    // `post_rect`. The critical invariant is that the post-editor layer has an
    // **empty** clip stack (the editor's rect clip did not leak out).
    let post_layer = scene
        .layers
        .iter()
        .find(|layer| {
            layer.cmds.iter().any(|cmd| match cmd {
                DrawCmd::Rect(rect) => rect.rect == post_rect && rect.color == post_color,
                _ => false,
            })
        })
        .expect("post-editor rect must be present in some layer");
    assert!(
        post_layer.clip.is_empty(),
        "editor clip leaked into post-editor layer: {:?}",
        post_layer.clip.entries()
    );
}

fn first_text_x(scene: &ailloli_ui_runtime::Scene, needle: &str) -> f32 {
    scene
        .layers
        .iter()
        .flat_map(|layer| layer.cmds.iter())
        .find_map(|cmd| match cmd {
            DrawCmd::Text(text) if text.layout.text() == needle => Some(text.pos[0]),
            _ => None,
        })
        .unwrap_or_else(|| panic!("missing text item {needle:?}"))
}

fn first_text_y(scene: &ailloli_ui_runtime::Scene, needle: &str) -> f32 {
    scene
        .layers
        .iter()
        .flat_map(|layer| layer.cmds.iter())
        .find_map(|cmd| match cmd {
            DrawCmd::Text(text) if text.layout.text() == needle => Some(text.pos[1]),
            _ => None,
        })
        .unwrap_or_else(|| panic!("missing text item {needle:?}"))
}

fn numbered_editor_source(line_count: usize) -> String {
    (1..=line_count)
        .map(|line| format!("line {line}"))
        .collect::<Vec<_>>()
        .join("\n")
}

fn scrolling_editor_source() -> String {
    (0..80)
        .map(|index| {
            format!(
                "line_{index:02}: {}",
                "a long code-editor payload ".repeat(8)
            )
        })
        .collect::<Vec<_>>()
        .join("\n")
}

fn text_draw_positions(scene: &ailloli_ui_runtime::Scene) -> Vec<(String, [f32; 2])> {
    scene
        .layers
        .iter()
        .flat_map(|layer| layer.cmds.iter())
        .filter_map(|cmd| match cmd {
            DrawCmd::Text(text) => Some((text.layout.text().to_string(), text.pos)),
            _ => None,
        })
        .collect()
}

fn code_editor_thumb_origins(scene: &ailloli_ui_runtime::Scene) -> Point {
    let thumb_color = EditorScrollbarStyle::default().thumb_color;
    let thumbs = rrects_in_text_layer(scene)
        .into_iter()
        .filter(|rrect| rrect.color == thumb_color)
        .collect::<Vec<_>>();
    let horizontal = thumbs
        .iter()
        .find(|rrect| rrect.rect.w > rrect.rect.h)
        .expect("horizontal scrollbar thumb");
    let vertical = thumbs
        .iter()
        .find(|rrect| rrect.rect.h > rrect.rect.w)
        .expect("vertical scrollbar thumb");
    Point::new(horizontal.rect.x, vertical.rect.y)
}

fn code_editor_horizontal_thumb_x(scene: &ailloli_ui_runtime::Scene) -> f32 {
    let thumb_color = EditorScrollbarStyle::default().thumb_color;
    rrects_in_text_layer(scene)
        .into_iter()
        .find(|rrect| rrect.color == thumb_color && rrect.rect.w > rrect.rect.h)
        .expect("horizontal scrollbar thumb")
        .rect
        .x
}

fn code_editor_vertical_thumb_y(scene: &ailloli_ui_runtime::Scene) -> f32 {
    let thumb_color = EditorScrollbarStyle::default().thumb_color;
    scene
        .layers
        .iter()
        .flat_map(|layer| layer.cmds.iter())
        .find_map(|cmd| match cmd {
            DrawCmd::RRect(rrect) if rrect.color == thumb_color && rrect.rect.h > rrect.rect.w => {
                Some(rrect.rect.y)
            }
            _ => None,
        })
        .expect("vertical scrollbar thumb")
}

fn vertical_thumb_around_edit(document_id: u64, margin_lines: Option<f32>) -> (f32, f32) {
    let document = State::new(Document::new(
        DocumentId(document_id),
        TextBuffer::from_string(scrolling_editor_source()),
    ));
    let runtime: RuntimeHandle<()> = RuntimeHandle::new();
    let mut app = Runtime::new(runtime.clone());
    let editor = CodeEditor::new(document)
        .initial_scroll(0.0, 180.0)
        .width(260.0)
        .height(130.0);
    let editor = match margin_lines {
        Some(lines) => editor.caret_follow_margin_lines(lines),
        None => editor,
    };
    app.reconcile(editor.into_view());
    let mut text_system = TextSystem::new();
    app.layout(
        Constraints::tight(260.0, 130.0),
        Scale::new(1.0),
        &mut text_system,
    );
    let mut router = InputRouter::default();
    let pos = Point::new(160.0, 100.0);
    click_left_at(&mut router, &app, runtime.clone(), pos, 100);
    let before = code_editor_thumb_origins(&app.paint(&mut text_system)).y;
    router.route_event(
        &app.tree,
        runtime,
        &Event::Keyboard(KeyEvent {
            state: KeyState::Pressed,
            key: Key::Named(NamedKey::Enter),
            modifiers: Modifiers::default(),
            repeat: false,
            pointer_pos: Some(pos),
            text: Some("\n".into()),
        }),
    );
    app.layout(
        Constraints::tight(260.0, 130.0),
        Scale::new(1.0),
        &mut text_system,
    );
    let after = code_editor_thumb_origins(&app.paint(&mut text_system)).y;
    (before, after)
}

fn rrects_in_text_layer(scene: &ailloli_ui_runtime::Scene) -> Vec<ailloli_ui_runtime::DrawRRect> {
    scene
        .layers
        .iter()
        .filter(|layer| {
            layer
                .clip
                .entries()
                .first()
                .is_some_and(|entry| matches!(entry.shape, ClipShape::Rect(rect) if rect.x >= 58.0))
        })
        .flat_map(|layer| layer.cmds.iter())
        .filter_map(|cmd| match cmd {
            DrawCmd::RRect(rrect) => Some(*rrect),
            _ => None,
        })
        .collect()
}

#[test]
fn editor_rect_clip_inside_window_root_round_clip_restores_parent_for_following_widget() {
    let buffer = State::new(TextBuffer::from_string("Hello, world!"));
    let runtime: RuntimeHandle<()> = RuntimeHandle::new();
    let mut app = Runtime::new(runtime);
    app.reconcile(
        Container::new()
            .width(160.0)
            .height(240.0)
            .radius(8.0)
            .clip_children(true)
            .window_root_clip(true)
            .child(
                Column::new()
                    .child(Editor::new(buffer))
                    .child(Text::new("after")),
            )
            .into_view(),
    );
    let mut text_system = TextSystem::new();
    app.layout(
        Constraints::tight(160.0, 240.0),
        Scale::new(1.0),
        &mut text_system,
    );

    let scene = app.paint(&mut text_system);

    // With `ctx.with_clip` restored in the editor, the editor's text/caret live
    // in a sub-layer that adds `Rect(content)` on top of the window root.
    // The text "after" is painted **after** the editor's `with_clip` pops, so
    // it lands on a sibling layer where the clip stack is back to just the
    // window root round clip.
    let editor_layer = scene
        .layers
        .iter()
        .find(|layer| {
            layer.cmds.iter().any(|cmd| match cmd {
                DrawCmd::Text(text) => text.layout.text() == "Hello, world!",
                _ => false,
            })
        })
        .expect("editor layer with Hello, world!");
    assert_eq!(
        editor_layer.clip.entries().len(),
        2,
        "editor content must be clipped by [window_root, viewport_rect]"
    );
    assert!(editor_layer.clip.entries()[0].is_window_root);
    assert!(matches!(
        editor_layer.clip.entries()[0].shape,
        ClipShape::RoundRect { .. }
    ));
    assert!(matches!(
        editor_layer.clip.entries()[1].shape,
        ClipShape::Rect(_)
    ));
    assert!(!editor_layer.clip.entries()[1].is_window_root);

    let after_layer = scene
        .layers
        .iter()
        .find(|layer| {
            layer.cmds.iter().any(|cmd| match cmd {
                DrawCmd::Text(text) => text.layout.text() == "after",
                _ => false,
            })
        })
        .expect("layer with following text");
    assert_eq!(
        after_layer.clip.entries().len(),
        1,
        "following widget must be under the restored window-root clip only"
    );
    assert!(matches!(
        after_layer.clip.entries()[0].shape,
        ClipShape::RoundRect { .. }
    ));
    assert!(after_layer.clip.entries()[0].is_window_root);
}

#[test]
fn editor_wheel_updates_nowrap_horizontal_scroll() {
    let text = "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa";
    let buffer = State::new(TextBuffer::from_string(text));
    let runtime: RuntimeHandle<()> = RuntimeHandle::new();
    let mut app = Runtime::new(runtime.clone());
    app.reconcile(
        Editor::new(buffer)
            .wrap_mode(EditorWrapMode::NoWrap)
            .into_view(),
    );
    let mut text_system = TextSystem::new();
    app.layout(
        Constraints::tight(100.0, 80.0),
        Scale::new(1.0),
        &mut text_system,
    );
    let _ = app.paint(&mut text_system);

    let mut router = InputRouter::default();
    router.route_event(
        &app.tree,
        runtime,
        &Event::Pointer(PointerEvent::Wheel {
            pos: Point::new(5.0, 5.0),
            delta: WheelDelta::PixelDelta { x: -40.0, y: 0.0 },
            modifiers: Modifiers::default(),
            precise: true,
        }),
    );
    let scene = app.paint(&mut text_system);
    let text_x = scene
        .layers
        .iter()
        .flat_map(|layer| layer.cmds.iter())
        .find_map(|cmd| match cmd {
            DrawCmd::Text(text) => Some(text.pos[0]),
            _ => None,
        })
        .expect("draw text");

    assert_eq!(text_x, -30.0);
}

#[test]
fn editor_wheel_bubbles_to_parent_scroll_when_editor_cannot_scroll() {
    let buffer = State::new(TextBuffer::from_string("short"));
    let runtime: RuntimeHandle<()> = RuntimeHandle::new();
    let mut app = Runtime::new(runtime.clone());
    let root_id = app.reconcile(
        ScrollView::vertical()
            .child(
                Column::new()
                    .child(Editor::new(buffer).height(80.0))
                    .child(Text::new("after")),
            )
            .into_view(),
    );
    let mut text_system = TextSystem::new();
    app.layout(
        Constraints::tight(120.0, 60.0),
        Scale::new(1.0),
        &mut text_system,
    );
    let _ = app.paint(&mut text_system);

    let mut router = InputRouter::default();
    router.route_event(
        &app.tree,
        runtime,
        &Event::Pointer(PointerEvent::Wheel {
            pos: Point::new(5.0, 5.0),
            delta: WheelDelta::PixelDelta { x: 0.0, y: -20.0 },
            modifiers: Modifiers::default(),
            precise: true,
        }),
    );
    app.layout(
        Constraints::tight(120.0, 60.0),
        Scale::new(1.0),
        &mut text_system,
    );

    let scroll_id = app.tree.children_of(root_id)[0];
    let scroll_layout = app.tree.get(scroll_id).unwrap().layout.as_ref().unwrap();
    assert_eq!(
        scroll_layout.children[0].offset,
        ailloli_ui_core::Offset::new(0.0, -20.0)
    );
}

#[test]
fn editor_keyboard_edit_synchronizes_public_buffer() {
    let buffer = State::new(TextBuffer::new());
    let runtime: RuntimeHandle<()> = RuntimeHandle::new();
    let mut app = Runtime::new(runtime.clone());
    app.reconcile(Editor::new(buffer.clone()).into_view());
    let mut text_system = TextSystem::new();
    app.layout(
        Constraints::tight(240.0, 120.0),
        Scale::new(1.0),
        &mut text_system,
    );
    let _ = app.paint(&mut text_system);

    let mut router = InputRouter::default();
    router.route_event(
        &app.tree,
        runtime.clone(),
        &Event::Pointer(PointerEvent::Button {
            pos: Point::new(5.0, 5.0),
            button: MouseButton::Left,
            pressed: true,
            modifiers: Modifiers::default(),
        }),
    );
    router.route_event(
        &app.tree,
        runtime,
        &Event::Keyboard(KeyEvent {
            state: KeyState::Pressed,
            key: Key::Character("a".into()),
            modifiers: Modifiers::default(),
            repeat: false,
            pointer_pos: Some(Point::new(5.0, 5.0)),
            text: Some("a".into()),
        }),
    );

    assert_eq!(buffer.read().as_str(), "a");
}

#[test]
fn editor_double_click_word_selection_replaces_selected_word() {
    let buffer = State::new(TextBuffer::from_string("hello world"));
    let runtime: RuntimeHandle<()> = RuntimeHandle::new();
    let mut app = Runtime::new(runtime.clone());
    app.reconcile(Editor::new(buffer.clone()).into_view());
    let mut text_system = TextSystem::new();
    app.layout(
        Constraints::tight(260.0, 120.0),
        Scale::new(1.0),
        &mut text_system,
    );
    let _ = app.paint(&mut text_system);

    let mut router = InputRouter::default();
    let pos = Point::new(18.0, 18.0);
    click_left_at(&mut router, &app, runtime.clone(), pos, 100);
    click_left_at(&mut router, &app, runtime.clone(), pos, 600);
    type_char(&mut router, &app, runtime, pos, "x");

    assert_eq!(buffer.read().as_str(), "x world");
}

#[test]
fn editor_envelope_click_threshold_resets_before_a_new_double_click() {
    let buffer = State::new(TextBuffer::from_string("hello world"));
    let runtime: RuntimeHandle<()> = RuntimeHandle::new();
    let mut app = Runtime::new(runtime.clone());
    app.reconcile(Editor::new(buffer.clone()).into_view());
    let mut text_system = TextSystem::new();
    app.layout(
        Constraints::tight(260.0, 120.0),
        Scale::new(1.0),
        &mut text_system,
    );
    let _ = app.paint(&mut text_system);

    let mut router = InputRouter::default();
    let pos = Point::new(18.0, 18.0);
    click_left_at(&mut router, &app, runtime.clone(), pos, 100);
    click_left_at(&mut router, &app, runtime.clone(), pos, 601);
    click_left_at(&mut router, &app, runtime.clone(), pos, 1_101);
    type_char(&mut router, &app, runtime, pos, "x");

    assert_eq!(buffer.read().as_str(), "x world");
}

#[test]
fn editor_triple_click_line_selection_replaces_logical_line() {
    let buffer = State::new(TextBuffer::from_string("hello world\nsecond"));
    let runtime: RuntimeHandle<()> = RuntimeHandle::new();
    let mut app = Runtime::new(runtime.clone());
    app.reconcile(Editor::new(buffer.clone()).into_view());
    let mut text_system = TextSystem::new();
    app.layout(
        Constraints::tight(260.0, 120.0),
        Scale::new(1.0),
        &mut text_system,
    );
    let _ = app.paint(&mut text_system);

    let mut router = InputRouter::default();
    let pos = Point::new(18.0, 18.0);
    click_left_at(&mut router, &app, runtime.clone(), pos, 100);
    click_left_at(&mut router, &app, runtime.clone(), pos, 300);
    click_left_at(&mut router, &app, runtime.clone(), pos, 500);
    type_char(&mut router, &app, runtime, pos, "x");

    assert_eq!(buffer.read().as_str(), "x\nsecond");
}

#[test]
fn code_editor_double_click_uses_rust_token_selection() {
    let document = State::new(
        Document::new(
            DocumentId(601),
            TextBuffer::from_string("fn helper_name() {}\n"),
        )
        .with_language(EditorLanguage::Rust)
        .with_path("src/double_click.rs"),
    );
    let runtime: RuntimeHandle<()> = RuntimeHandle::new();
    let mut app = Runtime::new(runtime.clone());
    app.reconcile(
        CodeEditor::new(document.clone())
            .width(360.0)
            .height(140.0)
            .into_view(),
    );
    let mut text_system = TextSystem::new();
    app.layout(
        Constraints::tight(360.0, 140.0),
        Scale::new(1.0),
        &mut text_system,
    );
    let _ = app.paint(&mut text_system);

    let mut router = InputRouter::default();
    let pos = Point::new(84.0, 18.0);
    click_left_at(&mut router, &app, runtime.clone(), pos, 100);
    click_left_at(&mut router, &app, runtime.clone(), pos, 600);
    type_char(&mut router, &app, runtime, pos, "x");

    assert_eq!(document.read().buffer.as_str(), "fn x() {}\n");
}

#[test]
fn code_editor_triple_click_selects_the_logical_line() {
    let document = State::new(
        Document::new(
            DocumentId(603),
            TextBuffer::from_string("fn helper_name() {}\nsecond\n"),
        )
        .with_language(EditorLanguage::Rust),
    );
    let runtime: RuntimeHandle<()> = RuntimeHandle::new();
    let mut app = Runtime::new(runtime.clone());
    app.reconcile(
        CodeEditor::new(document.clone())
            .width(360.0)
            .height(140.0)
            .into_view(),
    );
    let mut text_system = TextSystem::new();
    app.layout(
        Constraints::tight(360.0, 140.0),
        Scale::new(1.0),
        &mut text_system,
    );
    let _ = app.paint(&mut text_system);

    let mut router = InputRouter::default();
    let pos = Point::new(84.0, 18.0);
    click_left_at(&mut router, &app, runtime.clone(), pos, 100);
    click_left_at(&mut router, &app, runtime.clone(), pos, 300);
    click_left_at(&mut router, &app, runtime.clone(), pos, 500);
    type_char(&mut router, &app, runtime, pos, "x");

    assert_eq!(document.read().buffer.as_str(), "x\nsecond\n");
}

#[test]
fn code_editor_double_click_gutter_selects_logical_line() {
    let document = State::new(
        Document::new(DocumentId(602), TextBuffer::from_string("first\nsecond\n"))
            .with_language(EditorLanguage::Rust),
    );
    let runtime: RuntimeHandle<()> = RuntimeHandle::new();
    let mut app = Runtime::new(runtime.clone());
    app.reconcile(
        CodeEditor::new(document.clone())
            .width(360.0)
            .height(140.0)
            .into_view(),
    );
    let mut text_system = TextSystem::new();
    app.layout(
        Constraints::tight(360.0, 140.0),
        Scale::new(1.0),
        &mut text_system,
    );
    let _ = app.paint(&mut text_system);

    let mut router = InputRouter::default();
    let pos = Point::new(20.0, 18.0);
    click_left_at(&mut router, &app, runtime.clone(), pos, 100);
    click_left_at(&mut router, &app, runtime.clone(), pos, 600);
    type_char(&mut router, &app, runtime, pos, "x");

    assert_eq!(document.read().buffer.as_str(), "x\nsecond\n");
}

#[test]
fn text_layout_artifact_variant_still_supports_other_widgets() {
    let artifact = LayoutArtifact::Text(TextSystem::new().layout_cached(
        ailloli_ui_text::TextLayoutParams {
            text: "ok",
            style: ailloli_ui_core::TextStyle::new(
                ailloli_ui_core::FontId::Mono,
                12,
                ailloli_ui_core::Color::WHITE,
            ),
            max_width: None,
            wrap_mode: ailloli_ui_text::WrapMode::NoWrap,
        },
    ));

    assert!(matches!(artifact, LayoutArtifact::Text(_)));
}

fn click_left_at(
    router: &mut InputRouter,
    app: &Runtime<()>,
    runtime: RuntimeHandle<()>,
    pos: Point,
    timestamp_ms: u64,
) {
    let event_id = timestamp_ms.saturating_mul(2);
    for (offset, pressed) in [(0, true), (1, false)] {
        router.route_envelope(
            &app.tree,
            runtime.clone(),
            &EventEnvelope::new(
                EventMeta::new(
                    EventId::new(event_id + offset),
                    EventTimestamp::new(Duration::from_millis(timestamp_ms)),
                    "editor-test",
                    PresentationGeneration::INITIAL,
                ),
                Event::Pointer(PointerEvent::Button {
                    pos,
                    button: MouseButton::Left,
                    pressed,
                    modifiers: Modifiers::default(),
                }),
            ),
        );
    }
}

fn type_char(
    router: &mut InputRouter,
    app: &Runtime<()>,
    runtime: RuntimeHandle<()>,
    pos: Point,
    ch: &str,
) {
    router.route_event(
        &app.tree,
        runtime,
        &Event::Keyboard(KeyEvent {
            state: KeyState::Pressed,
            key: Key::Character(ch.into()),
            modifiers: Modifiers::default(),
            repeat: false,
            pointer_pos: Some(pos),
            text: Some(ch.into()),
        }),
    );
}

fn route_named_key(
    router: &mut InputRouter,
    app: &Runtime<()>,
    runtime: RuntimeHandle<()>,
    key: NamedKey,
) {
    router.route_event(
        &app.tree,
        runtime,
        &Event::Keyboard(KeyEvent {
            state: KeyState::Pressed,
            key: Key::Named(key),
            modifiers: Modifiers::default(),
            repeat: false,
            pointer_pos: None,
            text: None,
        }),
    );
}
