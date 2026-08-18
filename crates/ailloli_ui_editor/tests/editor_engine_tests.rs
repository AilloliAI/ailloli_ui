use ailloli_ui_core::event::ImePreedit;
use ailloli_ui_core::Theme;
use ailloli_ui_core::{Point, Rect};
#[cfg(feature = "tree-sitter")]
use ailloli_ui_editor::code::highlight_rust_tree_sitter_hybrid;
#[cfg(feature = "tree-sitter")]
use ailloli_ui_editor::code::FoldRegionId;
#[cfg(feature = "tree-sitter")]
use ailloli_ui_editor::code::TreeSitterRustSymbolIndexer;
use ailloli_ui_editor::code::{
    detect_language, find_matches, highlight_rust_lexical, index_symbols_with_fallback,
    resolve_document_language, CodeEditorConfig, CodeEditorSession, CodeTheme, CtagsError,
    CtagsRunnerConfig, CtagsSymbolIndexer, Diagnostic, DiagnosticSeverity, DiagnosticSource,
    Document, DocumentId, DocumentSource, DocumentVersion, EditorLanguage, FoldRegion,
    GutterConfig, LexicalRustSymbolIndexer, LspBackend, LspCapabilities, LspDiagnostic, LspError,
    LspRequestId, NoopLspBackend, ScipOccurrenceRole, SearchMatch, SearchQuery,
    SemanticDocumentSymbol, SemanticReference, SymbolEdgeKind, SymbolId, SymbolIndexer, SymbolKind,
    SymbolSource, SyntaxKind, SyntaxToken,
};
use ailloli_ui_editor::layout::{first_layout_baseline, run_visual_bottom, run_visual_top};
use ailloli_ui_editor::{
    EditorClickZone, EditorConfig, EditorEngine, EditorHitZone, EditorPaintItem,
    EditorScrollbarStyle, EditorSession, EditorViewport, EditorWrapMode,
};
use ailloli_ui_fs::FileUri;
use ailloli_ui_text::{TextBuffer, TextEditAction, TextSelection, TextSystem};

fn session_with_text(text: &str) -> EditorSession {
    EditorSession::new(TextBuffer::from_string(text))
}

fn session_with_wrap(text: &str, wrap_mode: EditorWrapMode) -> EditorSession {
    let mut session = session_with_text(text);
    session.config.wrap_mode = wrap_mode;
    session
}

fn scrollbar_rects(frame: &ailloli_ui_editor::EditorFrame) -> Vec<(Rect, Rect)> {
    frame
        .paint_items
        .iter()
        .filter_map(|item| match item {
            EditorPaintItem::Scrollbar {
                track_rect,
                thumb_rect,
                ..
            } => Some((*track_rect, *thumb_rect)),
            _ => None,
        })
        .collect()
}

#[cfg(feature = "tree-sitter")]
fn assert_token(text: &str, tokens: &[SyntaxToken], kind: SyntaxKind, needle: &str) {
    assert!(
        tokens
            .iter()
            .any(|token| token.kind == kind && &text[token.range.clone()] == needle),
        "missing {kind:?} token for {needle:?}; tokens={tokens:?}"
    );
}

#[test]
fn virtual_scroll_only_paints_visible_paragraphs() {
    let doc: String = (0..200)
        .map(|i| format!("Paragraph #{i}\n"))
        .collect::<String>();
    let session = session_with_text(&doc);
    let mut engine = EditorEngine::new();
    let mut text_system = TextSystem::new();

    let frame = engine.frame(
        &session,
        Rect::new(0.0, 0.0, 400.0, 200.0),
        false,
        &mut text_system,
    );

    assert!(frame.painted_paragraphs.len() < session.buffer.paragraphs().len() / 2);
    assert_eq!(frame.painted_paragraphs.first().copied(), Some(0));
}

#[test]
fn scrolling_advances_first_painted_paragraph() {
    let doc: String = (0..200)
        .map(|i| format!("Paragraph #{i}\n"))
        .collect::<String>();
    let mut session = session_with_text(&doc);
    session.edit.scroll_y = 1800.0;
    let mut engine = EditorEngine::new();
    let mut text_system = TextSystem::new();

    let frame = engine.frame(
        &session,
        Rect::new(0.0, 0.0, 400.0, 200.0),
        false,
        &mut text_system,
    );

    assert!(frame.painted_paragraphs.first().copied().unwrap_or(0) > 50);
}

#[test]
fn nowrap_large_file_layout_only_shapes_visible_paragraphs() {
    let doc: String = (0..10_000)
        .map(|i| format!("let value_{i} = {i};\n"))
        .collect::<String>();
    let mut session = session_with_wrap(&doc, EditorWrapMode::NoWrap);
    session.edit.scroll_y = session.config.style.line_height * 9_000.0;
    let mut engine = EditorEngine::new();
    let mut text_system = TextSystem::new();

    let frame = engine.frame(
        &session,
        Rect::new(0.0, 0.0, 420.0, 180.0),
        false,
        &mut text_system,
    );

    assert!(frame.painted_paragraphs.first().copied().unwrap_or(0) >= 8_995);
    assert!(frame.painted_paragraphs.len() < 40);
    assert!(text_system.cached_layout_count() < 80);
    assert!(frame.content_size.h > 9_000.0 * session.config.style.line_height);
}

#[test]
fn nowrap_content_width_accounts_for_longest_line_outside_viewport_without_full_layout() {
    let long_line = "x".repeat(1_200);
    let doc = format!(
        "short start\n{}\n{}",
        (0..600).map(|i| format!("short {i}\n")).collect::<String>(),
        long_line
    );
    let mut session = session_with_wrap(&doc, EditorWrapMode::NoWrap);
    session.edit.scroll_y = 0.0;
    let mut engine = EditorEngine::new();
    let mut text_system = TextSystem::new();

    let frame = engine.frame(
        &session,
        Rect::new(0.0, 0.0, 420.0, 180.0),
        false,
        &mut text_system,
    );

    assert!(frame.content_size.w > 5_000.0);
    assert!(text_system.cached_layout_count() < 80);
    assert!(frame.debug_metrics.used_fast_path);
}

#[test]
fn fenwick_tree_resolves_scroll_offsets_to_paragraph_indices() {
    let tree = ailloli_ui_editor::layout::FenwickTree::from_values(&[10.0, 20.0, 30.0, 40.0]);

    assert_eq!(tree.prefix_sum(3), 60.0);
    assert_eq!(tree.lower_bound(0.0), 0);
    assert_eq!(tree.lower_bound(10.0), 0);
    assert_eq!(tree.lower_bound(10.1), 1);
    assert_eq!(tree.lower_bound(61.0), 3);
}

#[test]
fn editor_frame_debug_metrics_report_cache_reuse_and_paint_counts() {
    let doc: String = (0..250)
        .map(|i| format!("let value_{i} = {i};\n"))
        .collect();
    let mut session = session_with_wrap(&doc, EditorWrapMode::NoWrap);
    session.edit.scroll_y = session.config.style.line_height * 100.0;
    let mut engine = EditorEngine::new();
    let mut text_system = TextSystem::new();
    let bounds = Rect::new(0.0, 0.0, 420.0, 180.0);

    let first = engine.frame(&session, bounds, false, &mut text_system);
    let second = engine.frame(&session, bounds, false, &mut text_system);

    assert!(first.debug_metrics.frame_id < second.debug_metrics.frame_id);
    assert!(first.debug_metrics.paragraph_cache_misses > 0);
    assert!(
        second.debug_metrics.paragraph_cache_hits >= first.debug_metrics.paragraph_cache_misses
    );
    assert_eq!(
        second.debug_metrics.paint_item_count,
        second.paint_items.len()
    );
    assert!(second.debug_metrics.glyph_upload_count > 0);
}

#[test]
fn caret_rect_follows_soft_wrapped_visual_line() {
    let text = "aaaaaaaaaaaa bbbbbbbbbbbb";
    let mut session = session_with_text(text);
    session.edit.caret_byte = text.len();
    let mut engine = EditorEngine::new();
    let mut text_system = TextSystem::new();
    let bounds = Rect::new(0.0, 0.0, 110.0, 220.0);

    let frame = engine.frame(&session, bounds, true, &mut text_system);
    assert_eq!(frame.runs.len(), 1);
    assert!(frame.runs[0].layout.lines.len() > 1);

    let caret = frame
        .paint_items
        .iter()
        .find_map(|item| match item {
            EditorPaintItem::Caret { rect, .. } => Some(*rect),
            _ => None,
        })
        .expect("caret");

    assert!(caret.y > frame.viewport.content_rect.y + 1.0);
}

#[test]
fn editor_caret_blinks_from_frame_time() {
    let session = session_with_text("hello");
    let mut engine = EditorEngine::new();
    let mut text_system = TextSystem::new();
    let bounds = Rect::new(0.0, 0.0, 180.0, 90.0);

    let on_frame = engine.frame_at(&session, bounds, true, 0, &mut text_system);
    assert!(on_frame
        .paint_items
        .iter()
        .any(|item| matches!(item, EditorPaintItem::Caret { .. })));

    let off_frame = engine.frame_at(
        &session,
        bounds,
        true,
        session.config.style.caret_blink_ms as u128,
        &mut text_system,
    );
    assert!(!off_frame
        .paint_items
        .iter()
        .any(|item| matches!(item, EditorPaintItem::Caret { .. })));
}

#[test]
fn editor_caret_is_absent_when_unfocused_even_on_visible_phase() {
    let session = session_with_text("hello");
    let mut engine = EditorEngine::new();
    let mut text_system = TextSystem::new();

    let frame = engine.frame_at(
        &session,
        Rect::new(0.0, 0.0, 180.0, 90.0),
        false,
        0,
        &mut text_system,
    );

    assert!(!frame
        .paint_items
        .iter()
        .any(|item| matches!(item, EditorPaintItem::Caret { .. })));
}

#[test]
fn editor_caret_blink_disabled_stays_visible() {
    let mut session = session_with_text("hello");
    session.config.style.caret_blink_ms = 0;
    let mut engine = EditorEngine::new();
    let mut text_system = TextSystem::new();
    let bounds = Rect::new(0.0, 0.0, 180.0, 90.0);

    for frame_time_ms in [0, 500, 1_500] {
        let frame = engine.frame_at(&session, bounds, true, frame_time_ms, &mut text_system);
        assert!(
            frame
                .paint_items
                .iter()
                .any(|item| matches!(item, EditorPaintItem::Caret { .. })),
            "caret should stay visible at {frame_time_ms}ms"
        );
    }
}

#[test]
fn long_unspaced_text_wraps_inside_editor() {
    let session = session_with_text("cccccccccccccccccccccccccccccccccccccccccccccccc");
    let mut engine = EditorEngine::new();
    let mut text_system = TextSystem::new();

    let frame = engine.frame(
        &session,
        Rect::new(0.0, 0.0, 100.0, 240.0),
        false,
        &mut text_system,
    );

    assert_eq!(frame.runs.len(), 1);
    assert!(frame.runs[0].layout.lines.len() > 1);
    assert!(frame.runs[0].layout.width() <= 82.0);
}

#[test]
fn nowrap_keeps_long_text_on_one_line_and_paints_with_scroll_x() {
    let mut session = session_with_wrap(
        "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa",
        EditorWrapMode::NoWrap,
    );
    session.edit.scroll_x = 30.0;
    let mut engine = EditorEngine::new();
    let mut text_system = TextSystem::new();
    let bounds = Rect::new(0.0, 0.0, 100.0, 80.0);

    let frame = engine.frame(&session, bounds, false, &mut text_system);

    assert_eq!(frame.runs.len(), 1);
    assert_eq!(frame.runs[0].layout.lines.len(), 1);
    assert!(frame.runs[0].layout.width() > 80.0);
    let text_x = frame
        .paint_items
        .iter()
        .find_map(|item| match item {
            EditorPaintItem::Text { pos, .. } => Some(pos[0]),
            _ => None,
        })
        .expect("text item");
    assert_eq!(text_x, frame.viewport.content_rect.x - 30.0);
}

#[test]
fn nowrap_hit_test_uses_scroll_x() {
    let text = "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa";
    let mut session = session_with_wrap(text, EditorWrapMode::NoWrap);
    session.edit.scroll_x = 60.0;
    let mut engine = EditorEngine::new();
    let mut text_system = TextSystem::new();
    let bounds = Rect::new(0.0, 0.0, 100.0, 100.0);
    let frame = engine.frame(&session, bounds, false, &mut text_system);
    let line_y =
        frame.runs[0].layout.lines[0].baseline_y - session.config.style.px_size as f32 / 2.0;

    let hit = engine.hit_test(
        &session,
        bounds,
        Point::new(
            frame.viewport.content_rect.x + 1.0,
            frame.viewport.content_rect.y + line_y,
        ),
        &mut text_system,
    );

    assert!(hit.byte > 0);
}

#[test]
fn ime_preedit_builds_display_buffer_without_mutating_source() {
    let mut session = session_with_text("caf");
    session.edit.caret_byte = 3;
    session.edit.preedit = Some(ImePreedit {
        text: "é".into(),
        selection: None,
    });
    let mut engine = EditorEngine::new();
    let mut text_system = TextSystem::new();

    let frame = engine.frame(
        &session,
        Rect::new(0.0, 0.0, 220.0, 120.0),
        true,
        &mut text_system,
    );

    assert_eq!(session.buffer.as_str(), "caf");
    assert!(frame.runs.iter().any(|run| run.layout.text() == "café"));
}

#[test]
fn selection_across_soft_wrap_draws_multiple_rects() {
    let text = "aaaaaaaaaaaa bbbbbbbbbbbb";
    let mut session = session_with_text(text);
    session.edit.selection = Some(TextSelection {
        anchor: 0,
        caret: text.len(),
    });
    let mut engine = EditorEngine::new();
    let mut text_system = TextSystem::new();

    let frame = engine.frame(
        &session,
        Rect::new(0.0, 0.0, 110.0, 220.0),
        false,
        &mut text_system,
    );

    let selection_rects = frame
        .paint_items
        .iter()
        .filter(|item| matches!(item, EditorPaintItem::Selection { .. }))
        .count();
    assert!(selection_rects > 1);
}

#[test]
fn selection_rect_uses_caret_line_height_for_descenders() {
    let text = "const pppp";
    let mut session = session_with_text(text);
    let start = text.find("pppp").expect("selection text");
    session.edit.selection = Some(TextSelection {
        anchor: start,
        caret: text.len(),
    });
    let mut engine = EditorEngine::new();
    let mut text_system = TextSystem::new();

    let frame = engine.frame(
        &session,
        Rect::new(0.0, 0.0, 220.0, 80.0),
        false,
        &mut text_system,
    );

    let selection_rect = frame
        .paint_items
        .iter()
        .find_map(|item| match item {
            EditorPaintItem::Selection { rect, .. } => Some(*rect),
            _ => None,
        })
        .expect("selection rect");
    let run = frame.runs.first().expect("first run");
    let caret = run.layout.caret_rect_at(start, 1.0);
    let text_origin_y = run.baseline_y - first_layout_baseline(&run.layout);
    let expected_y = frame.viewport.text_origin_y() + text_origin_y + caret.y;
    let expected_h = caret.h.max(session.config.style.px_size as f32 + 2.0);

    assert!((selection_rect.y - expected_y.round()).abs() <= 0.01);
    assert!((selection_rect.h - expected_h).abs() <= 0.01);
}

#[test]
fn double_click_word_selection_uses_lexical_code_boundaries() {
    let text = "let foo_bar = r#async + 'a;\nprintln!(\"ok\");";
    let mut session = session_with_text(text);

    let foo = text.find("foo_bar").expect("foo_bar");
    assert!(session.select_word_at_byte(foo + 2, None, EditorLanguage::Rust));
    assert_eq!(
        session.edit.selection,
        Some(TextSelection {
            anchor: foo,
            caret: foo + "foo_bar".len()
        })
    );

    let raw = text.find("async").expect("raw identifier body");
    assert!(session.select_word_at_byte(raw, None, EditorLanguage::Rust));
    assert_eq!(
        session.edit.selection,
        Some(TextSelection {
            anchor: raw - 2,
            caret: raw + "async".len()
        })
    );

    let lifetime = text.find("'a").expect("lifetime");
    assert!(session.select_word_at_byte(lifetime + 1, None, EditorLanguage::Rust));
    assert_eq!(
        session.edit.selection,
        Some(TextSelection {
            anchor: lifetime,
            caret: lifetime + 2
        })
    );

    let macro_name = text.find("println").expect("macro name");
    assert!(session.select_word_at_byte(macro_name + 2, None, EditorLanguage::Rust));
    assert_eq!(
        session.edit.selection,
        Some(TextSelection {
            anchor: macro_name,
            caret: macro_name + "println".len()
        })
    );

    let bang = text.find('!').expect("macro bang");
    assert!(session.select_word_at_byte(bang, None, EditorLanguage::Rust));
    assert_eq!(
        session.edit.selection,
        Some(TextSelection {
            anchor: bang,
            caret: bang + 1
        })
    );
}

#[test]
fn double_click_token_selection_prefers_selectable_syntax_tokens() {
    let text = "fn main() { let value = \"value\"; // value\n }";
    let value = text.find("value").expect("value");
    let string = text.find("\"value\"").expect("string");
    let comment = text.find("// value").expect("comment");
    let mut session = session_with_text(text);
    let tokens = vec![
        SyntaxToken {
            range: value..value + "value".len(),
            kind: SyntaxKind::Function,
        },
        SyntaxToken {
            range: string..string + "\"value\"".len(),
            kind: SyntaxKind::String,
        },
        SyntaxToken {
            range: comment..text.len() - 1,
            kind: SyntaxKind::Comment,
        },
    ];

    assert!(session.select_word_at_byte(value + 1, Some(&tokens), EditorLanguage::Rust));
    assert_eq!(
        session.edit.selection,
        Some(TextSelection {
            anchor: value,
            caret: value + "value".len()
        })
    );

    assert!(session.select_word_at_byte(string + 2, Some(&tokens), EditorLanguage::Rust));
    assert_eq!(
        session.edit.selection,
        Some(TextSelection {
            anchor: string + 1,
            caret: string + 1 + "value".len()
        })
    );

    assert!(session.select_word_at_byte(comment + 3, Some(&tokens), EditorLanguage::Rust));
    assert_eq!(
        session.edit.selection,
        Some(TextSelection {
            anchor: comment + 3,
            caret: comment + 3 + "value".len()
        })
    );
}

#[test]
fn line_selection_selects_logical_line_without_newline() {
    let text = "first\nsecond line\nthird";
    let mut session = session_with_text(text);
    let second = text.find("second").expect("second");

    assert!(session.select_line_at_byte(second + 4));

    assert_eq!(
        session.edit.selection,
        Some(TextSelection {
            anchor: second,
            caret: second + "second line".len()
        })
    );
}

#[test]
fn multi_click_state_counts_nearby_text_clicks_and_resets_by_zone() {
    use std::time::{Duration, Instant};

    let mut session = session_with_text("abc");
    let start = Instant::now();

    assert_eq!(
        session.register_pointer_click(start, Point::new(8.0, 8.0), 1, EditorClickZone::Text),
        1
    );
    assert_eq!(
        session.register_pointer_click(
            start + Duration::from_millis(200),
            Point::new(10.0, 9.0),
            1,
            EditorClickZone::Text
        ),
        2
    );
    assert_eq!(
        session.register_pointer_click(
            start + Duration::from_millis(350),
            Point::new(11.0, 9.0),
            1,
            EditorClickZone::Text
        ),
        3
    );
    assert_eq!(
        session.register_pointer_click(
            start + Duration::from_millis(450),
            Point::new(11.0, 9.0),
            1,
            EditorClickZone::Gutter
        ),
        1
    );
}

#[test]
fn code_editor_hit_test_zone_distinguishes_gutter_from_text() {
    let document = Document::new(
        DocumentId(900),
        TextBuffer::from_string("fn main() {}\nlet value = 1;\n"),
    )
    .with_language(EditorLanguage::Rust);
    let session = CodeEditorSession::new(document, CodeEditorConfig::default());
    let mut engine = EditorEngine::new();
    let mut text_system = TextSystem::new();
    let bounds = Rect::new(0.0, 0.0, 420.0, 180.0);
    let frame = engine.code_frame_at(&session, bounds, false, 0, &mut text_system);
    let gutter = frame.viewport.gutter_rect.expect("gutter");

    let gutter_hit = engine.hit_test_zone_cached(
        &session.editor,
        bounds,
        Point::new(gutter.x + 4.0, gutter.y + 6.0),
    );
    let text_hit = engine.hit_test_zone_cached(
        &session.editor,
        bounds,
        Point::new(
            frame.viewport.text_rect.x + 20.0,
            frame.viewport.text_rect.y + 6.0,
        ),
    );

    assert_eq!(gutter_hit.zone, EditorHitZone::Gutter);
    assert_eq!(text_hit.zone, EditorHitZone::Text);
}

#[test]
fn paragraph_after_soft_wrap_is_laid_out_below_visual_height() {
    let session = session_with_text("cccccccccccccccccccccccccccccccc\nnext");
    let mut engine = EditorEngine::new();
    let mut text_system = TextSystem::new();

    let frame = engine.frame(
        &session,
        Rect::new(0.0, 0.0, 100.0, 420.0),
        false,
        &mut text_system,
    );

    assert_eq!(frame.runs.len(), 2);
    assert!(frame.runs[0].layout.lines.len() > 1);
    assert!(
        run_visual_top(&frame.runs[1])
            >= run_visual_bottom(&frame.runs[0], session.config.style) - 0.5
    );
}

#[test]
fn external_buffer_replace_clamps_caret() {
    let mut session = session_with_text("hello world");
    session.edit.caret_byte = 11;

    assert!(session.replace_buffer_if_changed(TextBuffer::from_string("hi")));

    assert_eq!(session.buffer.as_str(), "hi");
    assert_eq!(session.edit.caret_byte, 2);
}

#[test]
fn empty_buffer_produces_stable_frame() {
    let session = EditorSession::with_config(TextBuffer::new(), EditorConfig::default());
    let mut engine = EditorEngine::new();
    let mut text_system = TextSystem::new();

    let frame = engine.frame(
        &session,
        Rect::new(0.0, 0.0, 100.0, 80.0),
        true,
        &mut text_system,
    );

    assert_eq!(frame.painted_paragraphs.first().copied(), Some(0));
    assert!(frame
        .paint_items
        .iter()
        .any(|item| matches!(item, EditorPaintItem::Background { .. })));
}

#[test]
fn soft_wrap_ignores_horizontal_scroll() {
    let mut session = session_with_text("hello");
    session.edit.scroll_x = 44.0;
    let mut engine = EditorEngine::new();
    let mut text_system = TextSystem::new();

    let frame = engine.frame(
        &session,
        Rect::new(0.0, 0.0, 100.0, 80.0),
        false,
        &mut text_system,
    );

    assert_eq!(frame.viewport.scroll_x, 0.0);
}

#[test]
fn code_editor_config_defaults_to_nowrap_with_gutter() {
    let config = CodeEditorConfig::default();

    assert_eq!(config.editor.wrap_mode, EditorWrapMode::NoWrap);
    assert!(config.gutter.enabled);
    assert!(config.gutter.line_numbers);
    assert!(config.gutter.width > 0.0);
    assert_eq!(config.editor.style.bg, config.theme.background);
    assert_eq!(config.editor.style.fg, config.theme.foreground);
}

#[test]
fn code_theme_uses_framework_theme_tokens() {
    let theme = CodeTheme::from_theme(Theme::default());
    let palette = Theme::default().palette();

    assert_eq!(theme.background, palette.background);
    assert_eq!(theme.foreground, palette.text);
    assert_eq!(theme.diagnostic_error, palette.danger);
    assert_eq!(CodeTheme::default(), theme);
}

#[test]
fn code_editor_session_initializes_from_document() {
    let document = Document::new(DocumentId(7), TextBuffer::from_string("fn main() {}\n"));
    let session = CodeEditorSession::new(document.clone(), CodeEditorConfig::default());

    assert_eq!(session.document.id, document.id);
    assert_eq!(session.editor.buffer.as_str(), document.buffer.as_str());
    assert_eq!(session.editor.config.wrap_mode, EditorWrapMode::NoWrap);
}

#[test]
fn document_language_detection_uses_path_extension() {
    let mut document =
        Document::new(DocumentId(70), TextBuffer::from_string("{}")).with_path("src/main.rs");

    document.detect_language();

    assert_eq!(document.language, EditorLanguage::Rust);
    assert_eq!(
        detect_language(Some(std::path::Path::new("README.md"))),
        EditorLanguage::Markdown
    );
    assert_eq!(detect_language(None), EditorLanguage::PlainText);
}

#[test]
fn document_source_tracks_memory_paths_and_file_uris() {
    let memory = Document::new(DocumentId(74), TextBuffer::from_string("fn main() {}\n"));
    assert_eq!(memory.path, None);
    assert_eq!(memory.source, DocumentSource::Memory);
    assert_eq!(memory.language, EditorLanguage::PlainText);

    let local = Document::new(DocumentId(75), TextBuffer::from_string("fn main() {}\n"))
        .with_path("src/main.rs");
    assert_eq!(local.path, Some(std::path::PathBuf::from("src/main.rs")));
    assert_eq!(
        local.source,
        DocumentSource::LocalPath(std::path::PathBuf::from("src/main.rs"))
    );
    assert_eq!(local.version, DocumentVersion(0));
    assert!(!local.dirty);

    let uri = FileUri::parse("file:///repo/src/main.rs").expect("file uri");
    let mut from_uri = Document::new(DocumentId(76), TextBuffer::from_string("fn main() {}\n"))
        .with_uri(uri.clone());
    assert_eq!(
        from_uri.path,
        Some(std::path::PathBuf::from("/repo/src/main.rs"))
    );
    assert_eq!(from_uri.source, DocumentSource::Uri(uri));
    from_uri.detect_language();
    assert_eq!(from_uri.language, EditorLanguage::Rust);

    let remote_uri = FileUri::parse("sftp://prod/var/www/app.rs").expect("remote uri");
    let remote = Document::new(DocumentId(77), TextBuffer::from_string("fn main() {}\n"))
        .with_uri(remote_uri);
    assert_eq!(remote.path, None);
    assert_eq!(
        resolve_document_language(&remote, None),
        EditorLanguage::Rust
    );
}

#[test]
fn resolve_document_language_uses_extension_when_language_is_implicit() {
    let plain_rs = Document::new(DocumentId(71), TextBuffer::from_string("fn main() {}\n"))
        .with_path("main.rs");
    assert_eq!(
        resolve_document_language(&plain_rs, None),
        EditorLanguage::Rust
    );

    let unknown_rs = Document::new(DocumentId(72), TextBuffer::from_string("fn main() {}\n"))
        .with_path("main.rs")
        .with_language(EditorLanguage::Unknown);
    assert_eq!(
        resolve_document_language(&unknown_rs, None),
        EditorLanguage::Rust
    );

    let explicit_markdown = Document::new(DocumentId(73), TextBuffer::from_string("# title\n"))
        .with_path("main.rs")
        .with_language(EditorLanguage::Markdown);
    assert_eq!(
        resolve_document_language(&explicit_markdown, None),
        EditorLanguage::Markdown
    );

    assert_eq!(
        resolve_document_language(&explicit_markdown, Some(EditorLanguage::PlainText)),
        EditorLanguage::PlainText
    );
}

#[test]
fn code_editor_session_syncs_document_from_editor() {
    let document = Document::new(DocumentId(8), TextBuffer::from_string("fn main()"));
    let mut session = CodeEditorSession::new(document, CodeEditorConfig::default());

    session
        .editor
        .apply_edit_action(TextEditAction::InsertText { text: " {}".into() });

    assert!(session.sync_document_from_editor());
    assert_eq!(session.document.buffer.as_str(), " {}fn main()");
    assert!(session.document.dirty);
    assert_eq!(session.document.version.0, 1);
    assert!(!session.sync_document_from_editor());
}

#[test]
fn viewport_without_gutter_keeps_text_rect_equal_to_content_rect() {
    let session = session_with_text("hello");
    let viewport = EditorViewport::new(
        Rect::new(0.0, 0.0, 100.0, 80.0),
        session.config,
        &session.edit,
    );

    assert_eq!(viewport.gutter_rect, None);
    assert_eq!(viewport.text_rect, viewport.content_rect);
    assert_eq!(viewport.text_origin_x(), viewport.content_rect.x);
    assert_eq!(viewport.text_origin_y(), viewport.content_rect.y);
}

#[test]
fn viewport_with_gutter_splits_content_and_text_rects() {
    let mut session = session_with_wrap("hello", EditorWrapMode::NoWrap);
    session.edit.scroll_x = 12.0;
    let viewport = EditorViewport::with_gutter(
        Rect::new(0.0, 0.0, 120.0, 80.0),
        session.config,
        &session.edit,
        Some(GutterConfig {
            enabled: true,
            width: 32.0,
            line_numbers: true,
            fold_markers: true,
        }),
    );

    let gutter = viewport.gutter_rect.expect("gutter rect");
    assert_eq!(gutter.x, viewport.content_rect.x);
    assert_eq!(gutter.w, 32.0);
    assert_eq!(viewport.text_rect.x, viewport.content_rect.x + 32.0);
    assert_eq!(viewport.text_rect.w, viewport.content_rect.w - 32.0);
    assert_eq!(viewport.text_origin_x(), viewport.text_rect.x - 12.0);
}

#[test]
fn code_frame_adds_gutter_and_line_numbers_outside_text_rect() {
    let document = Document::new(
        DocumentId(9),
        TextBuffer::from_string("fn main() {}\nlet value = 1;\n"),
    );
    let session = CodeEditorSession::new(document, CodeEditorConfig::default());
    let mut engine = EditorEngine::new();
    let mut text_system = TextSystem::new();

    let frame = engine.code_frame(
        &session,
        Rect::new(0.0, 0.0, 240.0, 120.0),
        false,
        &mut text_system,
    );

    assert!(frame.viewport.gutter_rect.is_some());
    assert!(frame.viewport.text_rect.x > frame.viewport.content_rect.x);
    assert!(frame
        .paint_items
        .iter()
        .any(|item| matches!(item, EditorPaintItem::GutterBackground { .. })));
    let line_numbers = frame
        .paint_items
        .iter()
        .filter(|item| matches!(item, EditorPaintItem::LineNumber { .. }))
        .count();
    assert_eq!(line_numbers, frame.runs.len());
    let first_text_x = frame
        .paint_items
        .iter()
        .find_map(|item| match item {
            EditorPaintItem::Text { pos, .. } => Some(pos[0]),
            _ => None,
        })
        .expect("text item");
    assert_eq!(first_text_x, frame.viewport.text_rect.x);
}

#[test]
fn code_frame_without_line_numbers_omits_line_number_items() {
    let document = Document::new(DocumentId(10), TextBuffer::from_string("hello\n"));
    let mut config = CodeEditorConfig::default();
    config.gutter.line_numbers = false;
    let session = CodeEditorSession::new(document, config);
    let mut engine = EditorEngine::new();
    let mut text_system = TextSystem::new();

    let frame = engine.code_frame(
        &session,
        Rect::new(0.0, 0.0, 220.0, 100.0),
        false,
        &mut text_system,
    );

    assert!(frame
        .paint_items
        .iter()
        .any(|item| matches!(item, EditorPaintItem::GutterBackground { .. })));
    assert!(!frame
        .paint_items
        .iter()
        .any(|item| matches!(item, EditorPaintItem::LineNumber { .. })));
}

#[test]
fn code_frame_line_numbers_track_visible_runs_after_scroll_y() {
    let text: String = (0..120)
        .map(|i| format!("let value_{i:03} = {i};\n"))
        .collect();
    let document = Document::new(DocumentId(18), TextBuffer::from_string(text));
    let mut session = CodeEditorSession::new(document, CodeEditorConfig::default());
    session.editor.edit.scroll_y = session.editor.config.style.line_height * 72.0;
    let mut engine = EditorEngine::new();
    let mut text_system = TextSystem::new();

    let frame = engine.code_frame(
        &session,
        Rect::new(0.0, 0.0, 280.0, 140.0),
        false,
        &mut text_system,
    );

    let labels: Vec<_> = frame
        .paint_items
        .iter()
        .filter_map(|item| match item {
            EditorPaintItem::LineNumber { layout, .. } => Some(layout.text().to_string()),
            _ => None,
        })
        .collect();
    let expected: Vec<_> = frame
        .painted_paragraphs
        .iter()
        .map(|index| (index + 1).to_string())
        .collect();

    assert!(frame.painted_paragraphs.first().copied().unwrap_or(0) >= 70);
    assert_eq!(labels, expected);
}

#[test]
fn code_frame_paints_active_line_only_when_focused() {
    let source = "fn main() {\n    const test: &str = \"test\";\n}\n";
    let document = Document::new(DocumentId(22), TextBuffer::from_string(source))
        .with_language(EditorLanguage::Rust);
    let mut session = CodeEditorSession::new(document, CodeEditorConfig::default());
    session.editor.edit.caret_byte = source.find("const").expect("const token");
    let mut engine = EditorEngine::new();
    let mut text_system = TextSystem::new();
    let bounds = Rect::new(0.0, 0.0, 320.0, 140.0);

    let unfocused = engine.code_frame(&session, bounds, false, &mut text_system);
    assert!(!unfocused
        .paint_items
        .iter()
        .any(|item| matches!(item, EditorPaintItem::ActiveLine { .. })));

    let focused = engine.code_frame(&session, bounds, true, &mut text_system);
    let (fill_rect, ring_rect) = focused
        .paint_items
        .iter()
        .find_map(|item| match item {
            EditorPaintItem::ActiveLine {
                fill_rect,
                ring_rect,
                ..
            } => Some((*fill_rect, *ring_rect)),
            _ => None,
        })
        .expect("active line");
    let active_run = focused
        .runs
        .iter()
        .find(|run| {
            run.byte_range.start <= session.editor.edit.caret_byte
                && session.editor.edit.caret_byte <= run.byte_range.end
        })
        .expect("active run");

    assert_eq!(active_run.index, 1);
    let active_local = session
        .editor
        .edit
        .caret_byte
        .saturating_sub(active_run.byte_range.start)
        .min(active_run.layout.text().len());
    let caret = active_run.layout.caret_rect_at(active_local, 1.0);
    let text_origin_y = active_run.baseline_y - first_layout_baseline(&active_run.layout);
    let expected_y = focused.viewport.text_origin_y() + text_origin_y + caret.y;
    let expected_h = caret
        .h
        .max(session.editor.config.style.px_size as f32 + 2.0);
    assert_eq!(fill_rect.x, focused.viewport.text_rect.x);
    assert_eq!(fill_rect.w, focused.viewport.text_rect.w);
    assert!(fill_rect.x > focused.viewport.content_rect.x);
    assert!((fill_rect.y - expected_y.round()).abs() <= 0.01);
    assert!((fill_rect.h - expected_h.round()).abs() <= 0.01);
    assert!(ring_rect.x >= focused.viewport.text_rect.x);
    assert!(ring_rect.y >= focused.viewport.text_rect.y);
    assert!(
        ring_rect.x + ring_rect.w <= focused.viewport.text_rect.x + focused.viewport.text_rect.w
    );
    assert!(
        ring_rect.y + ring_rect.h <= focused.viewport.text_rect.y + focused.viewport.text_rect.h
    );
    assert!(ring_rect.y < fill_rect.y);
    assert!(ring_rect.y + ring_rect.h > fill_rect.y + fill_rect.h);
    assert!(ring_rect.h > fill_rect.h);
}

#[test]
fn code_editor_caret_uses_editor_blink_timing() {
    let document = Document::new(
        DocumentId(25),
        TextBuffer::from_string("fn main() {\n    let value = 1;\n}\n"),
    )
    .with_language(EditorLanguage::Rust);
    let mut session = CodeEditorSession::new(document, CodeEditorConfig::default());
    session.editor.edit.caret_byte = session
        .editor
        .buffer
        .as_str()
        .find("value")
        .expect("value marker");
    let mut engine = EditorEngine::new();
    let mut text_system = TextSystem::new();
    let bounds = Rect::new(0.0, 0.0, 320.0, 140.0);

    let on_frame = engine.code_frame_at(&session, bounds, true, 0, &mut text_system);
    assert!(on_frame
        .paint_items
        .iter()
        .any(|item| matches!(item, EditorPaintItem::Caret { .. })));

    let off_frame = engine.code_frame_at(
        &session,
        bounds,
        true,
        session.editor.config.style.caret_blink_ms as u128,
        &mut text_system,
    );
    assert!(!off_frame
        .paint_items
        .iter()
        .any(|item| matches!(item, EditorPaintItem::Caret { .. })));
    assert!(off_frame
        .paint_items
        .iter()
        .any(|item| matches!(item, EditorPaintItem::ActiveLine { .. })));
    assert!(off_frame
        .paint_items
        .iter()
        .any(|item| matches!(item, EditorPaintItem::Text { .. })));
}

#[test]
fn code_frame_active_line_tracks_scroll_and_active_line_number_color() {
    let text: String = (0..80)
        .map(|i| format!("let value_{i:02} = {i};\n"))
        .collect();
    let caret_line = 48usize;
    let caret_marker = format!("value_{caret_line:02}");
    let document = Document::new(DocumentId(23), TextBuffer::from_string(text.clone()))
        .with_language(EditorLanguage::Rust);
    let mut config = CodeEditorConfig {
        theme: CodeTheme {
            active_line_number: ailloli_ui_core::Color::rgb(255, 0, 255),
            ..CodeTheme::default()
        },
        ..CodeEditorConfig::default()
    };
    config.gutter.width = 52.0;
    let mut session = CodeEditorSession::new(document, config);
    session.editor.edit.caret_byte = text.find(&caret_marker).expect("caret marker");
    session.editor.edit.scroll_y = session.editor.config.style.line_height * 46.0;
    session.editor.edit.scroll_x = 36.0;
    let mut engine = EditorEngine::new();
    let mut text_system = TextSystem::new();

    let frame = engine.code_frame(
        &session,
        Rect::new(0.0, 0.0, 340.0, 120.0),
        true,
        &mut text_system,
    );

    let (fill_rect, ring_rect) = frame
        .paint_items
        .iter()
        .find_map(|item| match item {
            EditorPaintItem::ActiveLine {
                fill_rect,
                ring_rect,
                ..
            } => Some((*fill_rect, *ring_rect)),
            _ => None,
        })
        .expect("active line");
    let active_label = (caret_line + 1).to_string();
    let active_label_color = frame
        .paint_items
        .iter()
        .find_map(|item| match item {
            EditorPaintItem::LineNumber { layout, color, .. } if layout.text() == active_label => {
                Some(*color)
            }
            _ => None,
        })
        .expect("active line number");

    assert_eq!(fill_rect.x, frame.viewport.text_rect.x);
    assert_eq!(fill_rect.w, frame.viewport.text_rect.w);
    assert!(
        fill_rect.y >= frame.viewport.text_rect.y
            && fill_rect.y + fill_rect.h <= frame.viewport.text_rect.y + frame.viewport.text_rect.h
    );
    assert!(ring_rect.x >= frame.viewport.text_rect.x);
    assert!(ring_rect.y >= frame.viewport.text_rect.y);
    assert!(ring_rect.x + ring_rect.w <= frame.viewport.text_rect.x + frame.viewport.text_rect.w);
    assert!(ring_rect.y + ring_rect.h <= frame.viewport.text_rect.y + frame.viewport.text_rect.h);
    assert!(ring_rect.y < fill_rect.y);
    assert!(ring_rect.y + ring_rect.h > fill_rect.y + fill_rect.h);
    assert_eq!(active_label_color, session.config.theme.active_line_number);
}

#[test]
fn code_frame_active_line_ring_clamps_at_viewport_edge() {
    let source = "const test: &str = \"test\";\nfn main() {}\n";
    let document = Document::new(DocumentId(24), TextBuffer::from_string(source))
        .with_language(EditorLanguage::Rust);
    let mut session = CodeEditorSession::new(document, CodeEditorConfig::default());
    session.editor.edit.caret_byte = source.find("const").expect("const token");
    let mut engine = EditorEngine::new();
    let mut text_system = TextSystem::new();

    let frame = engine.code_frame(
        &session,
        Rect::new(0.0, 0.0, 320.0, 80.0),
        true,
        &mut text_system,
    );

    let (fill_rect, ring_rect) = frame
        .paint_items
        .iter()
        .find_map(|item| match item {
            EditorPaintItem::ActiveLine {
                fill_rect,
                ring_rect,
                ..
            } => Some((*fill_rect, *ring_rect)),
            _ => None,
        })
        .expect("active line");

    assert!(ring_rect.y >= frame.viewport.text_rect.y);
    assert!(ring_rect.y <= fill_rect.y);
    assert!(ring_rect.h > 0.0);
    assert!(ring_rect.y + ring_rect.h <= frame.viewport.text_rect.y + frame.viewport.text_rect.h);
}

#[test]
fn code_frame_with_disabled_gutter_omits_gutter_items_and_keeps_text_rect() {
    let document = Document::new(
        DocumentId(19),
        TextBuffer::from_string("aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa\n"),
    );
    let mut config = CodeEditorConfig::default();
    config.gutter.enabled = false;
    let mut session = CodeEditorSession::new(document, config);
    session.editor.edit.scroll_x = 24.0;
    let mut engine = EditorEngine::new();
    let mut text_system = TextSystem::new();

    let frame = engine.code_frame(
        &session,
        Rect::new(0.0, 0.0, 180.0, 100.0),
        false,
        &mut text_system,
    );

    assert_eq!(frame.viewport.gutter_rect, None);
    assert_eq!(frame.viewport.text_rect, frame.viewport.content_rect);
    assert!(!frame
        .paint_items
        .iter()
        .any(|item| matches!(item, EditorPaintItem::GutterBackground { .. })));
    assert!(!frame
        .paint_items
        .iter()
        .any(|item| matches!(item, EditorPaintItem::LineNumber { .. })));
    let text_x = frame
        .paint_items
        .iter()
        .find_map(|item| match item {
            EditorPaintItem::Text { pos, .. } => Some(pos[0]),
            _ => None,
        })
        .expect("text item");
    assert_eq!(text_x, frame.viewport.content_rect.x - 24.0);
}

#[test]
fn code_frame_paints_search_and_diagnostic_decorations_inside_text_rect() {
    let document = Document::new(DocumentId(13), TextBuffer::from_string("let value = 1;\n"));
    let mut session = CodeEditorSession::new(document, CodeEditorConfig::default());
    session.set_search_matches(vec![SearchMatch { range: 4..9 }]);
    session.set_diagnostics(vec![Diagnostic::new(
        4..9,
        DiagnosticSeverity::Warning,
        "example",
    )]);
    let mut engine = EditorEngine::new();
    let mut text_system = TextSystem::new();

    let frame = engine.code_frame(
        &session,
        Rect::new(0.0, 0.0, 240.0, 120.0),
        false,
        &mut text_system,
    );

    let search_rect = frame
        .paint_items
        .iter()
        .find_map(|item| match item {
            EditorPaintItem::SearchHighlight { rect, .. } => Some(*rect),
            _ => None,
        })
        .expect("search highlight");
    let diagnostic_rect = frame
        .paint_items
        .iter()
        .find_map(|item| match item {
            EditorPaintItem::DiagnosticUnderline { rect, .. } => Some(*rect),
            _ => None,
        })
        .expect("diagnostic underline");

    assert!(search_rect.x >= frame.viewport.text_rect.x);
    assert!(diagnostic_rect.x >= frame.viewport.text_rect.x);
    assert!(diagnostic_rect.h <= 2.0);
}

#[test]
fn code_frame_maps_multiline_diagnostics_and_gutter_markers_inside_viewport() {
    let source = "let first = 1;\nlet second = first;\nlet third = second;\n";
    let document = Document::new(DocumentId(22), TextBuffer::from_string(source));
    let mut session = CodeEditorSession::new(document, CodeEditorConfig::default());
    session.set_diagnostics(vec![
        Diagnostic::new(4..33, DiagnosticSeverity::Error, "multi-line error"),
        Diagnostic::new(39..44, DiagnosticSeverity::Info, "info"),
    ]);
    session.set_active_diagnostic_index(Some(0));
    let mut engine = EditorEngine::new();
    let mut text_system = TextSystem::new();

    let frame = engine.code_frame(
        &session,
        Rect::new(0.0, 0.0, 280.0, 160.0),
        false,
        &mut text_system,
    );

    let underlines: Vec<_> = frame
        .paint_items
        .iter()
        .filter_map(|item| match item {
            EditorPaintItem::DiagnosticUnderline {
                rect,
                color: _,
                active,
            } => Some((*rect, *active)),
            _ => None,
        })
        .collect();
    let markers: Vec<_> = frame
        .paint_items
        .iter()
        .filter_map(|item| match item {
            EditorPaintItem::DiagnosticGutterMarker { rect, .. } => Some(*rect),
            _ => None,
        })
        .collect();
    let active_backgrounds = frame
        .paint_items
        .iter()
        .filter(|item| matches!(item, EditorPaintItem::SearchHighlight { active: true, .. }))
        .count();

    assert!(
        underlines.len() >= 3,
        "multi-line diagnostic must produce visible underline rects: {underlines:?}"
    );
    assert!(underlines.iter().any(|(_, active)| *active));
    assert!(
        markers.len() >= 2,
        "diagnostics must produce gutter markers for visible runs: {markers:?}"
    );
    assert!(active_backgrounds >= 1);
    assert!(underlines
        .iter()
        .all(|(rect, _)| rect.x >= frame.viewport.text_rect.x));
    assert!(markers.iter().all(|rect| frame
        .viewport
        .gutter_rect
        .is_some_and(|gutter| rect.x >= gutter.x && rect.x + rect.w <= gutter.x + gutter.w)));
}

#[test]
fn code_editor_session_returns_neutral_diagnostic_hit() {
    let document = Document::new(DocumentId(23), TextBuffer::from_string("let value = 1;\n"));
    let mut session = CodeEditorSession::new(document, CodeEditorConfig::default());
    session.set_diagnostics(vec![Diagnostic::new(
        4..9,
        DiagnosticSeverity::Warning,
        "hover me",
    )]);

    let hit = session
        .diagnostic_at_byte(6)
        .expect("diagnostic hit in range");

    assert_eq!(hit.index, 0);
    assert_eq!(hit.diagnostic.message, "hover me");
    assert!(session.diagnostic_at_byte(12).is_none());
}

#[test]
fn code_frame_uses_syntax_tokens_for_single_styled_text_item() {
    let source = "fn main() { let n = 1; // ok\n}\n";
    let document = Document::new(DocumentId(16), TextBuffer::from_string(source))
        .with_language(EditorLanguage::Rust);
    let mut session = CodeEditorSession::new(document, CodeEditorConfig::default());
    session.refresh_syntax_tokens();
    let mut engine = EditorEngine::new();
    let mut text_system = TextSystem::new();

    let frame = engine.code_frame(
        &session,
        Rect::new(0.0, 0.0, 240.0, 120.0),
        false,
        &mut text_system,
    );

    let text_items: Vec<_> = frame
        .paint_items
        .iter()
        .filter_map(|item| match item {
            EditorPaintItem::Text { layout, .. } => Some(layout.clone()),
            _ => None,
        })
        .collect();

    assert_eq!(text_items.len(), frame.runs.len());
    assert_eq!(text_items[0].text(), "fn main() { let n = 1; // ok");
    let styled_glyphs = text_items[0]
        .glyphs()
        .iter()
        .filter_map(|glyph| glyph.color.map(|color| color.as_rgba8()))
        .collect::<std::collections::HashSet<_>>();
    assert!(
        styled_glyphs.len() >= 4,
        "styled glyph colors={styled_glyphs:?}"
    );
}

#[test]
fn code_frame_styled_rust_text_uses_run_baseline() {
    let source = "fn main() {\n    const test: &str = \"test\";\n}\n";
    let document = Document::new(DocumentId(21), TextBuffer::from_string(source))
        .with_language(EditorLanguage::Rust);
    let mut session = CodeEditorSession::new(document, CodeEditorConfig::default());
    session.refresh_syntax_tokens();
    let mut engine = EditorEngine::new();
    let mut text_system = TextSystem::new();

    let frame = engine.code_frame(
        &session,
        Rect::new(0.0, 0.0, 320.0, 140.0),
        false,
        &mut text_system,
    );

    let first_run = frame.runs.first().expect("first run");
    let expected_y = frame.viewport.text_origin_y() + first_run.baseline_y;
    let text_item = frame
        .paint_items
        .iter()
        .find_map(|item| match item {
            EditorPaintItem::Text { pos, layout, .. } if layout.text().starts_with("fn main") => {
                Some((*pos, layout))
            }
            _ => None,
        })
        .expect("styled text item");
    let line_number_y = frame
        .paint_items
        .iter()
        .find_map(|item| match item {
            EditorPaintItem::LineNumber { pos, layout, .. } if layout.text() == "1" => Some(pos[1]),
            _ => None,
        })
        .expect("line 1 number");

    assert!(
        (text_item.0[1] - expected_y).abs() <= 0.01,
        "text y={} expected_y={expected_y}",
        text_item.0[1]
    );
    assert!(
        (line_number_y - expected_y).abs() <= 0.01,
        "line number y={line_number_y} expected_y={expected_y}"
    );
    assert!(
        text_item
            .1
            .glyphs()
            .iter()
            .any(|glyph| glyph.color.is_some()),
        "expected styled glyph colors"
    );
}

#[test]
fn code_frame_without_syntax_tokens_uses_uniform_text_item() {
    let document = Document::new(DocumentId(20), TextBuffer::from_string("plain text\n"));
    let session = CodeEditorSession::new(document, CodeEditorConfig::default());
    let mut engine = EditorEngine::new();
    let mut text_system = TextSystem::new();

    let frame = engine.code_frame(
        &session,
        Rect::new(0.0, 0.0, 240.0, 120.0),
        false,
        &mut text_system,
    );

    let text_layout = frame
        .paint_items
        .iter()
        .find_map(|item| match item {
            EditorPaintItem::Text { layout, .. } => Some(layout),
            _ => None,
        })
        .expect("text layout");
    assert!(text_layout
        .glyphs()
        .iter()
        .all(|glyph| glyph.color.is_none()));
}

#[test]
fn code_scroll_metrics_use_text_rect_not_full_content_rect() {
    let document = Document::new(DocumentId(15), TextBuffer::from_string("hello\n"));
    let mut config = CodeEditorConfig::default();
    config.gutter.width = 48.0;
    let session = CodeEditorSession::new(document, config);
    let mut engine = EditorEngine::new();
    let mut text_system = TextSystem::new();
    let bounds = Rect::new(0.0, 0.0, 240.0, 120.0);

    let frame = engine.code_frame(&session, bounds, false, &mut text_system);
    let metrics = engine.code_scroll_metrics_cached(&session, bounds);
    let caret = engine.code_caret_rect_cached(&session, bounds);

    assert_eq!(metrics.viewport.w, frame.viewport.text_rect.w);
    assert!(metrics.viewport.w < frame.viewport.content_rect.w);
    assert!(caret.x >= frame.viewport.text_rect.x);
}

#[test]
fn code_frame_omits_scrollbars_without_overflow() {
    let document = Document::new(DocumentId(91), TextBuffer::from_string("hello\n"));
    let session = CodeEditorSession::new(document, CodeEditorConfig::default());
    let mut engine = EditorEngine::new();
    let mut text_system = TextSystem::new();

    let frame = engine.code_frame(
        &session,
        Rect::new(0.0, 0.0, 360.0, 160.0),
        false,
        &mut text_system,
    );

    assert!(scrollbar_rects(&frame).is_empty());
}

#[test]
fn code_frame_paints_vertical_scrollbar_inside_text_rect() {
    let text: String = (0..80)
        .map(|idx| format!("let value_{idx} = {idx};\n"))
        .collect();
    let document = Document::new(DocumentId(92), TextBuffer::from_string(text));
    let session = CodeEditorSession::new(document, CodeEditorConfig::default());
    let mut engine = EditorEngine::new();
    let mut text_system = TextSystem::new();

    let frame = engine.code_frame(
        &session,
        Rect::new(0.0, 0.0, 280.0, 120.0),
        false,
        &mut text_system,
    );

    let (track, thumb) = scrollbar_rects(&frame)
        .into_iter()
        .find(|(track, _)| track.h > track.w)
        .expect("vertical scrollbar");
    assert!(frame.viewport.text_rect.contains(track.x, track.y));
    assert!(frame
        .viewport
        .text_rect
        .contains(track.right(), track.bottom()));
    assert!(frame.viewport.text_rect.contains(thumb.x, thumb.y));
    assert!(frame
        .viewport
        .text_rect
        .contains(thumb.right(), thumb.bottom()));
    if let Some(gutter) = frame.viewport.gutter_rect {
        assert!(track.x >= gutter.right());
    }
}

#[test]
fn code_frame_paints_horizontal_scrollbar_only_in_nowrap() {
    let long_line = format!("{}\n", "x".repeat(280));
    let document = Document::new(DocumentId(93), TextBuffer::from_string(long_line.clone()));
    let mut nowrap = CodeEditorSession::new(document, CodeEditorConfig::default());
    nowrap.editor.config.wrap_mode = EditorWrapMode::NoWrap;
    let mut engine = EditorEngine::new();
    let mut text_system = TextSystem::new();

    let nowrap_frame = engine.code_frame(
        &nowrap,
        Rect::new(0.0, 0.0, 220.0, 120.0),
        false,
        &mut text_system,
    );

    let (track, thumb) = scrollbar_rects(&nowrap_frame)
        .into_iter()
        .find(|(track, _)| track.w > track.h)
        .expect("horizontal scrollbar");
    assert!(nowrap_frame.viewport.text_rect.contains(track.x, track.y));
    assert!(nowrap_frame
        .viewport
        .text_rect
        .contains(track.right(), track.bottom()));
    assert!(nowrap_frame.viewport.text_rect.contains(thumb.x, thumb.y));
    assert!(nowrap_frame
        .viewport
        .text_rect
        .contains(thumb.right(), thumb.bottom()));

    let document = Document::new(DocumentId(94), TextBuffer::from_string(long_line));
    let mut soft_wrap = CodeEditorSession::new(document, CodeEditorConfig::default());
    soft_wrap.editor.config.wrap_mode = EditorWrapMode::SoftWrap;
    let soft_wrap_frame = engine.code_frame(
        &soft_wrap,
        Rect::new(0.0, 0.0, 220.0, 120.0),
        false,
        &mut text_system,
    );
    assert!(
        scrollbar_rects(&soft_wrap_frame)
            .into_iter()
            .all(|(track, _)| track.h >= track.w),
        "SoftWrap may need vertical scrolling, but must not paint a horizontal scrollbar"
    );
}

#[test]
fn code_frame_scrollbar_thumbs_follow_scroll_offsets() {
    let text: String = (0..100)
        .map(|idx| format!("let value_{idx} = \"{}\";\n", "x".repeat(220)))
        .collect();
    let document = Document::new(DocumentId(95), TextBuffer::from_string(text.clone()));
    let mut top = CodeEditorSession::new(document, CodeEditorConfig::default());
    top.editor.config.wrap_mode = EditorWrapMode::NoWrap;
    let document = Document::new(DocumentId(96), TextBuffer::from_string(text));
    let mut scrolled = CodeEditorSession::new(document, CodeEditorConfig::default());
    scrolled.editor.config.wrap_mode = EditorWrapMode::NoWrap;
    scrolled.editor.edit.scroll_x = 180.0;
    scrolled.editor.edit.scroll_y = 500.0;
    let mut engine = EditorEngine::new();
    let mut text_system = TextSystem::new();
    let bounds = Rect::new(0.0, 0.0, 260.0, 130.0);

    let top_frame = engine.code_frame(&top, bounds, false, &mut text_system);
    let scrolled_frame = engine.code_frame(&scrolled, bounds, false, &mut text_system);
    let top_rects = scrollbar_rects(&top_frame);
    let scrolled_rects = scrollbar_rects(&scrolled_frame);
    let top_vertical = top_rects
        .iter()
        .find(|(track, _)| track.h > track.w)
        .expect("top vertical")
        .1;
    let scrolled_vertical = scrolled_rects
        .iter()
        .find(|(track, _)| track.h > track.w)
        .expect("scrolled vertical")
        .1;
    let top_horizontal = top_rects
        .iter()
        .find(|(track, _)| track.w > track.h)
        .expect("top horizontal")
        .1;
    let scrolled_horizontal = scrolled_rects
        .iter()
        .find(|(track, _)| track.w > track.h)
        .expect("scrolled horizontal")
        .1;

    assert!(scrolled_vertical.y > top_vertical.y);
    assert!(scrolled_horizontal.x > top_horizontal.x);
}

#[test]
fn code_frame_respects_scrollbar_disable_and_style() {
    let text: String = (0..80)
        .map(|idx| format!("let value_{idx} = {idx};\n"))
        .collect();
    let mut config = CodeEditorConfig::default();
    config.scrollbars.enabled = false;
    let document = Document::new(DocumentId(97), TextBuffer::from_string(text.clone()));
    let session = CodeEditorSession::new(document, config);
    let mut engine = EditorEngine::new();
    let mut text_system = TextSystem::new();
    let bounds = Rect::new(0.0, 0.0, 260.0, 120.0);

    let disabled_frame = engine.code_frame(&session, bounds, false, &mut text_system);
    assert!(scrollbar_rects(&disabled_frame).is_empty());

    let mut config = CodeEditorConfig::default();
    config.scrollbars.style = EditorScrollbarStyle {
        track_color: ailloli_ui_core::Color::rgb(1, 2, 3),
        thumb_color: ailloli_ui_core::Color::rgb(4, 5, 6),
        thickness: 9.0,
        min_thumb_len: 31.0,
        inset: 4.0,
        radius: 2.0,
    };
    let document = Document::new(DocumentId(98), TextBuffer::from_string(text));
    let session = CodeEditorSession::new(document, config);
    let styled_frame = engine.code_frame(&session, bounds, false, &mut text_system);
    assert!(styled_frame.paint_items.iter().any(|item| {
        matches!(
            item,
            EditorPaintItem::Scrollbar {
                track_color,
                thumb_color,
                radius,
                ..
            } if *track_color == config.scrollbars.style.track_color
                && *thumb_color == config.scrollbars.style.thumb_color
                && (*radius - config.scrollbars.style.radius).abs() < 0.001
        )
    }));
}

#[test]
fn code_frame_omits_lines_hidden_by_collapsed_fold_regions() {
    let document = Document::new(
        DocumentId(17),
        TextBuffer::from_string("fn main() {\nlet a = 1;\nlet b = 2;\n}\n"),
    );
    let mut session = CodeEditorSession::new(document, CodeEditorConfig::default());
    session.set_fold_regions(vec![FoldRegion::new(0, 2).collapsed(true)]);
    let mut engine = EditorEngine::new();
    let mut text_system = TextSystem::new();

    let frame = engine.code_frame(
        &session,
        Rect::new(0.0, 0.0, 260.0, 160.0),
        false,
        &mut text_system,
    );

    assert_eq!(frame.painted_paragraphs, vec![0, 3]);
    assert_eq!(frame.runs.len(), 2);
    assert!(frame.content_size.h < 4.0 * session.editor.config.style.line_height);
    let line_numbers = frame
        .paint_items
        .iter()
        .filter(|item| matches!(item, EditorPaintItem::LineNumber { .. }))
        .count();
    assert_eq!(line_numbers, 2);
}

#[test]
fn code_frame_paints_fold_markers_and_placeholder_for_collapsed_region() {
    let document = Document::new(
        DocumentId(24),
        TextBuffer::from_string("fn main() {\nlet a = 1;\nlet b = 2;\n}\nfn next() {}\n"),
    );
    let mut session = CodeEditorSession::new(document, CodeEditorConfig::default());
    session.set_fold_regions(vec![FoldRegion::new(0, 2).collapsed(true)]);
    let mut engine = EditorEngine::new();
    let mut text_system = TextSystem::new();

    let frame = engine.code_frame(
        &session,
        Rect::new(0.0, 0.0, 320.0, 180.0),
        false,
        &mut text_system,
    );

    assert_eq!(frame.painted_paragraphs, vec![0, 3, 4]);
    assert!(frame.paint_items.iter().any(|item| matches!(
        item,
        EditorPaintItem::FoldGutterMarker {
            region_index: 0,
            collapsed: true,
            ..
        }
    )));
    let marker_rect = frame
        .paint_items
        .iter()
        .find_map(|item| match item {
            EditorPaintItem::FoldGutterMarker {
                rect,
                color,
                collapsed,
                ..
            } => Some((*rect, *color, *collapsed)),
            _ => None,
        })
        .expect("fold marker");
    let guide_rect = frame
        .paint_items
        .iter()
        .find_map(|item| match item {
            EditorPaintItem::FoldGutterGuide { rect, color } => Some((*rect, *color)),
            _ => None,
        })
        .expect("fold guide");
    let line_number_pos = frame
        .paint_items
        .iter()
        .find_map(|item| match item {
            EditorPaintItem::LineNumber { pos, layout, .. } => Some((*pos, layout.width())),
            _ => None,
        })
        .expect("line number");
    let gutter = frame.viewport.gutter_rect.expect("gutter");
    assert!(marker_rect.0.x >= gutter.x);
    assert!(marker_rect.0.x + marker_rect.0.w <= gutter.x + gutter.w);
    assert_eq!(marker_rect.1, session.config.theme.fold_marker_active);
    assert!(marker_rect.2);
    assert_eq!(guide_rect.1, session.config.theme.fold_guide);
    assert!(guide_rect.0.h > 0.0);
    assert!(
        line_number_pos.0[0] + line_number_pos.1 <= marker_rect.0.x,
        "line number overlaps fold marker: line={line_number_pos:?} marker={:?}",
        marker_rect.0
    );
    assert!(frame.paint_items.iter().any(|item| matches!(
        item,
        EditorPaintItem::FoldPlaceholder { layout, .. } if layout.text().contains("2 lines folded")
    )));
}

#[test]
fn code_frame_omits_fold_markers_when_gutter_fold_markers_disabled() {
    let document = Document::new(
        DocumentId(2401),
        TextBuffer::from_string("fn main() {\nlet a = 1;\n}\n"),
    );
    let mut config = CodeEditorConfig::default();
    config.gutter.fold_markers = false;
    let mut session = CodeEditorSession::new(document, config);
    session.set_fold_regions(vec![FoldRegion::new(0, 2)]);
    let mut engine = EditorEngine::new();
    let mut text_system = TextSystem::new();

    let frame = engine.code_frame(
        &session,
        Rect::new(0.0, 0.0, 320.0, 180.0),
        false,
        &mut text_system,
    );

    assert!(!frame
        .paint_items
        .iter()
        .any(|item| matches!(item, EditorPaintItem::FoldGutterMarker { .. })));
    assert!(!frame
        .paint_items
        .iter()
        .any(|item| matches!(item, EditorPaintItem::FoldGutterGuide { .. })));
}

#[test]
fn code_editor_session_toggle_fold_moves_caret_out_of_hidden_region() {
    let document = Document::new(
        DocumentId(25),
        TextBuffer::from_string("fn main() {\nlet a = 1;\nlet b = 2;\n}\n"),
    );
    let mut session = CodeEditorSession::new(document, CodeEditorConfig::default());
    session.set_fold_regions(vec![FoldRegion::new(0, 2)]);
    session
        .editor
        .edit
        .set_caret(&session.editor.buffer, "fn main() {\n".len(), false);

    assert!(session.toggle_fold_region(0));

    assert!(session.fold_regions[0].collapsed);
    assert_eq!(session.editor.edit.caret_byte, 0);
}

#[cfg(feature = "tree-sitter")]
#[test]
fn tree_sitter_fold_regions_include_rust_blocks_and_preserve_collapsed_state() {
    let source = "mod app {\nimpl App {\nfn run() {\nlet value = 1;\n}\n}\n}\n";
    let document = Document::new(DocumentId(26), TextBuffer::from_string(source))
        .with_language(EditorLanguage::Rust);
    let mut session = CodeEditorSession::new(document, CodeEditorConfig::default());

    session.refresh_fold_regions();
    assert!(session
        .fold_regions
        .iter()
        .any(|region| region.start_line == 0 && region.end_line == 6));
    assert!(session
        .fold_regions
        .iter()
        .any(|region| region.start_line == 2 && region.end_line == 4));

    let first_id = session.fold_regions[0].id;
    session.fold_regions[0].collapsed = true;
    session.fold_regions_version = None;
    session.refresh_fold_regions();

    assert!(session
        .fold_regions
        .iter()
        .any(|region| region.id == first_id && region.collapsed));
    assert_eq!(
        first_id,
        FoldRegionId::from_lines(
            session.fold_regions[0].start_line,
            session.fold_regions[0].end_line
        )
    );
}

#[cfg(feature = "tree-sitter")]
#[test]
fn tree_sitter_fold_regions_do_not_survive_document_change() {
    let foldable = "fn outer() {\nif true {\nlet value = 1;\n}\n}\n";
    let flat = "let value = 1;\n";
    let first = Document::new(DocumentId(1260), TextBuffer::from_string(foldable))
        .with_language(EditorLanguage::Rust);
    let second = Document::new(DocumentId(1261), TextBuffer::from_string(flat))
        .with_language(EditorLanguage::Rust);
    let mut session = CodeEditorSession::new(first, CodeEditorConfig::default());

    session.refresh_fold_regions();
    assert!(
        !session.fold_regions.is_empty(),
        "first document should produce fold regions"
    );
    session.fold_regions[0].collapsed = true;
    assert_eq!(session.fold_regions_document_id, Some(DocumentId(1260)));
    assert_eq!(session.fold_regions_version, Some(DocumentVersion(0)));

    assert!(session.replace_document_if_changed(second));
    session.refresh_fold_regions();

    assert_eq!(session.fold_regions_document_id, Some(DocumentId(1261)));
    assert_eq!(session.fold_regions_version, Some(DocumentVersion(0)));
    assert!(
        session.fold_regions.is_empty(),
        "fold regions from the previous document must not be reused"
    );
}

#[test]
fn rust_lexical_highlighter_marks_basic_tokens() {
    let tokens = highlight_rust_lexical("fn main() { let n = 1; // ok\n }");

    assert!(tokens
        .iter()
        .any(|token| token.kind == SyntaxKind::Keyword && token.range == (0..2)));
    assert!(tokens
        .iter()
        .any(|token| token.kind == SyntaxKind::Function && token.range == (3..7)));
    assert!(tokens.iter().any(|token| token.kind == SyntaxKind::Number));
    assert!(tokens.iter().any(|token| token.kind == SyntaxKind::Comment));
}

#[test]
fn code_editor_session_refresh_syntax_tokens_reuses_document_version() {
    let source = "fn main() { let n = 1; }\n";
    let document = Document::new(DocumentId(26), TextBuffer::from_string(source))
        .with_language(EditorLanguage::Rust);
    let mut session = CodeEditorSession::new(document, CodeEditorConfig::default());

    session.refresh_syntax_tokens();
    let first = session.syntax_tokens.clone();
    let version = session.syntax_tokens_version;
    session.set_syntax_tokens(vec![SyntaxToken {
        range: 0..2,
        kind: SyntaxKind::Comment,
    }]);
    session.syntax_tokens_document_id = Some(session.document.id);
    session.syntax_tokens_version = version;
    session.syntax_tokens_language = Some(EditorLanguage::Rust);

    session.refresh_syntax_tokens();

    assert_eq!(
        session.syntax_tokens,
        vec![SyntaxToken {
            range: 0..2,
            kind: SyntaxKind::Comment,
        }]
    );
    assert_eq!(session.syntax_tokens_version, version);

    session.document.version = DocumentVersion(session.document.version.0 + 1);
    session.refresh_syntax_tokens();
    assert_eq!(session.syntax_tokens, first);
    assert_eq!(
        session.syntax_tokens_version,
        Some(session.document.version)
    );
}

#[test]
fn code_editor_session_refresh_syntax_tokens_invalidates_on_document_change() {
    let first_source = "// stale comment token\n";
    let second_source = "fn fresh_main() {}\n";
    let first = Document::new(DocumentId(126), TextBuffer::from_string(first_source))
        .with_uri(FileUri::parse("file:///repo/src/first.rs").expect("file uri"));
    let second = Document::new(DocumentId(127), TextBuffer::from_string(second_source))
        .with_uri(FileUri::parse("file:///repo/src/second.rs").expect("file uri"));
    let mut session = CodeEditorSession::new(first, CodeEditorConfig::default());
    session.document.detect_language();

    session.refresh_syntax_tokens();
    assert!(session
        .syntax_tokens
        .iter()
        .any(|token| token.kind == SyntaxKind::Comment));
    assert_eq!(session.syntax_tokens_document_id, Some(DocumentId(126)));
    assert_eq!(session.syntax_tokens_version, Some(DocumentVersion(0)));

    let mut second = second;
    second.detect_language();
    assert_eq!(second.version, DocumentVersion(0));
    assert_eq!(second.language, EditorLanguage::Rust);
    assert!(session.replace_document_if_changed(second));
    session.refresh_syntax_tokens();

    assert_eq!(session.syntax_tokens_document_id, Some(DocumentId(127)));
    assert_eq!(session.syntax_tokens_version, Some(DocumentVersion(0)));
    assert!(session
        .syntax_tokens
        .iter()
        .any(|token| token.kind == SyntaxKind::Function
            && &second_source[token.range.clone()] == "fresh_main"));
    assert!(
        !session
            .syntax_tokens
            .iter()
            .any(|token| token.kind == SyntaxKind::Comment),
        "syntax tokens from the previous document must not survive a document swap"
    );
}

#[test]
fn code_editor_session_refresh_syntax_tokens_invalidates_on_language_change() {
    let source = "fn main() { let n = 1; }\n";
    let document = Document::new(DocumentId(28), TextBuffer::from_string(source))
        .with_language(EditorLanguage::PlainText);
    let mut session = CodeEditorSession::new(document, CodeEditorConfig::default());

    session.refresh_syntax_tokens();
    assert!(session.syntax_tokens.is_empty());
    assert_eq!(
        session.syntax_tokens_version,
        Some(session.document.version)
    );
    assert_eq!(
        session.syntax_tokens_language,
        Some(EditorLanguage::PlainText)
    );

    session.document.language = EditorLanguage::Rust;
    session.refresh_syntax_tokens();

    assert!(session
        .syntax_tokens
        .iter()
        .any(|token| token.kind == SyntaxKind::Function && &source[token.range.clone()] == "main"));
    assert_eq!(
        session.syntax_tokens_version,
        Some(session.document.version)
    );
    assert_eq!(session.syntax_tokens_language, Some(EditorLanguage::Rust));
}

#[cfg(feature = "tree-sitter")]
#[test]
fn tree_sitter_rust_hybrid_highlights_structural_and_gap_tokens() {
    let source = [
        "#[derive(Debug)]",
        "pub struct Parser<'a> {",
        "    source: &'a str,",
        "}",
        "impl<'a> Parser<'a> {",
        "    pub fn parse(&self) -> Result<(), Error> {",
        "        let value = 42;",
        "        println!(\"value = {}\", value);",
        "        let raw = r#\"raw string\"#;",
        "        let ch = 'x';",
        "        // done",
        "        Ok(())",
        "    }",
        "}",
    ]
    .join("\n");

    let first = highlight_rust_tree_sitter_hybrid(&source).expect("tree-sitter tokens");
    let second = highlight_rust_tree_sitter_hybrid(&source).expect("tree-sitter tokens");

    assert_eq!(first, second);
    assert_token(&source, &first, SyntaxKind::Keyword, "pub");
    assert_token(&source, &first, SyntaxKind::Keyword, "#[derive(Debug)]");
    assert_token(&source, &first, SyntaxKind::Type, "Parser");
    assert_token(&source, &first, SyntaxKind::Function, "parse");
    assert_token(&source, &first, SyntaxKind::Function, "println");
    assert_token(&source, &first, SyntaxKind::Identifier, "'a");
    assert_token(&source, &first, SyntaxKind::String, "\"value = {}\"");
    assert_token(&source, &first, SyntaxKind::String, "r#\"raw string\"#");
    assert_token(&source, &first, SyntaxKind::String, "'x'");
    assert_token(&source, &first, SyntaxKind::Number, "42");
    assert_token(&source, &first, SyntaxKind::Comment, "// done");
    assert_token(&source, &first, SyntaxKind::Operator, "->");
    assert_token(&source, &first, SyntaxKind::Punctuation, "{");
    assert!(first.iter().all(|token| {
        token.range.start < token.range.end
            && source.is_char_boundary(token.range.start)
            && source.is_char_boundary(token.range.end)
    }));
}

#[cfg(feature = "tree-sitter")]
#[test]
fn code_editor_session_uses_hybrid_tree_sitter_tokens_when_feature_enabled() {
    let source = "#[derive(Debug)]\npub struct Parser<'a>;\nfn parse() {}\n";
    let document = Document::new(DocumentId(27), TextBuffer::from_string(source))
        .with_language(EditorLanguage::Rust);
    let mut session = CodeEditorSession::new(document, CodeEditorConfig::default());

    session.refresh_syntax_tokens();

    assert_token(
        source,
        &session.syntax_tokens,
        SyntaxKind::Keyword,
        "#[derive(Debug)]",
    );
    assert_token(source, &session.syntax_tokens, SyntaxKind::Type, "Parser");
    assert_token(source, &session.syntax_tokens, SyntaxKind::Identifier, "'a");
    assert_token(
        source,
        &session.syntax_tokens,
        SyntaxKind::Function,
        "parse",
    );
}

#[test]
fn lexical_symbol_indexer_builds_json_summary() {
    let document = Document::new(
        DocumentId(11),
        TextBuffer::from_string(
            "use crate::app;\nstruct App;\nfn run() {}\nfn main() { run(); }\n",
        ),
    )
    .with_language(EditorLanguage::Rust);
    let mut indexer = LexicalRustSymbolIndexer;

    let summary = indexer.index_document(&document);
    let json = summary.to_json_pretty().expect("summary json");

    assert_eq!(summary.symbols.len(), 4);
    assert!(summary
        .symbols
        .iter()
        .any(|symbol| symbol.kind == SymbolKind::Import && symbol.name == "app"));
    assert!(summary
        .symbols
        .iter()
        .any(|symbol| symbol.kind == SymbolKind::Struct && symbol.name == "App"));
    assert!(summary
        .symbols
        .iter()
        .any(|symbol| symbol.kind == SymbolKind::Function && symbol.name == "run"));
    assert!(summary
        .edges
        .iter()
        .any(|edge| edge.kind == SymbolEdgeKind::Imports));
    assert!(summary
        .edges
        .iter()
        .any(|edge| edge.kind == SymbolEdgeKind::Calls));
    assert!(json.contains("\"symbols\""));
    assert!(json.contains("\"Contains\""));
}

#[test]
fn ctags_symbol_indexer_converts_json_lines_to_octav_ir() {
    let document = Document::new(
        DocumentId(12),
        TextBuffer::from_string("struct App;\nfn run() {}\n"),
    )
    .with_language(EditorLanguage::Rust)
    .with_path("src/app.rs");
    let fixture = r#"{"_type":"tag","name":"App","path":"src/app.rs","pattern":"/^struct App;$/","kind":"struct","line":1}
{"_type":"tag","name":"run","path":"src/app.rs","pattern":"/^fn run() {}$/","kind":"function","line":2,"signature":"()"}"#;
    let mut indexer = CtagsSymbolIndexer::from_json_lines(fixture);

    let summary = indexer.index_document(&document);

    assert_eq!(summary.symbols.len(), 2);
    assert_eq!(summary.symbols[0].name, "App");
    assert_eq!(summary.symbols[0].kind, SymbolKind::Struct);
    assert_eq!(summary.symbols[0].source, SymbolSource::Ctags);
    assert_eq!(summary.symbols[0].selection_range, 7..10);
    assert_eq!(summary.symbols[1].name, "run");
    assert_eq!(summary.symbols[1].kind, SymbolKind::Function);
    assert_eq!(summary.symbols[1].signature.as_deref(), Some("()"));
    assert!(summary
        .edges
        .iter()
        .all(|edge| edge.kind == SymbolEdgeKind::Contains));
}

#[test]
fn ctags_symbol_indexer_parses_enriched_json_and_scope_hierarchy() {
    let source = [
        "use crate::fmt::Display;",
        "struct App {",
        "    value: usize,",
        "}",
        "impl App {",
        "    const LIMIT: usize = 42;",
        "    fn run(&self) -> usize { self.value }",
        "}",
        "enum Mode { Fast }",
        "type Output = usize;",
        "macro_rules! trace_value { () => {} }",
    ]
    .join("\n");
    let document = Document::new(DocumentId(16), TextBuffer::from_string(source))
        .with_language(EditorLanguage::Rust)
        .with_path("src/ctags.rs");
    let fixture = r#"{"_type":"tag","name":"Display","path":"src/ctags.rs","kind":"import","line":1,"end":1,"language":"Rust","roles":"imported"}
{"_type":"tag","name":"App","path":"src/ctags.rs","kind":"struct","line":2,"end":4,"signature":"struct App"}
{"_type":"tag","name":"value","path":"src/ctags.rs","kind":"field","line":3,"end":3,"scope":"App","scopeKind":"struct","typeref":"usize"}
{"_type":"tag","name":"LIMIT","path":"src/ctags.rs","kind":"constant","line":6,"end":6,"scope":"App","scopeKind":"struct","typeref":"usize"}
{"_type":"tag","name":"run","path":"src/ctags.rs","kind":"method","line":7,"end":7,"scope":"App","scopeKind":"struct","signature":"(&self) -> usize"}
{"_type":"tag","name":"Mode","path":"src/ctags.rs","kind":"enum","line":9,"end":9}
{"_type":"tag","name":"Fast","path":"src/ctags.rs","kind":"enumerator","line":9,"end":9,"scope":"Mode","scopeKind":"enum"}
{"_type":"tag","name":"Output","path":"src/ctags.rs","kind":"type","line":10,"end":10,"typeref":"usize"}
{"_type":"tag","name":"trace_value","path":"src/ctags.rs","kind":"macro","line":11,"end":11}"#;
    let mut first = CtagsSymbolIndexer::from_json_lines(fixture);
    let mut second = CtagsSymbolIndexer::from_json_lines(fixture);

    let summary = first.try_index_document(&document).expect("ctags fixture");
    let repeated = second.index_document(&document);

    assert_eq!(summary, repeated);
    assert_eq!(summary.symbols.len(), 9);
    let app = symbol_by(&summary, SymbolKind::Struct, "App");
    let value = symbol_by(&summary, SymbolKind::Field, "value");
    let limit = symbol_by(&summary, SymbolKind::Constant, "LIMIT");
    let run = symbol_by(&summary, SymbolKind::Method, "run");
    assert_eq!(value.parent, Some(app.id));
    assert_eq!(limit.parent, Some(app.id));
    assert_eq!(run.parent, Some(app.id));
    assert_eq!(value.signature.as_deref(), Some("usize"));
    assert_eq!(run.signature.as_deref(), Some("(&self) -> usize"));

    let mode = symbol_by(&summary, SymbolKind::Enum, "Mode");
    let fast = symbol_by(&summary, SymbolKind::EnumVariant, "Fast");
    assert_eq!(fast.parent, Some(mode.id));
    assert_eq!(
        symbol_by(&summary, SymbolKind::TypeAlias, "Output")
            .signature
            .as_deref(),
        Some("usize")
    );
    assert_eq!(
        symbol_by(&summary, SymbolKind::Macro, "trace_value").source,
        SymbolSource::Ctags
    );
    assert!(summary
        .edges
        .iter()
        .any(|edge| edge.kind == SymbolEdgeKind::Imports));
    assert!(summary.edges.iter().any(|edge| edge.from == app.id
        && edge.to == run.id
        && edge.kind == SymbolEdgeKind::Contains));
    for symbol in &summary.symbols {
        assert!(symbol.range.end <= document.buffer.len_bytes());
        assert!(symbol.selection_range.end <= document.buffer.len_bytes());
    }
    let json = summary.to_json_pretty().expect("ctags json");
    assert!(json.contains("\"source\": \"Ctags\""));
    assert!(json.contains("\"TypeAlias\""));
    assert!(json.contains("\"EnumVariant\""));
    assert!(json.contains("\"Macro\""));
}

#[test]
fn ctags_runner_errors_are_typed_and_index_document_is_infallible() {
    let document = Document::new(DocumentId(17), TextBuffer::from_string("fn run() {}\n"))
        .with_language(EditorLanguage::Rust)
        .with_path("src/missing.rs");
    let config = CtagsRunnerConfig {
        binary: std::path::PathBuf::from("/definitely/not/ailloli_ui/ctags"),
        ..CtagsRunnerConfig::default()
    };
    let mut indexer = CtagsSymbolIndexer::from_runner_config(config);

    let err = indexer
        .try_index_document(&document)
        .expect_err("missing ctags must be reported");
    assert!(matches!(err, CtagsError::MissingBinary(_)));
    assert!(indexer.index_document(&document).symbols.is_empty());
}

#[test]
fn symbol_fallback_uses_ctags_for_non_tree_sitter_language() {
    let document = Document::new(
        DocumentId(19),
        TextBuffer::from_string("function run() {}\n"),
    )
    .with_language(EditorLanguage::Unknown)
    .with_path("src/app.js");
    let fixture = r#"{"_type":"tag","name":"run","path":"src/app.js","kind":"function","line":1,"end":1,"language":"JavaScript"}"#;
    let mut ctags = CtagsSymbolIndexer::from_json_lines(fixture);

    let summary = index_symbols_with_fallback(&document, &mut ctags);

    assert_eq!(summary.symbols.len(), 1);
    assert_eq!(summary.symbols[0].name, "run");
    assert_eq!(summary.symbols[0].source, SymbolSource::Ctags);
}

#[cfg(unix)]
#[test]
fn ctags_runner_reports_nonzero_timeout_and_output_limit() {
    use std::os::unix::fs::PermissionsExt;

    let document = Document::new(DocumentId(18), TextBuffer::from_string("fn run() {}\n"))
        .with_language(EditorLanguage::Rust)
        .with_path("src/runner.rs");

    let nonzero = temp_script("ctags_nonzero", "echo failed >&2\nexit 7\n");
    let mut indexer = CtagsSymbolIndexer::from_runner_config(CtagsRunnerConfig {
        binary: nonzero.clone(),
        ..CtagsRunnerConfig::default()
    });
    assert!(matches!(
        indexer.try_index_document(&document),
        Err(CtagsError::NonZeroStatus { code: Some(7), .. })
    ));

    let large = temp_script("ctags_large", "printf 'abcdef'\n");
    let mut indexer = CtagsSymbolIndexer::from_runner_config(CtagsRunnerConfig {
        binary: large.clone(),
        max_stdout_bytes: 3,
        ..CtagsRunnerConfig::default()
    });
    assert!(matches!(
        indexer.try_index_document(&document),
        Err(CtagsError::OutputTooLarge { len: 6, max: 3 })
    ));

    let slow = temp_script("ctags_slow", "sleep 1\n");
    let mut indexer = CtagsSymbolIndexer::from_runner_config(CtagsRunnerConfig {
        binary: slow.clone(),
        timeout: std::time::Duration::from_millis(10),
        ..CtagsRunnerConfig::default()
    });
    assert!(matches!(
        indexer.try_index_document(&document),
        Err(CtagsError::Timeout { .. })
    ));

    for path in [nonzero, large, slow] {
        let _ = std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o600));
        let _ = std::fs::remove_file(path);
    }
}

#[cfg(unix)]
fn temp_script(name: &str, body: &str) -> std::path::PathBuf {
    use std::os::unix::fs::PermissionsExt;

    let path = std::env::temp_dir().join(format!(
        "ailloli_ui_{name}_{}_{}.sh",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .expect("system time")
            .as_nanos()
    ));
    std::fs::write(&path, format!("#!/bin/sh\n{body}")).expect("write test script");
    let mut perms = std::fs::metadata(&path)
        .expect("script metadata")
        .permissions();
    perms.set_mode(0o700);
    std::fs::set_permissions(&path, perms).expect("chmod test script");
    path
}

fn symbol_by<'a>(
    summary: &'a ailloli_ui_editor::code::CodeFileSummary,
    kind: SymbolKind,
    name: &str,
) -> &'a ailloli_ui_editor::code::CodeSymbol {
    summary
        .symbols
        .iter()
        .find(|symbol| symbol.kind == kind && symbol.name == name)
        .unwrap_or_else(|| {
            panic!(
                "missing symbol {kind:?} {name}; symbols={:?}",
                summary.symbols
            )
        })
}

#[test]
fn symbol_kind_new_ir_variants_serialize_stably() {
    let variants = [
        SymbolKind::Impl,
        SymbolKind::TypeAlias,
        SymbolKind::EnumVariant,
        SymbolKind::Macro,
    ];

    let json = serde_json::to_string(&variants).expect("serialize symbol kinds");
    let decoded: Vec<SymbolKind> = serde_json::from_str(&json).expect("deserialize symbol kinds");

    assert_eq!(decoded, variants);
    assert!(json.contains("Impl"));
    assert!(json.contains("TypeAlias"));
    assert!(json.contains("EnumVariant"));
    assert!(json.contains("Macro"));
}

#[cfg(feature = "tree-sitter")]
#[test]
fn tree_sitter_rust_symbol_indexer_extracts_rust_symbols() {
    let document = Document::new(
        DocumentId(14),
        TextBuffer::from_string("use crate::app;\nstruct App;\nfn run() {}\n"),
    )
    .with_language(EditorLanguage::Rust);
    let mut indexer = TreeSitterRustSymbolIndexer;

    let summary = indexer.index_document(&document);

    assert!(summary
        .symbols
        .iter()
        .any(|symbol| symbol.kind == SymbolKind::Struct
            && symbol.name == "App"
            && symbol.source == SymbolSource::TreeSitter));
    assert!(summary
        .symbols
        .iter()
        .any(|symbol| symbol.kind == SymbolKind::Function
            && symbol.name == "run"
            && symbol.source == SymbolSource::TreeSitter));
    assert!(summary
        .edges
        .iter()
        .any(|edge| edge.kind == SymbolEdgeKind::Contains));
}

#[cfg(feature = "tree-sitter")]
#[test]
fn tree_sitter_rust_symbol_indexer_builds_complete_octav_ir() {
    let source = r#"//! module docs
use crate::fmt::Display;

/// parser docs
#[doc = "parser attribute docs"]
pub struct Parser<'a> {
    source: &'a str,
    count: usize,
}

pub enum Mode {
    Fast,
    Slow,
}

pub trait Runnable {
    fn run(&self) -> usize;
}

impl<'a> Parser<'a> {
    pub const LIMIT: usize = 42;
    pub static NAME: &str = "parser";
    pub type Output = usize;

    pub fn new(source: &'a str) -> Self {
        Self { source, count: 0 }
    }

    pub fn parse(&self) -> usize {
        helper();
        self.count
    }
}

macro_rules! trace_value {
    ($value:expr) => { println!("{}", $value) };
}

mod nested {
    pub fn helper() {}
}

fn helper() {}
"#;
    let document = Document::new(DocumentId(15), TextBuffer::from_string(source))
        .with_language(EditorLanguage::Rust)
        .with_path("src/complete_symbols.rs");
    let mut first = TreeSitterRustSymbolIndexer;
    let mut second = TreeSitterRustSymbolIndexer;

    let summary = first.index_document(&document);
    let repeated = second.index_document(&document);

    assert_eq!(summary, repeated, "symbol IR must be deterministic");
    assert_eq!(summary.path.as_deref(), document.path.as_deref());
    assert_eq!(summary.language, EditorLanguage::Rust);
    assert_eq!(summary.version, document.version);

    assert_symbol(&summary, SymbolKind::Import, "Display");
    let parser = assert_symbol(&summary, SymbolKind::Struct, "Parser");
    assert!(parser.docs.as_deref().is_some_and(|docs| {
        docs.contains("parser docs") && docs.contains("parser attribute docs")
    }));
    assert!(parser
        .signature
        .as_deref()
        .is_some_and(|signature| { signature.starts_with("pub struct Parser<'a>") }));

    let source_field = assert_symbol(&summary, SymbolKind::Field, "source");
    let count_field = assert_symbol(&summary, SymbolKind::Field, "count");
    assert_eq!(source_field.parent, Some(parser.id));
    assert_eq!(count_field.parent, Some(parser.id));

    let mode = assert_symbol(&summary, SymbolKind::Enum, "Mode");
    let fast = assert_symbol(&summary, SymbolKind::EnumVariant, "Fast");
    let slow = assert_symbol(&summary, SymbolKind::EnumVariant, "Slow");
    assert_eq!(fast.parent, Some(mode.id));
    assert_eq!(slow.parent, Some(mode.id));

    let runnable = assert_symbol(&summary, SymbolKind::Trait, "Runnable");
    let run = assert_symbol(&summary, SymbolKind::Method, "run");
    assert_eq!(run.parent, Some(runnable.id));
    assert!(run
        .signature
        .as_deref()
        .is_some_and(|signature| signature.starts_with("fn run(&self) -> usize")));

    let impl_symbol = summary
        .symbols
        .iter()
        .find(|symbol| symbol.kind == SymbolKind::Impl && symbol.name.contains("Parser"))
        .expect("impl Parser symbol");
    let new_method = assert_symbol(&summary, SymbolKind::Method, "new");
    let parse_method = assert_symbol(&summary, SymbolKind::Method, "parse");
    let limit = assert_symbol(&summary, SymbolKind::Constant, "LIMIT");
    let name = assert_symbol(&summary, SymbolKind::Constant, "NAME");
    let output = assert_symbol(&summary, SymbolKind::TypeAlias, "Output");
    assert_eq!(new_method.parent, Some(impl_symbol.id));
    assert_eq!(parse_method.parent, Some(impl_symbol.id));
    assert_eq!(limit.parent, Some(impl_symbol.id));
    assert_eq!(name.parent, Some(impl_symbol.id));
    assert_eq!(output.parent, Some(impl_symbol.id));
    assert!(new_method
        .signature
        .as_deref()
        .is_some_and(|signature| signature.starts_with("pub fn new(")));
    assert!(output
        .signature
        .as_deref()
        .is_some_and(|signature| signature == "pub type Output = usize;"));

    assert_symbol(&summary, SymbolKind::Macro, "trace_value");
    let nested = assert_symbol(&summary, SymbolKind::Module, "nested");
    let nested_helper = summary
        .symbols
        .iter()
        .find(|symbol| {
            symbol.kind == SymbolKind::Function
                && symbol.name == "helper"
                && symbol.parent == Some(nested.id)
        })
        .expect("nested helper function");
    assert!(nested_helper
        .signature
        .as_deref()
        .is_some_and(|signature| signature == "pub fn helper()"));
    assert!(summary
        .symbols
        .iter()
        .any(|symbol| symbol.kind == SymbolKind::Function
            && symbol.name == "helper"
            && symbol.parent.is_none()));

    for symbol in &summary.symbols {
        assert!(
            symbol.range.start <= symbol.range.end && symbol.range.end <= source.len(),
            "invalid range for {symbol:?}"
        );
        assert!(
            symbol.selection_range.start <= symbol.selection_range.end
                && symbol.selection_range.end <= source.len(),
            "invalid selection range for {symbol:?}"
        );
        assert!(source.is_char_boundary(symbol.range.start));
        assert!(source.is_char_boundary(symbol.range.end));
        assert!(source.is_char_boundary(symbol.selection_range.start));
        assert!(source.is_char_boundary(symbol.selection_range.end));
    }

    for symbol in &summary.symbols {
        let expected_edge_kind = if symbol.kind == SymbolKind::Import {
            SymbolEdgeKind::Imports
        } else {
            SymbolEdgeKind::Contains
        };
        assert!(
            summary
                .edges
                .iter()
                .any(|edge| edge.from == symbol.parent.unwrap_or(SymbolId(0))
                    && edge.to == symbol.id
                    && edge.kind == expected_edge_kind),
            "missing containment/import edge for {symbol:?}"
        );
    }
    assert!(summary
        .edges
        .iter()
        .any(|edge| edge.kind == SymbolEdgeKind::Calls));

    let json = summary.to_json_pretty().expect("stable symbol json");
    assert!(json.contains("\"document_id\""));
    assert!(json.contains("\"path\""));
    assert!(json.contains("\"language\""));
    assert!(json.contains("\"version\""));
    assert!(json.contains("\"symbols\""));
    assert!(json.contains("\"edges\""));
    assert!(json.contains("\"Impl\""));
    assert!(json.contains("\"EnumVariant\""));
    assert!(json.contains("\"TypeAlias\""));
    assert!(json.contains("\"Macro\""));
}

#[cfg(feature = "tree-sitter")]
fn assert_symbol<'a>(
    summary: &'a ailloli_ui_editor::code::CodeFileSummary,
    kind: SymbolKind,
    name: &str,
) -> &'a ailloli_ui_editor::code::CodeSymbol {
    summary
        .symbols
        .iter()
        .find(|symbol| symbol.kind == kind && symbol.name == name)
        .unwrap_or_else(|| {
            panic!(
                "missing symbol {kind:?} {name}; symbols={:?}",
                summary.symbols
            )
        })
}

#[cfg(feature = "tree-sitter")]
#[test]
fn tree_sitter_rust_symbol_graph_uses_callers_and_ignores_false_positives() {
    let source = r##"use crate::runtime::build;

pub struct Parser;

impl Parser {
    pub fn parse(&self) {
        helper();
        build();
        nested::helper();
        missing_macro!();
        let _text = "helper() build()";
        let _raw = r#"nested::helper()"#;
        // helper()
    }
}

mod nested {
    pub fn caller() {
        helper();
    }

    pub fn helper() {}
}

pub fn helper() {}
pub fn build() {}
"##;
    let document = Document::new(DocumentId(19), TextBuffer::from_string(source))
        .with_language(EditorLanguage::Rust)
        .with_path("src/symbol_graph.rs");
    let mut first = TreeSitterRustSymbolIndexer;
    let mut second = TreeSitterRustSymbolIndexer;

    let summary = first.index_document(&document);
    let repeated = second.index_document(&document);

    assert_eq!(summary, repeated, "symbol graph must be deterministic");
    assert_eq!(
        summary.to_json_pretty().expect("json"),
        repeated.to_json_pretty().expect("json")
    );

    let parser = assert_symbol(&summary, SymbolKind::Struct, "Parser");
    let impl_parser = summary
        .symbols
        .iter()
        .find(|symbol| symbol.kind == SymbolKind::Impl && symbol.name.contains("Parser"))
        .expect("impl Parser");
    let parse = summary
        .symbols
        .iter()
        .find(|symbol| {
            symbol.kind == SymbolKind::Method
                && symbol.name == "parse"
                && symbol.parent == Some(impl_parser.id)
        })
        .expect("parse method");
    let nested = assert_symbol(&summary, SymbolKind::Module, "nested");
    let nested_caller = summary
        .symbols
        .iter()
        .find(|symbol| {
            symbol.kind == SymbolKind::Function
                && symbol.name == "caller"
                && symbol.parent == Some(nested.id)
        })
        .expect("nested caller");
    let nested_helper = summary
        .symbols
        .iter()
        .find(|symbol| {
            symbol.kind == SymbolKind::Function
                && symbol.name == "helper"
                && symbol.parent == Some(nested.id)
        })
        .expect("nested helper");
    let root_helper = summary
        .symbols
        .iter()
        .find(|symbol| {
            symbol.kind == SymbolKind::Function
                && symbol.name == "helper"
                && symbol.parent.is_none()
        })
        .expect("root helper");
    let build = assert_symbol(&summary, SymbolKind::Function, "build");

    assert_graph_edge(&summary, SymbolId(0), parser.id, SymbolEdgeKind::Contains);
    assert_graph_edge(
        &summary,
        SymbolId(0),
        impl_parser.id,
        SymbolEdgeKind::Contains,
    );
    assert_graph_edge(&summary, impl_parser.id, parse.id, SymbolEdgeKind::Contains);
    assert_graph_edge(
        &summary,
        nested.id,
        nested_helper.id,
        SymbolEdgeKind::Contains,
    );

    assert_graph_edge(&summary, parse.id, root_helper.id, SymbolEdgeKind::Calls);
    assert_graph_edge(&summary, parse.id, build.id, SymbolEdgeKind::Calls);
    assert_graph_edge(&summary, parse.id, nested_helper.id, SymbolEdgeKind::Calls);
    assert_graph_edge(
        &summary,
        nested_caller.id,
        nested_helper.id,
        SymbolEdgeKind::Calls,
    );

    assert!(!summary
        .edges
        .iter()
        .any(|edge| edge.kind == SymbolEdgeKind::Calls && edge.from == SymbolId(0)));
    assert!(!summary.edges.iter().any(|edge| {
        edge.kind == SymbolEdgeKind::Calls && edge.from == parse.id && edge.to == parse.id
    }));

    let call_edges: Vec<_> = summary
        .edges
        .iter()
        .filter(|edge| edge.kind == SymbolEdgeKind::Calls)
        .collect();
    assert_eq!(
        call_edges.len(),
        4,
        "strings, comments and unresolved macros must not create calls: {:?}",
        summary.edges
    );
}

#[cfg(feature = "tree-sitter")]
fn assert_graph_edge(
    summary: &ailloli_ui_editor::code::CodeFileSummary,
    from: SymbolId,
    to: SymbolId,
    kind: SymbolEdgeKind,
) {
    assert!(
        summary
            .edges
            .iter()
            .any(|edge| edge.from == from && edge.to == to && edge.kind == kind),
        "missing edge {from:?} -> {to:?} {kind:?}; edges={:?}",
        summary.edges
    );
}

#[test]
fn code_search_finds_case_sensitive_and_insensitive_matches() {
    let text = "Foo foo FOO";

    let insensitive = find_matches(text, &SearchQuery::new("foo"));
    let sensitive = find_matches(text, &SearchQuery::new("foo").case_sensitive(true));

    assert_eq!(
        insensitive,
        vec![
            SearchMatch { range: 0..3 },
            SearchMatch { range: 4..7 },
            SearchMatch { range: 8..11 },
        ]
    );
    assert_eq!(sensitive, vec![SearchMatch { range: 4..7 }]);
}

#[test]
fn code_search_query_empty_whole_word_and_ascii_case_rules() {
    assert!(find_matches("value Value", &SearchQuery::new("")).is_empty());

    let whole_word = find_matches(
        "value value_ value2 prevalue value",
        &SearchQuery::new("value").whole_word(true),
    );
    assert_eq!(
        whole_word,
        vec![SearchMatch { range: 0..5 }, SearchMatch { range: 29..34 }]
    );

    let ascii_only = find_matches("é É e E", &SearchQuery::new("é"));
    assert_eq!(ascii_only, vec![SearchMatch { range: 0..2 }]);
}

#[test]
fn code_editor_session_search_caches_and_navigates_matches() {
    let document = Document::new(
        DocumentId(20),
        TextBuffer::from_string("value other value\nvalue\n"),
    );
    let mut session = CodeEditorSession::new(document, CodeEditorConfig::default());

    session.set_search_query(SearchQuery::new("value"));
    assert_eq!(session.search.matches.len(), 3);
    assert_eq!(session.search.active_index, Some(0));
    assert!(
        !session.refresh_search(),
        "unchanged cache key must be reused"
    );

    session.search_next();
    assert_eq!(session.search.active_index, Some(1));
    session.search_next();
    session.search_next();
    assert_eq!(session.search.active_index, Some(0));
    session.search_previous();
    assert_eq!(session.search.active_index, Some(2));

    session.set_search_query(SearchQuery::new("missing"));
    assert!(session.search.matches.is_empty());
    assert_eq!(session.search.active_index, None);

    session.editor.buffer = TextBuffer::from_string("missing value\n");
    assert!(session.sync_document_from_editor());
    assert!(
        session.refresh_search(),
        "document version must invalidate search cache"
    );
    assert_eq!(session.search.matches.len(), 1);

    session.clear_search();
    assert!(session.search.matches.is_empty());
    assert_eq!(session.search.query.text, "");
}

#[test]
fn code_search_case_insensitive_preserves_original_byte_ranges() {
    let text = "é Foo";

    let matches = find_matches(text, &SearchQuery::new("foo"));

    assert_eq!(matches, vec![SearchMatch { range: 3..6 }]);
}

#[test]
fn code_frame_paints_active_search_match_with_distinct_color_inside_text_rect() {
    let document = Document::new(
        DocumentId(21),
        TextBuffer::from_string("value other value\n"),
    );
    let mut session = CodeEditorSession::new(document, CodeEditorConfig::default());
    session.set_search_query(SearchQuery::new("value"));
    session.set_search_active_index(Some(1));
    let mut engine = EditorEngine::new();
    let mut text_system = TextSystem::new();

    let frame = engine.code_frame(
        &session,
        Rect::new(0.0, 0.0, 260.0, 120.0),
        false,
        &mut text_system,
    );

    let highlights: Vec<_> = frame
        .paint_items
        .iter()
        .filter_map(|item| match item {
            EditorPaintItem::SearchHighlight {
                rect,
                color,
                active,
            } => Some((*rect, *color, *active)),
            _ => None,
        })
        .collect();
    assert_eq!(highlights.len(), 2);
    assert_eq!(
        highlights.iter().filter(|(_, _, active)| *active).count(),
        1
    );
    assert_ne!(highlights[0].1, highlights[1].1);
    assert!(highlights
        .iter()
        .all(|(rect, _, _)| rect.x >= frame.viewport.text_rect.x));
}

#[test]
fn diagnostics_and_fold_regions_are_plain_code_models() {
    let diagnostic = Diagnostic::new(3..8, DiagnosticSeverity::Warning, "unused value");
    let fold = FoldRegion::new(2, 5).collapsed(true);

    assert_eq!(diagnostic.range, 3..8);
    assert!(fold.hides_line(3));
    assert!(!fold.hides_line(2));
    assert!(!fold.hides_line(6));
}

#[test]
fn semantic_lsp_and_scip_mappers_enrich_octav_ir_without_being_primary_indexers() {
    let document = Document::new(DocumentId(18), TextBuffer::from_string("fn run() {}\n"))
        .with_language(EditorLanguage::Rust);
    let symbols = vec![SemanticDocumentSymbol {
        name: "run".into(),
        kind: SymbolKind::Function,
        range: 0..11,
        selection_range: 3..6,
        detail: Some("fn run()".into()),
        source: SymbolSource::Lsp,
    }];
    let lsp_symbols = ailloli_ui_editor::code::lsp_symbols_to_code_symbols(&document, &symbols);
    let scip_symbols = ailloli_ui_editor::code::scip_symbols_to_code_symbols(&document, &symbols);
    let edges = ailloli_ui_editor::code::semantic_references_to_edges(&[SemanticReference {
        from: SymbolId(1),
        to: SymbolId(2),
        kind: SymbolEdgeKind::References,
        source: SymbolSource::Scip,
    }]);

    assert_eq!(lsp_symbols[0].source, SymbolSource::Lsp);
    assert_eq!(scip_symbols[0].source, SymbolSource::Scip);
    assert_eq!(lsp_symbols[0].signature.as_deref(), Some("fn run()"));
    assert_eq!(edges[0].kind, SymbolEdgeKind::References);
}

#[derive(Default)]
struct MockLspBackend {
    opened: bool,
    changed: bool,
    closed: bool,
    diagnostics: Vec<LspDiagnostic>,
    symbols: Vec<SemanticDocumentSymbol>,
    references: Vec<SemanticReference>,
}

impl LspBackend for MockLspBackend {
    fn capabilities(&self) -> LspCapabilities {
        LspCapabilities {
            document_symbols: true,
            references: true,
            diagnostics: true,
            ..LspCapabilities::default()
        }
    }

    fn open_document(&mut self, _document: &Document) -> Result<(), LspError> {
        self.opened = true;
        Ok(())
    }

    fn change_document(&mut self, _document: &Document) -> Result<(), LspError> {
        self.changed = true;
        Ok(())
    }

    fn close_document(&mut self, _document: &Document) -> Result<(), LspError> {
        self.closed = true;
        Ok(())
    }

    fn document_symbols(
        &mut self,
        _document: &Document,
    ) -> Result<Vec<SemanticDocumentSymbol>, LspError> {
        Ok(self.symbols.clone())
    }

    fn references(&mut self, _document: &Document) -> Result<Vec<SemanticReference>, LspError> {
        Ok(self.references.clone())
    }

    fn diagnostics(&mut self, _document: &Document) -> Result<Vec<LspDiagnostic>, LspError> {
        Ok(self.diagnostics.clone())
    }
}

#[test]
fn lsp_backend_capabilities_lifecycle_and_absent_backend_are_ui_agnostic() {
    let document = Document::new(DocumentId(54), TextBuffer::from_string("fn run() {}\n"))
        .with_language(EditorLanguage::Rust);
    let mut backend = MockLspBackend::default();

    backend.open_document(&document).expect("open document");
    backend.change_document(&document).expect("change document");
    backend.close_document(&document).expect("close document");

    assert!(backend.opened);
    assert!(backend.changed);
    assert!(backend.closed);

    let mut noop = NoopLspBackend;
    assert_eq!(noop.capabilities(), LspCapabilities::default());
    assert!(matches!(
        noop.document_symbols(&document),
        Err(LspError::CapabilityUnavailable("document_symbols"))
    ));
    assert!(matches!(
        noop.cancel(LspRequestId(7)),
        Err(LspError::RequestCancelled(LspRequestId(7)))
    ));
}

#[test]
fn lsp_enrichment_filters_stale_diagnostics_and_maps_symbols_references() {
    let document = Document::new(DocumentId(55), TextBuffer::from_string("fn run() {}\n"))
        .with_language(EditorLanguage::Rust);
    let stale_version = DocumentVersion(document.version.0.saturating_sub(1));
    let mut backend = MockLspBackend {
        diagnostics: vec![
            LspDiagnostic {
                range: 3..6,
                severity: DiagnosticSeverity::Warning,
                message: "current".into(),
                document_version: document.version,
            },
            LspDiagnostic {
                range: 0..2,
                severity: DiagnosticSeverity::Error,
                message: "stale".into(),
                document_version: stale_version,
            },
        ],
        symbols: vec![SemanticDocumentSymbol {
            name: "run".into(),
            kind: SymbolKind::Function,
            range: 0..11,
            selection_range: 3..6,
            detail: Some("fn run()".into()),
            source: SymbolSource::Lsp,
        }],
        references: vec![SemanticReference {
            from: SymbolId(1),
            to: SymbolId(1),
            kind: SymbolEdgeKind::References,
            source: SymbolSource::Lsp,
        }],
        ..MockLspBackend::default()
    };

    let enrichment = ailloli_ui_editor::code::collect_lsp_enrichment(&mut backend, &document)
        .expect("enrichment");

    assert!(enrichment.capabilities.document_symbols);
    assert_eq!(enrichment.symbols.len(), 1);
    assert_eq!(enrichment.references.len(), 1);
    assert_eq!(enrichment.diagnostics.len(), 1);
    assert_eq!(enrichment.diagnostics[0].message, "current");
    assert_eq!(enrichment.diagnostics[0].source, DiagnosticSource::Lsp);
    assert_eq!(
        enrichment.diagnostics[0].document_version,
        Some(document.version)
    );

    let mut session = CodeEditorSession::new(document, CodeEditorConfig::default());
    assert!(session.apply_lsp_enrichment(&enrichment));
    assert_eq!(session.diagnostics.len(), 1);
}

#[test]
fn code_editor_session_merges_lsp_diagnostics_without_stale_versions() {
    let document = Document::new(DocumentId(56), TextBuffer::from_string("fn run() {}\n"))
        .with_language(EditorLanguage::Rust);
    let mut session = CodeEditorSession::new(document.clone(), CodeEditorConfig::default());
    session.set_diagnostics(vec![Diagnostic::new(
        0..2,
        DiagnosticSeverity::Hint,
        "local",
    )]);

    let changed = session.apply_lsp_diagnostics(&[
        LspDiagnostic {
            range: 3..6,
            severity: DiagnosticSeverity::Info,
            message: "lsp current".into(),
            document_version: document.version,
        },
        LspDiagnostic {
            range: 0..2,
            severity: DiagnosticSeverity::Error,
            message: "lsp stale".into(),
            document_version: DocumentVersion(document.version.0.saturating_sub(1)),
        },
    ]);

    assert!(changed);
    assert_eq!(session.diagnostics.len(), 2);
    assert!(session
        .diagnostics
        .iter()
        .any(|diagnostic| diagnostic.source == DiagnosticSource::Local));
    assert!(session
        .diagnostics
        .iter()
        .any(|diagnostic| diagnostic.source == DiagnosticSource::Lsp
            && diagnostic.message == "lsp current"));
    assert!(!session
        .diagnostics
        .iter()
        .any(|diagnostic| diagnostic.message == "lsp stale"));
}

#[test]
fn scip_importer_maps_project_symbols_occurrences_and_cross_file_navigation() {
    let fixture = r#"{
        "metadata": { "project_root": "/repo", "tool_info": "mock-scip" },
        "documents": [
            {
                "path": "src/lib.rs",
                "language": "Rust",
                "version": 1,
                "symbols": [
                    {
                        "symbol": "local src/lib.rs main().",
                        "name": "main",
                        "kind": "Function",
                        "range": { "start": 0, "end": 11 },
                        "selection_range": { "start": 3, "end": 7 },
                        "signature": "fn main()",
                        "docs": null
                    }
                ],
                "occurrences": [
                    {
                        "symbol": "local src/lib.rs main().",
                        "range": { "start": 3, "end": 7 },
                        "role": "Definition"
                    },
                    {
                        "symbol": "local src/helper.rs helper().",
                        "range": { "start": 16, "end": 22 },
                        "role": "Reference"
                    }
                ],
                "relations": [
                    {
                        "from_symbol": "local src/lib.rs main().",
                        "to_symbol": "local src/helper.rs helper().",
                        "kind": "References"
                    }
                ]
            },
            {
                "path": "src/helper.rs",
                "language": "Rust",
                "version": 1,
                "symbols": [
                    {
                        "symbol": "local src/helper.rs helper().",
                        "name": "helper",
                        "kind": "Function",
                        "range": { "start": 0, "end": 13 },
                        "selection_range": { "start": 3, "end": 9 },
                        "signature": "fn helper()",
                        "docs": "shared helper"
                    }
                ],
                "occurrences": [
                    {
                        "symbol": "local src/helper.rs helper().",
                        "range": { "start": 3, "end": 9 },
                        "role": "Definition"
                    }
                ],
                "relations": []
            }
        ]
    }"#;

    let index = ailloli_ui_editor::code::import_scip_json_str(fixture).expect("scip fixture");
    let project = ailloli_ui_editor::code::scip_project_to_summary(&index);

    assert_eq!(project.metadata.tool_info, "mock-scip");
    assert_eq!(project.documents.len(), 2);
    assert_eq!(project.documents[0].symbols[0].source, SymbolSource::Scip);
    assert_eq!(project.documents[0].symbols[0].id, SymbolId(1));
    assert_eq!(project.navigation.len(), 1);
    assert_eq!(project.navigation[0].from_path, "src/lib.rs");
    assert_eq!(project.navigation[0].to_path, "src/helper.rs");
    assert_eq!(project.navigation[0].kind, SymbolEdgeKind::References);
    assert!(matches!(
        index.documents[0].occurrences[0].role,
        ScipOccurrenceRole::Definition
    ));
}

#[test]
fn merge_code_file_summaries_prefers_stronger_sources_and_keeps_edges() {
    let tree_summary = ailloli_ui_editor::code::CodeFileSummary {
        document_id: DocumentId(60),
        path: Some("src/lib.rs".into()),
        language: EditorLanguage::Rust,
        version: DocumentVersion(1),
        symbols: vec![ailloli_ui_editor::code::CodeSymbol {
            id: SymbolId(1),
            name: "run".into(),
            kind: SymbolKind::Function,
            language: EditorLanguage::Rust,
            range: 0..11,
            selection_range: 3..6,
            parent: None,
            signature: Some("fn run()".into()),
            docs: None,
            source: SymbolSource::TreeSitter,
        }],
        edges: vec![ailloli_ui_editor::code::SymbolEdge {
            from: SymbolId(0),
            to: SymbolId(1),
            kind: SymbolEdgeKind::Contains,
        }],
    };
    let scip_summary = ailloli_ui_editor::code::CodeFileSummary {
        symbols: vec![ailloli_ui_editor::code::CodeSymbol {
            source: SymbolSource::Scip,
            signature: Some("fn run() -> ()".into()),
            ..tree_summary.symbols[0].clone()
        }],
        ..tree_summary.clone()
    };

    let merged = ailloli_ui_editor::code::merge_code_file_summaries(
        DocumentId(60),
        Some("src/lib.rs".into()),
        EditorLanguage::Rust,
        DocumentVersion(1),
        &[scip_summary, tree_summary],
    );

    assert_eq!(merged.symbols.len(), 1);
    assert_eq!(merged.symbols[0].source, SymbolSource::TreeSitter);
    assert_eq!(merged.edges.len(), 1);
}
