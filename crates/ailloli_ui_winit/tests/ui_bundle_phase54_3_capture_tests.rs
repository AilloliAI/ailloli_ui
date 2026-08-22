//! Local-only Phase 54.3 visual tests for CodeEditor startup debt.
//!
//! ```sh
//! cargo test -p ailloli_ui_winit --test ui_bundle_phase54_3_capture_tests -- --ignored --nocapture
//! ```

use std::collections::HashSet;
use std::path::PathBuf;

use ailloli_ui::prelude::*;
use ailloli_ui::{App, Window};
use ailloli_ui_render_wgpu::CapturedFrame;

#[allow(dead_code)]
#[path = "../examples/support/ui_bundle_showcase.rs"]
/// Reuses the deterministic gallery builder exercised by the executable example.
mod ui_bundle_showcase;

use ui_bundle_showcase::{
    ui_bundle_code_editor_phase54_3_active_line_showcase,
    ui_bundle_code_editor_phase54_3_baseline_showcase,
    ui_bundle_code_editor_phase54_3_ctags_fallback_showcase,
    ui_bundle_code_editor_phase54_3_diagnostics_showcase,
    ui_bundle_code_editor_phase54_3_extension_detection_showcase,
    ui_bundle_code_editor_phase54_3_folding_showcase,
    ui_bundle_code_editor_phase54_3_ide_folding_gutter_showcase,
    ui_bundle_code_editor_phase54_3_large_file_showcase,
    ui_bundle_code_editor_phase54_3_lsp_showcase,
    ui_bundle_code_editor_phase54_3_multiclick_selection_showcase,
    ui_bundle_code_editor_phase54_3_scip_showcase, ui_bundle_code_editor_phase54_3_search_showcase,
    ui_bundle_code_editor_phase54_3_showcase,
    ui_bundle_code_editor_phase54_3_symbol_graph_showcase,
    ui_bundle_code_editor_phase54_3_symbol_outline_showcase,
    ui_bundle_code_editor_phase54_3_theme_variants_showcase,
    ui_bundle_code_editor_phase54_3_tree_sitter_showcase, ShowcaseMode,
};

/// Resolves the repository-local directory used for diagnostic captures.
fn repo_captures_dir() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../..")
        .join("artifacts")
        .join("captures")
}

/// Counts RGBA8 pixels accepted by `pred`; trailing incomplete bytes are ignored.
fn count_pixels(rgba: &[u8], pred: impl Fn([u8; 4]) -> bool) -> u64 {
    rgba.chunks_exact(4)
        .filter(|px| pred([px[0], px[1], px[2], px[3]]))
        .count() as u64
}

/// Verifies MVP-debt editor gutters, syntax colors, and encoded capture data.
fn assert_code_editor_phase54_3_frame(frame: &CapturedFrame, filename: &str) {
    let png = frame.png_data.as_ref().expect("png data");
    assert!(!png.is_empty(), "{filename}: empty png");
    assert!(frame.width > 500, "{filename}: width={}", frame.width);
    assert!(frame.height > 180, "{filename}: height={}", frame.height);

    let distinct = frame
        .rgba
        .chunks_exact(4)
        .step_by(32)
        .map(|px| [px[0], px[1], px[2], px[3]])
        .collect::<HashSet<_>>()
        .len();
    assert!(
        distinct > 16,
        "{filename}: distinct sampled colors={distinct}"
    );

    let dark_surface = count_pixels(&frame.rgba, |px| px[0] < 40 && px[1] < 45 && px[2] < 55);
    let text = count_pixels(&frame.rgba, |px| px[0] > 145 && px[1] > 145 && px[2] > 145);
    let gutter_gray = count_pixels(&frame.rgba, |px| {
        px[0] >= 70 && px[0] <= 150 && px[1] >= 70 && px[1] <= 150 && px[2] >= 70 && px[2] <= 170
    });
    let accent_or_syntax = count_pixels(&frame.rgba, |px| {
        (px[0] > 110 && px[1] > 120 && px[2] > 170) || (px[0] > 170 && px[1] > 110 && px[2] < 120)
    });

    assert!(
        dark_surface > 500,
        "{filename}: dark editor pixels={dark_surface}"
    );
    assert!(text > 160, "{filename}: text-ish pixels={text}");
    assert!(
        gutter_gray > 30,
        "{filename}: gutter-ish pixels={gutter_gray}"
    );
    assert!(
        accent_or_syntax > 25,
        "{filename}: syntax/accent pixels={accent_or_syntax}"
    );
}

/// Writes a frame's required PNG payload beneath the repository captures directory.
fn write_capture(name: &str, frame: &CapturedFrame) {
    let out_dir = repo_captures_dir();
    std::fs::create_dir_all(&out_dir).expect("mkdir captures");
    std::fs::write(
        out_dir.join(name),
        frame.png_data.as_ref().expect("png data"),
    )
    .expect("write capture");
}

#[test]
#[ignore]
fn ui_bundle_phase54_3_mvp_debt_gutter_scroll_capture() {
    let cap = CaptureHandle::new();
    cap.set_exit_after_all_captures(true);
    let mvp_id = cap.request_element("code-editor-phase54-3", "section-code-editor-phase54-3");
    let styled_id = cap.request_element(
        "code-editor-phase54-3-styled",
        "section-code-editor-phase54-3",
    );
    let baseline_id = cap.request_element(
        "code-editor-phase54-3-baseline",
        "section-code-editor-phase54-3-baseline",
    );
    let active_line_id = cap.request_element(
        "code-editor-phase54-3-active-line",
        "section-code-editor-phase54-3-active-line",
    );
    let tree_sitter_id = cap.request_element(
        "code-editor-phase54-3-tree-sitter",
        "section-code-editor-phase54-3-tree-sitter",
    );
    let extension_detection_id = cap.request_element(
        "code-editor-phase54-3-extension-detection",
        "section-code-editor-phase54-3-extension-detection",
    );
    let symbol_outline_id = cap.request_element(
        "code-editor-phase54-3-symbol-outline",
        "section-code-editor-phase54-3-symbol-outline",
    );
    let ctags_fallback_id = cap.request_element(
        "code-editor-phase54-3-ctags-fallback",
        "section-code-editor-phase54-3-ctags-fallback",
    );
    let symbol_graph_id = cap.request_element(
        "code-editor-phase54-3-symbol-graph",
        "section-code-editor-phase54-3-symbol-graph",
    );
    let search_id = cap.request_element(
        "code-editor-phase54-3-search",
        "section-code-editor-phase54-3-search",
    );
    let multiclick_selection_id = cap.request_element(
        "code-editor-phase54-3-multiclick-selection",
        "section-code-editor-phase54-3-multiclick-selection",
    );
    let diagnostics_id = cap.request_element(
        "code-editor-phase54-3-diagnostics",
        "section-code-editor-phase54-3-diagnostics",
    );
    let folding_id = cap.request_element(
        "code-editor-phase54-3-folding",
        "section-code-editor-phase54-3-folding",
    );
    let ide_folding_gutter_id = cap.request_element(
        "code-editor-phase54-3-ide-folding-gutter",
        "section-code-editor-phase54-3-ide-folding-gutter",
    );
    let lsp_id = cap.request_element(
        "code-editor-phase54-3-lsp",
        "section-code-editor-phase54-3-lsp",
    );
    let scip_id = cap.request_element(
        "code-editor-phase54-3-scip",
        "section-code-editor-phase54-3-scip",
    );
    let large_file_id = cap.request_element(
        "code-editor-phase54-3-large-file",
        "section-code-editor-phase54-3-large-file",
    );
    let theme_variants_id = cap.request_element(
        "code-editor-phase54-3-theme-variants",
        "section-code-editor-phase54-3-theme-variants",
    );

    App::new()
        .window(
            Window::new("code-editor-phase54-3")
                .title_text("ui_bundle_phase54_3_mvp_debt_gutter_scroll")
                .no_chrome()
                .size(1280.0, 900.0)
                .content(|| ui_bundle_code_editor_phase54_3_showcase(ShowcaseMode::DefaultTheme)),
        )
        .window(
            Window::new("code-editor-phase54-3-styled")
                .title_text("ui_bundle_phase54_3_styled_spans")
                .no_chrome()
                .size(1280.0, 900.0)
                .content(|| ui_bundle_code_editor_phase54_3_showcase(ShowcaseMode::DefaultTheme)),
        )
        .window(
            Window::new("code-editor-phase54-3-baseline")
                .title_text("ui_bundle_phase54_3_styled_baseline_alignment")
                .no_chrome()
                .size(1280.0, 900.0)
                .content(|| {
                    ui_bundle_code_editor_phase54_3_baseline_showcase(ShowcaseMode::DefaultTheme)
                }),
        )
        .window(
            Window::new("code-editor-phase54-3-active-line")
                .title_text("ui_bundle_phase54_3_active_line_ring")
                .no_chrome()
                .size(1280.0, 900.0)
                .content(|| {
                    ui_bundle_code_editor_phase54_3_active_line_showcase(ShowcaseMode::DefaultTheme)
                }),
        )
        .window(
            Window::new("code-editor-phase54-3-tree-sitter")
                .title_text("ui_bundle_phase54_3_tree_sitter_rust_tokens")
                .no_chrome()
                .size(1280.0, 900.0)
                .content(|| {
                    ui_bundle_code_editor_phase54_3_tree_sitter_showcase(ShowcaseMode::DefaultTheme)
                }),
        )
        .window(
            Window::new("code-editor-phase54-3-extension-detection")
                .title_text("ui_bundle_phase54_3_extension_language_detection")
                .no_chrome()
                .size(1280.0, 900.0)
                .content(|| {
                    ui_bundle_code_editor_phase54_3_extension_detection_showcase(
                        ShowcaseMode::DefaultTheme,
                    )
                }),
        )
        .window(
            Window::new("code-editor-phase54-3-symbol-outline")
                .title_text("ui_bundle_phase54_3_symbol_outline")
                .no_chrome()
                .size(1280.0, 900.0)
                .content(|| {
                    ui_bundle_code_editor_phase54_3_symbol_outline_showcase(
                        ShowcaseMode::DefaultTheme,
                    )
                }),
        )
        .window(
            Window::new("code-editor-phase54-3-ctags-fallback")
                .title_text("ui_bundle_phase54_3_ctags_fallback")
                .no_chrome()
                .size(1280.0, 900.0)
                .content(|| {
                    ui_bundle_code_editor_phase54_3_ctags_fallback_showcase(
                        ShowcaseMode::DefaultTheme,
                    )
                }),
        )
        .window(
            Window::new("code-editor-phase54-3-symbol-graph")
                .title_text("ui_bundle_phase54_3_symbol_graph")
                .no_chrome()
                .size(1280.0, 900.0)
                .content(|| {
                    ui_bundle_code_editor_phase54_3_symbol_graph_showcase(
                        ShowcaseMode::DefaultTheme,
                    )
                }),
        )
        .window(
            Window::new("code-editor-phase54-3-search")
                .title_text("ui_bundle_phase54_3_search_active_match")
                .no_chrome()
                .size(1280.0, 900.0)
                .content(|| {
                    ui_bundle_code_editor_phase54_3_search_showcase(ShowcaseMode::DefaultTheme)
                }),
        )
        .window(
            Window::new("code-editor-phase54-3-multiclick-selection")
                .title_text("ui_bundle_phase54_3_multiclick_selection")
                .no_chrome()
                .size(1280.0, 900.0)
                .content(|| {
                    ui_bundle_code_editor_phase54_3_multiclick_selection_showcase(
                        ShowcaseMode::DefaultTheme,
                    )
                }),
        )
        .window(
            Window::new("code-editor-phase54-3-diagnostics")
                .title_text("ui_bundle_phase54_3_diagnostics")
                .no_chrome()
                .size(1280.0, 900.0)
                .content(|| {
                    ui_bundle_code_editor_phase54_3_diagnostics_showcase(ShowcaseMode::DefaultTheme)
                }),
        )
        .window(
            Window::new("code-editor-phase54-3-folding")
                .title_text("ui_bundle_phase54_3_folding")
                .no_chrome()
                .size(1280.0, 900.0)
                .content(|| {
                    ui_bundle_code_editor_phase54_3_folding_showcase(ShowcaseMode::DefaultTheme)
                }),
        )
        .window(
            Window::new("code-editor-phase54-3-ide-folding-gutter")
                .title_text("ui_bundle_phase54_3_ide_folding_gutter")
                .no_chrome()
                .size(1280.0, 900.0)
                .content(|| {
                    ui_bundle_code_editor_phase54_3_ide_folding_gutter_showcase(
                        ShowcaseMode::DefaultTheme,
                    )
                }),
        )
        .window(
            Window::new("code-editor-phase54-3-lsp")
                .title_text("ui_bundle_phase54_3_lsp_enrichment")
                .no_chrome()
                .size(1280.0, 900.0)
                .content(|| {
                    ui_bundle_code_editor_phase54_3_lsp_showcase(ShowcaseMode::DefaultTheme)
                }),
        )
        .window(
            Window::new("code-editor-phase54-3-scip")
                .title_text("ui_bundle_phase54_3_scip_project_index")
                .no_chrome()
                .size(1280.0, 900.0)
                .content(|| {
                    ui_bundle_code_editor_phase54_3_scip_showcase(ShowcaseMode::DefaultTheme)
                }),
        )
        .window(
            Window::new("code-editor-phase54-3-large-file")
                .title_text("ui_bundle_phase54_3_large_file_scroll")
                .no_chrome()
                .size(1280.0, 900.0)
                .content(|| {
                    ui_bundle_code_editor_phase54_3_large_file_showcase(ShowcaseMode::DefaultTheme)
                }),
        )
        .window(
            Window::new("code-editor-phase54-3-theme-variants")
                .title_text("ui_bundle_phase54_3_code_theme_variants")
                .no_chrome()
                .size(1280.0, 900.0)
                .content(|| {
                    ui_bundle_code_editor_phase54_3_theme_variants_showcase(
                        ShowcaseMode::DefaultTheme,
                    )
                }),
        )
        .capture(cap.clone())
        .run()
        .expect("app run");

    let mvp_frame = cap
        .take_result(mvp_id)
        .expect("mvp capture slot")
        .expect("capture ok")
        .frame;
    let styled_frame = cap
        .take_result(styled_id)
        .expect("styled capture slot")
        .expect("capture ok")
        .frame;
    let baseline_frame = cap
        .take_result(baseline_id)
        .expect("baseline capture slot")
        .expect("capture ok")
        .frame;
    let active_line_frame = cap
        .take_result(active_line_id)
        .expect("active line capture slot")
        .expect("capture ok")
        .frame;
    let tree_sitter_frame = cap
        .take_result(tree_sitter_id)
        .expect("tree-sitter capture slot")
        .expect("capture ok")
        .frame;
    let extension_detection_frame = cap
        .take_result(extension_detection_id)
        .expect("extension detection capture slot")
        .expect("capture ok")
        .frame;
    let symbol_outline_frame = cap
        .take_result(symbol_outline_id)
        .expect("symbol outline capture slot")
        .expect("capture ok")
        .frame;
    let ctags_fallback_frame = cap
        .take_result(ctags_fallback_id)
        .expect("ctags fallback capture slot")
        .expect("capture ok")
        .frame;
    let symbol_graph_frame = cap
        .take_result(symbol_graph_id)
        .expect("symbol graph capture slot")
        .expect("capture ok")
        .frame;
    let search_frame = cap
        .take_result(search_id)
        .expect("search capture slot")
        .expect("capture ok")
        .frame;
    let multiclick_selection_frame = cap
        .take_result(multiclick_selection_id)
        .expect("multiclick selection capture slot")
        .expect("capture ok")
        .frame;
    let diagnostics_frame = cap
        .take_result(diagnostics_id)
        .expect("diagnostics capture slot")
        .expect("capture ok")
        .frame;
    let folding_frame = cap
        .take_result(folding_id)
        .expect("folding capture slot")
        .expect("capture ok")
        .frame;
    let ide_folding_gutter_frame = cap
        .take_result(ide_folding_gutter_id)
        .expect("ide folding gutter capture slot")
        .expect("capture ok")
        .frame;
    let lsp_frame = cap
        .take_result(lsp_id)
        .expect("lsp capture slot")
        .expect("capture ok")
        .frame;
    let scip_frame = cap
        .take_result(scip_id)
        .expect("scip capture slot")
        .expect("capture ok")
        .frame;
    let large_file_frame = cap
        .take_result(large_file_id)
        .expect("large file capture slot")
        .expect("capture ok")
        .frame;
    let theme_variants_frame = cap
        .take_result(theme_variants_id)
        .expect("theme variants capture slot")
        .expect("capture ok")
        .frame;

    let mvp_filename = "ui_bundle_phase54_3_mvp_debt_gutter_scroll.png";
    assert_code_editor_phase54_3_frame(&mvp_frame, mvp_filename);
    write_capture(mvp_filename, &mvp_frame);

    let styled_filename = "ui_bundle_phase54_3_styled_spans.png";
    assert_code_editor_phase54_3_frame(&styled_frame, styled_filename);
    write_capture(styled_filename, &styled_frame);

    let baseline_filename = "ui_bundle_phase54_3_styled_baseline_alignment.png";
    assert_code_editor_phase54_3_frame(&baseline_frame, baseline_filename);
    write_capture(baseline_filename, &baseline_frame);

    let active_line_filename = "ui_bundle_phase54_3_active_line_ring.png";
    assert_code_editor_phase54_3_frame(&active_line_frame, active_line_filename);
    write_capture(active_line_filename, &active_line_frame);

    let tree_sitter_filename = "ui_bundle_phase54_3_tree_sitter_rust_tokens.png";
    assert_code_editor_phase54_3_frame(&tree_sitter_frame, tree_sitter_filename);
    write_capture(tree_sitter_filename, &tree_sitter_frame);

    let extension_detection_filename = "ui_bundle_phase54_3_extension_language_detection.png";
    assert_code_editor_phase54_3_frame(&extension_detection_frame, extension_detection_filename);
    write_capture(extension_detection_filename, &extension_detection_frame);

    let symbol_outline_filename = "ui_bundle_phase54_3_symbol_outline.png";
    assert_code_editor_phase54_3_frame(&symbol_outline_frame, symbol_outline_filename);
    write_capture(symbol_outline_filename, &symbol_outline_frame);

    let ctags_fallback_filename = "ui_bundle_phase54_3_ctags_fallback.png";
    assert_code_editor_phase54_3_frame(&ctags_fallback_frame, ctags_fallback_filename);
    write_capture(ctags_fallback_filename, &ctags_fallback_frame);

    let symbol_graph_filename = "ui_bundle_phase54_3_symbol_graph.png";
    assert_code_editor_phase54_3_frame(&symbol_graph_frame, symbol_graph_filename);
    write_capture(symbol_graph_filename, &symbol_graph_frame);

    let search_filename = "ui_bundle_phase54_3_search_active_match.png";
    assert_code_editor_phase54_3_frame(&search_frame, search_filename);
    write_capture(search_filename, &search_frame);

    let multiclick_selection_filename = "ui_bundle_phase54_3_multiclick_selection.png";
    assert_code_editor_phase54_3_frame(&multiclick_selection_frame, multiclick_selection_filename);
    write_capture(multiclick_selection_filename, &multiclick_selection_frame);

    let diagnostics_filename = "ui_bundle_phase54_3_diagnostics.png";
    assert_code_editor_phase54_3_frame(&diagnostics_frame, diagnostics_filename);
    write_capture(diagnostics_filename, &diagnostics_frame);

    let folding_filename = "ui_bundle_phase54_3_folding.png";
    assert_code_editor_phase54_3_frame(&folding_frame, folding_filename);
    write_capture(folding_filename, &folding_frame);

    let ide_folding_gutter_filename = "ui_bundle_phase54_3_ide_folding_gutter.png";
    assert_code_editor_phase54_3_frame(&ide_folding_gutter_frame, ide_folding_gutter_filename);
    let fold_marker_pixels = count_pixels(&ide_folding_gutter_frame.rgba, |px| {
        px[0] > 180 && px[1] > 80 && px[1] < 190 && px[2] < 90
    });
    assert!(
        fold_marker_pixels > 20,
        "{ide_folding_gutter_filename}: fold marker/guide pixels={fold_marker_pixels}"
    );
    write_capture(ide_folding_gutter_filename, &ide_folding_gutter_frame);

    let lsp_filename = "ui_bundle_phase54_3_lsp_enrichment.png";
    assert_code_editor_phase54_3_frame(&lsp_frame, lsp_filename);
    write_capture(lsp_filename, &lsp_frame);

    let scip_filename = "ui_bundle_phase54_3_scip_project_index.png";
    assert_code_editor_phase54_3_frame(&scip_frame, scip_filename);
    write_capture(scip_filename, &scip_frame);

    let large_file_filename = "ui_bundle_phase54_3_large_file_scroll.png";
    assert_code_editor_phase54_3_frame(&large_file_frame, large_file_filename);
    write_capture(large_file_filename, &large_file_frame);

    let theme_variants_filename = "ui_bundle_phase54_3_code_theme_variants.png";
    assert_code_editor_phase54_3_frame(&theme_variants_frame, theme_variants_filename);
    write_capture(theme_variants_filename, &theme_variants_frame);
}
