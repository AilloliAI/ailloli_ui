//! Shared retained-view builders and deterministic fixtures for the widget-bundle examples.
//!
//! The public functions are example-target entry points, not library API. Each
//! accepts an explicit palette mode so visual captures can compare stable white
//! and default-theme variants without process-global theme mutation.

use ailloli_ui::core::TextStyle;
use ailloli_ui::prelude::*;

#[cfg(test)]
use ailloli_ui_editor::code::{
    CtagsSymbolIndexer, SemanticDocumentSymbol, SemanticReference, SymbolId, SymbolIndexer,
    TreeSitterRustSymbolIndexer,
};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
/// Palette variant applied to every deterministic showcase entry point.
///
/// # Examples
///
/// In this example target, call `ui_bundle_showcase(ShowcaseMode::White)` for
/// the fixed light palette or pass `ShowcaseMode::DefaultTheme` for theme tokens.
pub enum ShowcaseMode {
    /// Fixed high-contrast light palette used by screenshot baselines.
    White,
    /// Colors derived from [`Theme::default`].
    DefaultTheme,
}

#[derive(Debug, Clone, Copy)]
/// Complete color set shared by sections so one mode is applied consistently.
/// Retained helper state for the `ShowcasePalette` showcase fixture.
struct ShowcasePalette {
    /// Page background.
    background: Color,
    /// Primary card/control surface.
    surface: Color,
    /// Raised surface.
    elevated: Color,
    /// Separator and outline color.
    border: Color,
    /// Primary text color.
    text: Color,
    /// Secondary text color.
    muted: Color,
    /// Interactive accent.
    accent: Color,
    /// Destructive/error tone.
    danger: Color,
    /// Success tone.
    success: Color,
    /// Warning tone.
    warning: Color,
    /// Informational tone.
    info: Color,
    /// Shadow color including alpha.
    shadow: Color,
}

/// Stable root identity, title, and complete palette for a showcase mode.
impl ShowcaseMode {
    /// Returns the stable root view key used by capture tests.
    fn root_key(self) -> &'static str {
        match self {
            Self::White => "phase39-white-root",
            Self::DefaultTheme => "phase39-default-root",
        }
    }

    /// Returns the human-readable gallery title.
    fn title(self) -> &'static str {
        match self {
            Self::White => "Ailloli UI Widget Bundle - White Showcase",
            Self::DefaultTheme => "Ailloli UI Widget Bundle - Default Theme",
        }
    }

    /// Resolves every semantic color without later global lookup.
    fn palette(self) -> ShowcasePalette {
        let theme = Theme::default();
        let p = theme.palette();
        match self {
            Self::DefaultTheme => ShowcasePalette {
                background: p.background,
                surface: p.surface,
                elevated: p.surface_elevated,
                border: p.border,
                text: p.text,
                muted: p.text_muted,
                accent: p.accent,
                danger: p.danger,
                success: p.success,
                warning: p.warning,
                info: p.info,
                shadow: Color::rgba(0, 0, 0, 0.28),
            },
            Self::White => ShowcasePalette {
                background: Color::hex_rgb(0xF7F8FA),
                surface: Color::WHITE,
                elevated: Color::hex_rgb(0xFFFFFF),
                border: Color::hex_rgb(0xD7DCE2),
                text: Color::hex_rgb(0x111827),
                muted: Color::hex_rgb(0x647181),
                accent: p.accent,
                danger: Color::hex_rgb(0xDC2626),
                success: Color::hex_rgb(0x16A34A),
                warning: Color::hex_rgb(0xD97706),
                info: Color::hex_rgb(0x0284C7),
                shadow: Color::rgba(15, 23, 42, 0.12),
            },
        }
    }
}

/// Builds the `ui bundle showcase` deterministic retained-view scenario.
///
/// `mode` selects either the fixed white palette or default theme tokens; it
/// does not mutate global theme state.
///
/// # Examples
///
/// In this example target, call `ui_bundle_showcase(ShowcaseMode::White)` and pass the
/// returned view to the sandbox or capture harness.
pub fn ui_bundle_showcase(mode: ShowcaseMode) -> impl IntoView<()> {
    build_showcase(mode)
}

#[cfg(test)]
#[allow(dead_code)]
/// Builds the `ui bundle switches showcase` deterministic retained-view scenario.
///
/// `mode` selects either the fixed white palette or default theme tokens; it
/// does not mutate global theme state.
///
/// # Examples
///
/// In this example target, call `ui_bundle_switches_showcase(ShowcaseMode::White)` and pass the
/// returned view to the sandbox or capture harness.
pub fn ui_bundle_switches_showcase(mode: ShowcaseMode) -> impl IntoView<()> {
    let colors = mode.palette();
    Container::new()
        .fill()
        .background(colors.background)
        .padding(18.0)
        .child(section(
            mode,
            "section-switches",
            "Switches",
            "Phase 41 native boolean control states.",
            switches_section(mode),
        ))
}

#[cfg(test)]
#[allow(dead_code)]
/// Builds the `ui bundle radio group showcase` deterministic retained-view scenario.
///
/// `mode` selects either the fixed white palette or default theme tokens; it
/// does not mutate global theme state.
///
/// # Examples
///
/// In this example target, call `ui_bundle_radio_group_showcase(ShowcaseMode::White)` and pass the
/// returned view to the sandbox or capture harness.
pub fn ui_bundle_radio_group_showcase(mode: ShowcaseMode) -> impl IntoView<()> {
    let colors = mode.palette();
    Container::new()
        .fill()
        .background(colors.background)
        .padding(18.0)
        .child(section(
            mode,
            "section-radio-groups",
            "Radio Groups",
            "Phase 42 native exclusive selection controls.",
            radio_groups_section(mode),
        ))
}

#[cfg(test)]
#[allow(dead_code)]
/// Builds the `ui bundle segmented control showcase` deterministic retained-view scenario.
///
/// `mode` selects either the fixed white palette or default theme tokens; it
/// does not mutate global theme state.
///
/// # Examples
///
/// In this example target, call `ui_bundle_segmented_control_showcase(ShowcaseMode::White)` and pass the
/// returned view to the sandbox or capture harness.
pub fn ui_bundle_segmented_control_showcase(mode: ShowcaseMode) -> impl IntoView<()> {
    let colors = mode.palette();
    Container::new()
        .fill()
        .background(colors.background)
        .padding(18.0)
        .child(section(
            mode,
            "section-segmented-controls",
            "Segmented Controls",
            "Phase 43 native exclusive segmented controls.",
            segmented_controls_section(mode),
        ))
}

#[cfg(test)]
#[allow(dead_code)]
/// Builds the `ui bundle slider showcase` deterministic retained-view scenario.
///
/// `mode` selects either the fixed white palette or default theme tokens; it
/// does not mutate global theme state.
///
/// # Examples
///
/// In this example target, call `ui_bundle_slider_showcase(ShowcaseMode::White)` and pass the
/// returned view to the sandbox or capture harness.
pub fn ui_bundle_slider_showcase(mode: ShowcaseMode) -> impl IntoView<()> {
    let colors = mode.palette();
    Container::new()
        .fill()
        .background(colors.background)
        .padding(18.0)
        .child(section(
            mode,
            "section-sliders",
            "Sliders",
            "Phase 44 native value and range controls.",
            sliders_section(mode),
        ))
}

#[cfg(test)]
#[allow(dead_code)]
/// Builds the `ui bundle progress showcase` deterministic retained-view scenario.
///
/// `mode` selects either the fixed white palette or default theme tokens; it
/// does not mutate global theme state.
///
/// # Examples
///
/// In this example target, call `ui_bundle_progress_showcase(ShowcaseMode::White)` and pass the
/// returned view to the sandbox or capture harness.
pub fn ui_bundle_progress_showcase(mode: ShowcaseMode) -> impl IntoView<()> {
    let colors = mode.palette();
    Container::new()
        .fill()
        .background(colors.background)
        .padding(18.0)
        .child(section(
            mode,
            "section-progress",
            "Progress",
            "Phase 45 native determinate progress indicators.",
            progress_section(mode),
        ))
}

#[cfg(test)]
#[allow(dead_code)]
/// Builds the `ui bundle select dropdown showcase` deterministic retained-view scenario.
///
/// `mode` selects either the fixed white palette or default theme tokens; it
/// does not mutate global theme state.
///
/// # Examples
///
/// In this example target, call `ui_bundle_select_dropdown_showcase(ShowcaseMode::White)` and pass the
/// returned view to the sandbox or capture harness.
pub fn ui_bundle_select_dropdown_showcase(mode: ShowcaseMode) -> impl IntoView<()> {
    let colors = mode.palette();
    Container::new()
        .fill()
        .background(colors.background)
        .padding(18.0)
        .child(section(
            mode,
            "section-select-dropdowns",
            "Select & Dropdown",
            "Phase 46 native anchored popup controls.",
            select_dropdowns_section(mode),
        ))
}

#[cfg(test)]
#[allow(dead_code)]
/// Builds the `ui bundle combobox autocomplete showcase` deterministic retained-view scenario.
///
/// `mode` selects either the fixed white palette or default theme tokens; it
/// does not mutate global theme state.
///
/// # Examples
///
/// In this example target, call `ui_bundle_combobox_autocomplete_showcase(ShowcaseMode::White)` and pass the
/// returned view to the sandbox or capture harness.
pub fn ui_bundle_combobox_autocomplete_showcase(mode: ShowcaseMode) -> impl IntoView<()> {
    let colors = mode.palette();
    Container::new()
        .fill()
        .background(colors.background)
        .padding(18.0)
        .child(section(
            mode,
            "section-combobox-autocomplete",
            "ComboBox & Autocomplete",
            "Phase 47 native filtered text popup controls.",
            combobox_autocomplete_section(mode),
        ))
}

#[cfg(test)]
#[allow(dead_code)]
/// Builds the `ui bundle cards avatar status divider showcase` deterministic retained-view scenario.
///
/// `mode` selects either the fixed white palette or default theme tokens; it
/// does not mutate global theme state.
///
/// # Examples
///
/// In this example target, call `ui_bundle_cards_avatar_status_divider_showcase(ShowcaseMode::White)` and pass the
/// returned view to the sandbox or capture harness.
pub fn ui_bundle_cards_avatar_status_divider_showcase(mode: ShowcaseMode) -> impl IntoView<()> {
    let colors = mode.palette();
    Container::new()
        .fill()
        .background(colors.background)
        .padding(18.0)
        .child(section(
            mode,
            "section-cards-avatar-status-divider",
            "Cards, Avatars & Indicators",
            "Phase 48 native composition primitives.",
            cards_avatar_status_divider_section(mode),
        ))
}

#[cfg(test)]
#[allow(dead_code)]
/// Builds the `ui bundle navigation lists showcase` deterministic retained-view scenario.
///
/// `mode` selects either the fixed white palette or default theme tokens; it
/// does not mutate global theme state.
///
/// # Examples
///
/// In this example target, call `ui_bundle_navigation_lists_showcase(ShowcaseMode::White)` and pass the
/// returned view to the sandbox or capture harness.
pub fn ui_bundle_navigation_lists_showcase(mode: ShowcaseMode) -> impl IntoView<()> {
    let colors = mode.palette();
    Container::new()
        .fill()
        .background(colors.background)
        .padding(18.0)
        .child(section(
            mode,
            "section-navigation-lists",
            "Navigation & Lists",
            "Phase 49 native navigation rows and simple list compositions.",
            navigation_lists_section(mode),
        ))
}

#[cfg(test)]
#[allow(dead_code)]
/// Builds the `ui bundle accordion tree showcase` deterministic retained-view scenario.
///
/// `mode` selects either the fixed white palette or default theme tokens; it
/// does not mutate global theme state.
///
/// # Examples
///
/// In this example target, call `ui_bundle_accordion_tree_showcase(ShowcaseMode::White)` and pass the
/// returned view to the sandbox or capture harness.
pub fn ui_bundle_accordion_tree_showcase(mode: ShowcaseMode) -> impl IntoView<()> {
    let colors = mode.palette();
    Container::new()
        .fill()
        .background(colors.background)
        .padding(18.0)
        .child(section(
            mode,
            "section-accordion-tree",
            "Accordion & TreeView",
            "Phase 50 native disclosure and hierarchical controls.",
            accordion_tree_section(mode),
        ))
}

#[cfg(test)]
#[allow(dead_code)]
/// Builds the `ui bundle tree edit drag showcase` deterministic retained-view scenario.
///
/// `mode` selects either the fixed white palette or default theme tokens; it
/// does not mutate global theme state.
///
/// # Examples
///
/// In this example target, call `ui_bundle_tree_edit_drag_showcase(ShowcaseMode::White)` and pass the
/// returned view to the sandbox or capture harness.
pub fn ui_bundle_tree_edit_drag_showcase(mode: ShowcaseMode) -> impl IntoView<()> {
    let colors = mode.palette();
    Container::new()
        .fill()
        .background(colors.background)
        .padding(18.0)
        .child(section(
            mode,
            "section-tree-edit-drag",
            "TreeView Edit & Drag",
            "Phase 50.1 mutable tree operations and inline editing states.",
            tree_edit_drag_section(mode),
        ))
}

#[cfg(test)]
#[allow(dead_code)]
/// Builds the `ui bundle table view showcase` deterministic retained-view scenario.
///
/// `mode` selects either the fixed white palette or default theme tokens; it
/// does not mutate global theme state.
///
/// # Examples
///
/// In this example target, call `ui_bundle_table_view_showcase(ShowcaseMode::White)` and pass the
/// returned view to the sandbox or capture harness.
pub fn ui_bundle_table_view_showcase(mode: ShowcaseMode) -> impl IntoView<()> {
    let colors = mode.palette();
    Container::new()
        .fill()
        .background(colors.background)
        .padding(18.0)
        .child(section(
            mode,
            "section-table-view",
            "TableView",
            "Phase 51 native static data grid with sticky header and internal scroll.",
            table_view_section(mode),
        ))
}

#[cfg(test)]
#[allow(dead_code)]
/// Builds the `ui bundle feedback overlays showcase` deterministic retained-view scenario.
///
/// `mode` selects either the fixed white palette or default theme tokens; it
/// does not mutate global theme state.
///
/// # Examples
///
/// In this example target, call `ui_bundle_feedback_overlays_showcase(ShowcaseMode::White)` and pass the
/// returned view to the sandbox or capture harness.
pub fn ui_bundle_feedback_overlays_showcase(mode: ShowcaseMode) -> impl IntoView<()> {
    let colors = mode.palette();
    Container::new()
        .fill()
        .background(colors.background)
        .padding(18.0)
        .child(section(
            mode,
            "section-feedback-overlays",
            "Feedback & Overlays",
            "Phase 52 native toast host and confirmation dialog overlays.",
            feedback_overlays_section(mode),
        ))
}

#[cfg(test)]
#[allow(dead_code)]
/// Builds the `ui bundle command palette showcase` deterministic retained-view scenario.
///
/// `mode` selects either the fixed white palette or default theme tokens; it
/// does not mutate global theme state.
///
/// # Examples
///
/// In this example target, call `ui_bundle_command_palette_showcase(ShowcaseMode::White)` and pass the
/// returned view to the sandbox or capture harness.
pub fn ui_bundle_command_palette_showcase(mode: ShowcaseMode) -> impl IntoView<()> {
    let colors = mode.palette();
    Container::new()
        .fill()
        .background(colors.background)
        .padding(18.0)
        .child(section(
            mode,
            "section-command-palette",
            "Command Palette",
            "Phase 52 native filtered action palette overlay.",
            command_palette_section(mode),
        ))
}

#[cfg(test)]
#[allow(dead_code)]
/// Builds the `ui bundle pickers upload showcase` deterministic retained-view scenario.
///
/// `mode` selects either the fixed white palette or default theme tokens; it
/// does not mutate global theme state.
///
/// # Examples
///
/// In this example target, call `ui_bundle_pickers_upload_showcase(ShowcaseMode::White)` and pass the
/// returned view to the sandbox or capture harness.
pub fn ui_bundle_pickers_upload_showcase(mode: ShowcaseMode) -> impl IntoView<()> {
    let colors = mode.palette();
    Container::new()
        .fill()
        .background(colors.background)
        .padding(18.0)
        .child(section(
            mode,
            "section-pickers-upload",
            "Pickers & Upload",
            "Phase 53 native date, time, color and abstract upload controls.",
            pickers_upload_section(mode),
        ))
}

#[cfg(test)]
#[allow(dead_code)]
/// Builds the `ui bundle charts showcase` deterministic retained-view scenario.
///
/// `mode` selects either the fixed white palette or default theme tokens; it
/// does not mutate global theme state.
///
/// # Examples
///
/// In this example target, call `ui_bundle_charts_showcase(ShowcaseMode::White)` and pass the
/// returned view to the sandbox or capture harness.
pub fn ui_bundle_charts_showcase(mode: ShowcaseMode) -> impl IntoView<()> {
    let colors = mode.palette();
    Container::new()
        .fill()
        .background(colors.background)
        .padding(18.0)
        .child(section(
            mode,
            "section-charts",
            "Charts",
            "Phase 54 native simple charts rendered with existing primitives.",
            charts_section(mode),
        ))
}

#[cfg(test)]
#[allow(dead_code)]
/// Builds the `ui bundle terminal phase54 1 showcase` deterministic retained-view scenario.
///
/// `mode` selects either the fixed white palette or default theme tokens; it
/// does not mutate global theme state.
///
/// # Examples
///
/// In this example target, call `ui_bundle_terminal_phase54_1_showcase(ShowcaseMode::White)` and pass the
/// returned view to the sandbox or capture harness.
pub fn ui_bundle_terminal_phase54_1_showcase(mode: ShowcaseMode) -> impl IntoView<()> {
    terminal_phase54_1_showcase(mode, TerminalShowcaseScenario::Default)
}

#[cfg(test)]
#[allow(dead_code)]
/// Builds the `ui bundle terminal phase77 showcase` deterministic retained-view scenario.
///
/// `mode` selects either the fixed white palette or default theme tokens; it
/// does not mutate global theme state.
///
/// # Examples
///
/// In this example target, call `ui_bundle_terminal_phase77_showcase(ShowcaseMode::White)` and pass the
/// returned view to the sandbox or capture harness.
pub fn ui_bundle_terminal_phase77_showcase(mode: ShowcaseMode) -> impl IntoView<()> {
    let colors = mode.palette();
    Container::new()
        .fill()
        .background(colors.background)
        .padding(18.0)
        .child(section(
            mode,
            "section-terminal-widget-v2",
            "Terminal",
            "Phase 77 state-backed terminal widget with ANSI color, cursor, selection and input mapping.",
            terminal_phase77_section(mode),
        ))
}

#[cfg(test)]
#[allow(dead_code)]
/// Builds the `ui bundle terminal phase78 showcase` deterministic retained-view scenario.
///
/// `mode` selects either the fixed white palette or default theme tokens; it
/// does not mutate global theme state.
///
/// # Examples
///
/// In this example target, call `ui_bundle_terminal_phase78_showcase(ShowcaseMode::White)` and pass the
/// returned view to the sandbox or capture harness.
pub fn ui_bundle_terminal_phase78_showcase(mode: ShowcaseMode) -> impl IntoView<()> {
    let colors = mode.palette();
    Container::new()
        .fill()
        .background(colors.background)
        .padding(18.0)
        .child(
            Column::new()
                .gap(14.0)
                .child(section(
                    mode,
                    "section-terminal-scrollback-selection",
                    "Terminal scrollback",
                    "Phase 78 follow-output, scrollback viewport, selection and clipboard-ready extraction.",
                    terminal_phase78_scrollback_section(mode),
                ))
                .child(section(
                    mode,
                    "section-terminal-tui",
                    "Terminal TUI",
                    "Phase 78 alternate screen, bracketed paste, application cursor and mouse tracking modes.",
                    terminal_phase78_tui_section(mode),
                )),
        )
}

#[cfg(test)]
#[allow(dead_code)]
/// Builds the `ui bundle terminal phase80 showcase` deterministic retained-view scenario.
///
/// `mode` selects either the fixed white palette or default theme tokens; it
/// does not mutate global theme state.
///
/// # Examples
///
/// In this example target, call `ui_bundle_terminal_phase80_showcase(ShowcaseMode::White)` and pass the
/// returned view to the sandbox or capture harness.
pub fn ui_bundle_terminal_phase80_showcase(mode: ShowcaseMode) -> impl IntoView<()> {
    let colors = mode.palette();
    Container::new()
        .fill()
        .background(colors.background)
        .padding(18.0)
        .child(section(
            mode,
            "section-terminal-diagnostics",
            "Terminal diagnostics",
            "Phase 80 output classification with IDE-ready diagnostics and visual markers.",
            terminal_phase80_section(mode),
        ))
}

#[cfg(test)]
#[allow(dead_code)]
/// Builds the `ui bundle terminal phase54 1 search showcase` deterministic retained-view scenario.
///
/// `mode` selects either the fixed white palette or default theme tokens; it
/// does not mutate global theme state.
///
/// # Examples
///
/// In this example target, call `ui_bundle_terminal_phase54_1_search_showcase(ShowcaseMode::White)` and pass the
/// returned view to the sandbox or capture harness.
pub fn ui_bundle_terminal_phase54_1_search_showcase(mode: ShowcaseMode) -> impl IntoView<()> {
    terminal_phase54_1_showcase(mode, TerminalShowcaseScenario::Search)
}

#[cfg(test)]
#[allow(dead_code)]
/// Builds the `ui bundle terminal phase54 1 selection showcase` deterministic retained-view scenario.
///
/// `mode` selects either the fixed white palette or default theme tokens; it
/// does not mutate global theme state.
///
/// # Examples
///
/// In this example target, call `ui_bundle_terminal_phase54_1_selection_showcase(ShowcaseMode::White)` and pass the
/// returned view to the sandbox or capture harness.
pub fn ui_bundle_terminal_phase54_1_selection_showcase(mode: ShowcaseMode) -> impl IntoView<()> {
    terminal_phase54_1_showcase(mode, TerminalShowcaseScenario::Selection)
}

#[cfg(test)]
#[allow(dead_code)]
/// Builds the `ui bundle terminal phase54 1 capture suite showcase` deterministic retained-view scenario.
///
/// `mode` selects either the fixed white palette or default theme tokens; it
/// does not mutate global theme state.
///
/// # Examples
///
/// In this example target, call `ui_bundle_terminal_phase54_1_capture_suite_showcase(ShowcaseMode::White)` and pass the
/// returned view to the sandbox or capture harness.
pub fn ui_bundle_terminal_phase54_1_capture_suite_showcase(
    mode: ShowcaseMode,
) -> impl IntoView<()> {
    let colors = mode.palette();
    Container::new()
        .fill()
        .background(colors.background)
        .padding(18.0)
        .child(
            Row::new()
                .gap(12.0)
                .child(
                    Container::new()
                        .width(400.0)
                        .child(terminal_phase54_1_section(
                            mode,
                            "section-terminal-view",
                            TerminalShowcaseScenario::Default,
                        )),
                )
                .child(
                    Container::new()
                        .width(400.0)
                        .child(terminal_phase54_1_section(
                            mode,
                            "section-terminal-view-search",
                            TerminalShowcaseScenario::Search,
                        )),
                )
                .child(
                    Container::new()
                        .width(400.0)
                        .child(terminal_phase54_1_section(
                            mode,
                            "section-terminal-view-selection",
                            TerminalShowcaseScenario::Selection,
                        )),
                ),
        )
}

#[allow(dead_code)]
#[derive(Debug, Clone, Copy)]
/// Closed scenario/value set used by the `TerminalShowcaseScenario` showcase fixture.
enum TerminalShowcaseScenario {
    /// Ordinary terminal widget state without forced interaction.
    Default,
    /// Search overlay and match-navigation state.
    Search,
    /// Text-selection state.
    Selection,
}

#[cfg(test)]
/// Builds or computes the `terminal phase54 1 showcase` deterministic showcase fixture.
fn terminal_phase54_1_showcase(
    mode: ShowcaseMode,
    scenario: TerminalShowcaseScenario,
) -> impl IntoView<()> {
    let colors = mode.palette();
    Container::new()
        .fill()
        .background(colors.background)
        .padding(18.0)
        .child(terminal_phase54_1_section(
            mode,
            "section-terminal-view",
            scenario,
        ))
}

#[cfg(test)]
/// Builds or computes the `terminal phase54 1 section` deterministic showcase fixture.
fn terminal_phase54_1_section(
    mode: ShowcaseMode,
    key: &'static str,
    scenario: TerminalShowcaseScenario,
) -> View<()> {
    section(
        mode,
        key,
        "TerminalView",
        "Phase 54.1 read-only terminal widget with bounded history, search and selection.",
        terminal_view_section(mode, scenario),
    )
}

#[cfg(test)]
#[allow(dead_code)]
/// Builds the `ui bundle code editor showcase` deterministic retained-view scenario.
///
/// `mode` selects either the fixed white palette or default theme tokens; it
/// does not mutate global theme state.
///
/// # Examples
///
/// In this example target, call `ui_bundle_code_editor_showcase(ShowcaseMode::White)` and pass the
/// returned view to the sandbox or capture harness.
pub fn ui_bundle_code_editor_showcase(mode: ShowcaseMode) -> impl IntoView<()> {
    let colors = mode.palette();
    Container::new()
        .fill()
        .background(colors.background)
        .padding(18.0)
        .child(section(
            mode,
            "section-code-editor",
            "CodeEditor",
            "Phase 54.2 code editor MVP with gutter and horizontal scroll.",
            code_editor_section(mode),
        ))
}

#[cfg(test)]
#[allow(dead_code)]
/// Builds the `ui bundle code editor phase54 3 showcase` deterministic retained-view scenario.
///
/// `mode` selects either the fixed white palette or default theme tokens; it
/// does not mutate global theme state.
///
/// # Examples
///
/// In this example target, call `ui_bundle_code_editor_phase54_3_showcase(ShowcaseMode::White)` and pass the
/// returned view to the sandbox or capture harness.
pub fn ui_bundle_code_editor_phase54_3_showcase(mode: ShowcaseMode) -> impl IntoView<()> {
    let colors = mode.palette();
    Container::new()
        .fill()
        .background(colors.background)
        .padding(18.0)
        .child(section(
            mode,
            "section-code-editor-phase54-3",
            "CodeEditor Advanced",
            "Phase 54.3 startup scenario with fixed gutter and deterministic initial scroll.",
            code_editor_phase54_3_section(mode),
        ))
}

#[cfg(test)]
#[allow(dead_code)]
/// Builds the `ui bundle code editor phase54 3 baseline showcase` deterministic retained-view scenario.
///
/// `mode` selects either the fixed white palette or default theme tokens; it
/// does not mutate global theme state.
///
/// # Examples
///
/// In this example target, call `ui_bundle_code_editor_phase54_3_baseline_showcase(ShowcaseMode::White)` and pass the
/// returned view to the sandbox or capture harness.
pub fn ui_bundle_code_editor_phase54_3_baseline_showcase(mode: ShowcaseMode) -> impl IntoView<()> {
    let colors = mode.palette();
    Container::new()
        .fill()
        .background(colors.background)
        .padding(18.0)
        .child(section(
            mode,
            "section-code-editor-phase54-3-baseline",
            "CodeEditor Baseline",
            "Phase 54.3 styled Rust baseline alignment regression.",
            code_editor_phase54_3_baseline_section(mode),
        ))
}

#[cfg(test)]
#[allow(dead_code)]
/// Builds the `ui bundle code editor phase54 3 active line showcase` deterministic retained-view scenario.
///
/// `mode` selects either the fixed white palette or default theme tokens; it
/// does not mutate global theme state.
///
/// # Examples
///
/// In this example target, call `ui_bundle_code_editor_phase54_3_active_line_showcase(ShowcaseMode::White)` and pass the
/// returned view to the sandbox or capture harness.
pub fn ui_bundle_code_editor_phase54_3_active_line_showcase(
    mode: ShowcaseMode,
) -> impl IntoView<()> {
    let colors = mode.palette();
    Container::new()
        .fill()
        .background(colors.background)
        .padding(18.0)
        .child(section(
            mode,
            "section-code-editor-phase54-3-active-line",
            "CodeEditor Active Line",
            "Phase 54.3 active line ring scenario.",
            code_editor_phase54_3_active_line_section(mode),
        ))
}

#[cfg(test)]
#[allow(dead_code)]
/// Builds the `ui bundle code editor phase54 3 tree sitter showcase` deterministic retained-view scenario.
///
/// `mode` selects either the fixed white palette or default theme tokens; it
/// does not mutate global theme state.
///
/// # Examples
///
/// In this example target, call `ui_bundle_code_editor_phase54_3_tree_sitter_showcase(ShowcaseMode::White)` and pass the
/// returned view to the sandbox or capture harness.
pub fn ui_bundle_code_editor_phase54_3_tree_sitter_showcase(
    mode: ShowcaseMode,
) -> impl IntoView<()> {
    let colors = mode.palette();
    Container::new()
        .fill()
        .background(colors.background)
        .padding(18.0)
        .child(section(
            mode,
            "section-code-editor-phase54-3-tree-sitter",
            "CodeEditor Tree-sitter",
            "Phase 54.3 hybrid Tree-sitter Rust syntax tokens with lexical gap-fill.",
            code_editor_phase54_3_tree_sitter_section(mode),
        ))
}

#[cfg(test)]
#[allow(dead_code)]
/// Builds the `ui bundle code editor phase54 3 extension detection showcase` deterministic retained-view scenario.
///
/// `mode` selects either the fixed white palette or default theme tokens; it
/// does not mutate global theme state.
///
/// # Examples
///
/// In this example target, call `ui_bundle_code_editor_phase54_3_extension_detection_showcase(ShowcaseMode::White)` and pass the
/// returned view to the sandbox or capture harness.
pub fn ui_bundle_code_editor_phase54_3_extension_detection_showcase(
    mode: ShowcaseMode,
) -> impl IntoView<()> {
    let colors = mode.palette();
    Container::new()
        .fill()
        .background(colors.background)
        .padding(18.0)
        .child(section(
            mode,
            "section-code-editor-phase54-3-extension-detection",
            "CodeEditor Extension Detection",
            "Phase 54.3 auto-detect Rust syntax from the document .rs path.",
            code_editor_phase54_3_extension_detection_section(mode),
        ))
}

#[cfg(test)]
#[allow(dead_code)]
/// Builds the `ui bundle code editor phase54 3 symbol outline showcase` deterministic retained-view scenario.
///
/// `mode` selects either the fixed white palette or default theme tokens; it
/// does not mutate global theme state.
///
/// # Examples
///
/// In this example target, call `ui_bundle_code_editor_phase54_3_symbol_outline_showcase(ShowcaseMode::White)` and pass the
/// returned view to the sandbox or capture harness.
pub fn ui_bundle_code_editor_phase54_3_symbol_outline_showcase(
    mode: ShowcaseMode,
) -> impl IntoView<()> {
    let colors = mode.palette();
    Container::new()
        .fill()
        .background(colors.background)
        .padding(18.0)
        .child(section(
            mode,
            "section-code-editor-phase54-3-symbol-outline",
            "CodeEditor Symbol Outline",
            "Phase 54.3 Tree-sitter Rust symbol summary and outline IR.",
            code_editor_phase54_3_symbol_outline_section(mode),
        ))
}

#[cfg(test)]
#[allow(dead_code)]
/// Builds the `ui bundle code editor phase54 3 ctags fallback showcase` deterministic retained-view scenario.
///
/// `mode` selects either the fixed white palette or default theme tokens; it
/// does not mutate global theme state.
///
/// # Examples
///
/// In this example target, call `ui_bundle_code_editor_phase54_3_ctags_fallback_showcase(ShowcaseMode::White)` and pass the
/// returned view to the sandbox or capture harness.
pub fn ui_bundle_code_editor_phase54_3_ctags_fallback_showcase(
    mode: ShowcaseMode,
) -> impl IntoView<()> {
    let colors = mode.palette();
    Container::new()
        .fill()
        .background(colors.background)
        .padding(18.0)
        .child(section(
            mode,
            "section-code-editor-phase54-3-ctags-fallback",
            "CodeEditor Ctags Fallback",
            "Phase 54.3 Universal Ctags fallback summary with Ctags source provenance.",
            code_editor_phase54_3_ctags_fallback_section(mode),
        ))
}

#[cfg(test)]
#[allow(dead_code)]
/// Builds the `ui bundle code editor phase54 3 symbol graph showcase` deterministic retained-view scenario.
///
/// `mode` selects either the fixed white palette or default theme tokens; it
/// does not mutate global theme state.
///
/// # Examples
///
/// In this example target, call `ui_bundle_code_editor_phase54_3_symbol_graph_showcase(ShowcaseMode::White)` and pass the
/// returned view to the sandbox or capture harness.
pub fn ui_bundle_code_editor_phase54_3_symbol_graph_showcase(
    mode: ShowcaseMode,
) -> impl IntoView<()> {
    let colors = mode.palette();
    Container::new()
        .fill()
        .background(colors.background)
        .padding(18.0)
        .child(section(
            mode,
            "section-code-editor-phase54-3-symbol-graph",
            "CodeEditor Symbol Graph",
            "Phase 54.3 Tree-sitter Rust symbol graph with Contains, Imports and Calls edges.",
            code_editor_phase54_3_symbol_graph_section(mode),
        ))
}

#[cfg(test)]
#[allow(dead_code)]
/// Builds the `ui bundle code editor phase54 3 search showcase` deterministic retained-view scenario.
///
/// `mode` selects either the fixed white palette or default theme tokens; it
/// does not mutate global theme state.
///
/// # Examples
///
/// In this example target, call `ui_bundle_code_editor_phase54_3_search_showcase(ShowcaseMode::White)` and pass the
/// returned view to the sandbox or capture harness.
pub fn ui_bundle_code_editor_phase54_3_search_showcase(mode: ShowcaseMode) -> impl IntoView<()> {
    let colors = mode.palette();
    Container::new()
        .fill()
        .background(colors.background)
        .padding(18.0)
        .child(section(
            mode,
            "section-code-editor-phase54-3-search",
            "CodeEditor Search",
            "Phase 54.3 search highlights with an active match.",
            code_editor_phase54_3_search_section(mode),
        ))
}

#[cfg(test)]
#[allow(dead_code)]
/// Builds the `ui bundle code editor phase54 3 multiclick selection showcase` deterministic retained-view scenario.
///
/// `mode` selects either the fixed white palette or default theme tokens; it
/// does not mutate global theme state.
///
/// # Examples
///
/// In this example target, call `ui_bundle_code_editor_phase54_3_multiclick_selection_showcase(ShowcaseMode::White)` and pass the
/// returned view to the sandbox or capture harness.
pub fn ui_bundle_code_editor_phase54_3_multiclick_selection_showcase(
    mode: ShowcaseMode,
) -> impl IntoView<()> {
    let colors = mode.palette();
    Container::new()
        .fill()
        .background(colors.background)
        .padding(18.0)
        .child(section(
            mode,
            "section-code-editor-phase54-3-multiclick-selection",
            "CodeEditor Multi-click Selection",
            "Post-54.3 double-click word/token and gutter line selection scenario.",
            code_editor_phase54_3_multiclick_selection_section(mode),
        ))
}

#[cfg(test)]
#[allow(dead_code)]
/// Builds the `ui bundle code editor phase54 3 diagnostics showcase` deterministic retained-view scenario.
///
/// `mode` selects either the fixed white palette or default theme tokens; it
/// does not mutate global theme state.
///
/// # Examples
///
/// In this example target, call `ui_bundle_code_editor_phase54_3_diagnostics_showcase(ShowcaseMode::White)` and pass the
/// returned view to the sandbox or capture harness.
pub fn ui_bundle_code_editor_phase54_3_diagnostics_showcase(
    mode: ShowcaseMode,
) -> impl IntoView<()> {
    let colors = mode.palette();
    Container::new()
        .fill()
        .background(colors.background)
        .padding(18.0)
        .child(section(
            mode,
            "section-code-editor-phase54-3-diagnostics",
            "CodeEditor Diagnostics",
            "Phase 54.3 diagnostics with gutter markers and visible underlines.",
            code_editor_phase54_3_diagnostics_section(mode),
        ))
}

#[cfg(test)]
#[allow(dead_code)]
/// Builds the `ui bundle code editor phase54 3 folding showcase` deterministic retained-view scenario.
///
/// `mode` selects either the fixed white palette or default theme tokens; it
/// does not mutate global theme state.
///
/// # Examples
///
/// In this example target, call `ui_bundle_code_editor_phase54_3_folding_showcase(ShowcaseMode::White)` and pass the
/// returned view to the sandbox or capture harness.
pub fn ui_bundle_code_editor_phase54_3_folding_showcase(mode: ShowcaseMode) -> impl IntoView<()> {
    let colors = mode.palette();
    Container::new()
        .fill()
        .background(colors.background)
        .padding(18.0)
        .child(section(
            mode,
            "section-code-editor-phase54-3-folding",
            "CodeEditor Folding",
            "Phase 54.3 collapsed regions with gutter fold markers and placeholders.",
            code_editor_phase54_3_folding_section(mode),
        ))
}

#[cfg(test)]
#[allow(dead_code)]
/// Builds the `ui bundle code editor phase54 3 ide folding gutter showcase` deterministic retained-view scenario.
///
/// `mode` selects either the fixed white palette or default theme tokens; it
/// does not mutate global theme state.
///
/// # Examples
///
/// In this example target, call `ui_bundle_code_editor_phase54_3_ide_folding_gutter_showcase(ShowcaseMode::White)` and pass the
/// returned view to the sandbox or capture harness.
pub fn ui_bundle_code_editor_phase54_3_ide_folding_gutter_showcase(
    mode: ShowcaseMode,
) -> impl IntoView<()> {
    let colors = mode.palette();
    Container::new()
        .fill()
        .background(colors.background)
        .padding(18.0)
        .child(section(
            mode,
            "section-code-editor-phase54-3-ide-folding-gutter",
            "CodeEditor IDE Folding Gutter",
            "Post-54.3 folding gutter with IDE chevrons, guide rails and stable line-number reserve.",
            code_editor_phase54_3_ide_folding_gutter_section(mode),
        ))
}

#[cfg(test)]
#[allow(dead_code)]
/// Builds the `ui bundle code editor phase54 3 lsp showcase` deterministic retained-view scenario.
///
/// `mode` selects either the fixed white palette or default theme tokens; it
/// does not mutate global theme state.
///
/// # Examples
///
/// In this example target, call `ui_bundle_code_editor_phase54_3_lsp_showcase(ShowcaseMode::White)` and pass the
/// returned view to the sandbox or capture harness.
pub fn ui_bundle_code_editor_phase54_3_lsp_showcase(mode: ShowcaseMode) -> impl IntoView<()> {
    let colors = mode.palette();
    Container::new()
        .fill()
        .background(colors.background)
        .padding(18.0)
        .child(section(
            mode,
            "section-code-editor-phase54-3-lsp",
            "CodeEditor LSP Enrichment",
            "Phase 54.3 optional LSP diagnostics and semantic symbols from a mock backend.",
            code_editor_phase54_3_lsp_section(mode),
        ))
}

#[cfg(test)]
#[allow(dead_code)]
/// Builds the `ui bundle code editor phase54 3 scip showcase` deterministic retained-view scenario.
///
/// `mode` selects either the fixed white palette or default theme tokens; it
/// does not mutate global theme state.
///
/// # Examples
///
/// In this example target, call `ui_bundle_code_editor_phase54_3_scip_showcase(ShowcaseMode::White)` and pass the
/// returned view to the sandbox or capture harness.
pub fn ui_bundle_code_editor_phase54_3_scip_showcase(mode: ShowcaseMode) -> impl IntoView<()> {
    let colors = mode.palette();
    Container::new()
        .fill()
        .background(colors.background)
        .padding(18.0)
        .child(section(
            mode,
            "section-code-editor-phase54-3-scip",
            "CodeEditor SCIP Project Index",
            "Phase 54.3 optional SCIP project summary with cross-file navigation model.",
            code_editor_phase54_3_scip_section(mode),
        ))
}

#[cfg(test)]
#[allow(dead_code)]
/// Builds the `ui bundle code editor phase54 3 large file showcase` deterministic retained-view scenario.
///
/// `mode` selects either the fixed white palette or default theme tokens; it
/// does not mutate global theme state.
///
/// # Examples
///
/// In this example target, call `ui_bundle_code_editor_phase54_3_large_file_showcase(ShowcaseMode::White)` and pass the
/// returned view to the sandbox or capture harness.
pub fn ui_bundle_code_editor_phase54_3_large_file_showcase(
    mode: ShowcaseMode,
) -> impl IntoView<()> {
    let colors = mode.palette();
    Container::new()
        .fill()
        .background(colors.background)
        .padding(18.0)
        .child(section(
            mode,
            "section-code-editor-phase54-3-large-file",
            "CodeEditor Large File",
            "Phase 54.3 large NoWrap file scrolled deeply with instrumentation overlay.",
            code_editor_phase54_3_large_file_section(mode),
        ))
}

#[cfg(test)]
#[allow(dead_code)]
/// Builds the `ui bundle code editor phase54 3 theme variants showcase` deterministic retained-view scenario.
///
/// `mode` selects either the fixed white palette or default theme tokens; it
/// does not mutate global theme state.
///
/// # Examples
///
/// In this example target, call `ui_bundle_code_editor_phase54_3_theme_variants_showcase(ShowcaseMode::White)` and pass the
/// returned view to the sandbox or capture harness.
pub fn ui_bundle_code_editor_phase54_3_theme_variants_showcase(
    mode: ShowcaseMode,
) -> impl IntoView<()> {
    let colors = mode.palette();
    Container::new()
        .fill()
        .background(colors.background)
        .padding(18.0)
        .child(section(
            mode,
            "section-code-editor-phase54-3-theme-variants",
            "CodeEditor Theme Variants",
            "Phase 54.3 default and white CodeTheme variants using comparable Rust documents.",
            code_editor_phase54_3_theme_variants_section(mode),
        ))
}

#[cfg(test)]
#[allow(dead_code)]
/// Builds the `ui bundle line chart debug showcase` deterministic retained-view scenario.
///
/// `mode` selects either the fixed white palette or default theme tokens; it
/// does not mutate global theme state.
///
/// # Examples
///
/// In this example target, call `ui_bundle_line_chart_debug_showcase(ShowcaseMode::White)` and pass the
/// returned view to the sandbox or capture harness.
pub fn ui_bundle_line_chart_debug_showcase(mode: ShowcaseMode) -> impl IntoView<()> {
    let colors = mode.palette();
    let mut style = showcase_chart_style(mode, ChartSize::Large);
    style.line_thickness = 4.0;
    style.point_size = 7.0;

    Container::new()
        .fill()
        .background(colors.background)
        .padding(18.0)
        .child(
            LineChart::new()
                .series(
                    "Debug Stroke",
                    [
                        (0.0, 20.0),
                        (1.0, 80.0),
                        (2.0, 35.0),
                        (3.0, 75.0),
                        (4.0, 50.0),
                    ],
                )
                .x_range(0.0, 4.0)
                .range(0.0, 100.0)
                .show_points(true)
                .chart_style(style)
                .key("section-line-chart-debug"),
        )
}

/// Builds or computes the `build showcase` deterministic showcase fixture.
fn build_showcase(mode: ShowcaseMode) -> View<()> {
    let colors = mode.palette();

    Container::new()
        .fill()
        .background(colors.background)
        .child(ScrollView::vertical().child(
            Container::new().fill_width().padding(18.0).child(
                Column::new()
                    .gap(10.0)
                    .child(
                        Column::new()
                            .gap(4.0)
                            .child(text(mode.title(), 24, colors.text))
                            .child(text(
                                "Phase 39 scrollable showcase. Native widgets are shown as-is; missing widgets stay as planned placeholders.",
                                13,
                                colors.muted,
                            )),
                    )
                    .child(section(
                        mode,
                        "section-buttons",
                        "Buttons",
                        "Stable Button variants from Theme v1.",
                        buttons_section(mode),
                    ))
                    .child(section(
                        mode,
                        "section-badges-chips-tags",
                        "Badges, Chips & Tags",
                        "Phase 40 native compact status and filter pills.",
                        badges_chips_tags_section(),
                    ))
                    .child(section(
                        mode,
                        "section-text-inputs",
                        "Text Inputs",
                        "Bound single-line input states, placeholders, sizing and overflow.",
                        text_inputs_section(mode),
                    ))
                    .child(section(
                        mode,
                        "section-editor",
                        "Editor",
                        "Existing editor widget with a small code buffer.",
                        editor_section(mode),
                    ))
                    .child(section(
                        mode,
                        "section-code-editor",
                        "CodeEditor",
                        "Phase 54.2 code editor MVP with gutter and horizontal scroll.",
                        code_editor_section(mode),
                    ))
                    .child(section(
                        mode,
                        "section-terminal-view",
                        "TerminalView",
                        "Phase 54.1 read-only terminal widget with bounded history, search and selection.",
                        terminal_view_section(mode, TerminalShowcaseScenario::Default),
                    ))
                    .child(section(
                        mode,
                        "section-planned-widgets",
                        "Planned Widgets",
                        "Missing or partial widgets intentionally represented as roadmap placeholders.",
                        planned_widgets_section(mode),
                    ))
                    .child(section(
                        mode,
                        "section-switches",
                        "Switches",
                        "Phase 41 native boolean control states.",
                        switches_section(mode),
                    ))
                    .child(section(
                        mode,
                        "section-radio-groups",
                        "Radio Groups",
                        "Phase 42 native exclusive selection controls.",
                        radio_groups_section(mode),
                    ))
                    .child(section(
                        mode,
                        "section-segmented-controls",
                        "Segmented Controls",
                        "Phase 43 native exclusive segmented controls.",
                        segmented_controls_section(mode),
                    ))
                    .child(section(
                        mode,
                        "section-sliders",
                        "Sliders",
                        "Phase 44 native value and range controls.",
                        sliders_section(mode),
                    ))
                    .child(section(
                        mode,
                        "section-progress",
                        "Progress",
                        "Phase 45 native determinate progress indicators.",
                        progress_section(mode),
                    ))
                    .child(section(
                        mode,
                        "section-select-dropdowns",
                        "Select & Dropdown",
                        "Phase 46 native anchored popup controls.",
                        select_dropdowns_section(mode),
                    ))
                    .child(section(
                        mode,
                        "section-combobox-autocomplete",
                        "ComboBox & Autocomplete",
                        "Phase 47 native filtered text popup controls.",
                        combobox_autocomplete_section(mode),
                    ))
                    .child(section(
                        mode,
                        "section-cards-avatar-status-divider",
                        "Cards, Avatars & Indicators",
                        "Phase 48 native composition primitives.",
                        cards_avatar_status_divider_section(mode),
                    ))
                    .child(section(
                        mode,
                        "section-navigation-lists",
                        "Navigation & Lists",
                        "Phase 49 native navigation rows and simple list compositions.",
                        navigation_lists_section(mode),
                    ))
                    .child(section(
                        mode,
                        "section-accordion-tree",
                        "Accordion & TreeView",
                        "Phase 50 native disclosure and hierarchical controls.",
                        accordion_tree_section(mode),
                    ))
                    .child(section(
                        mode,
                        "section-tree-edit-drag",
                        "TreeView Edit & Drag",
                        "Phase 50.1 mutable tree operations and inline editing states.",
                        tree_edit_drag_section(mode),
                    ))
                    .child(section(
                        mode,
                        "section-table-view",
                        "TableView",
                        "Phase 51 native static data grid with sticky header and internal scroll.",
                        table_view_section(mode),
                    ))
                    .child(section(
                        mode,
                        "section-feedback-overlays",
                        "Feedback & Overlays",
                        "Phase 52 native toast host and confirmation dialog overlays.",
                        feedback_overlays_section(mode),
                    ))
                    .child(section(
                        mode,
                        "section-command-palette",
                        "Command Palette",
                        "Phase 52 native filtered action palette overlay.",
                        command_palette_section(mode),
                    ))
                    .child(section(
                        mode,
                        "section-pickers-upload",
                        "Pickers & Upload",
                        "Phase 53 native date, time, color and abstract upload controls.",
                        pickers_upload_section(mode),
                    ))
                    .child(section(
                        mode,
                        "section-charts",
                        "Charts",
                        "Phase 54 native simple charts rendered with existing primitives.",
                        charts_section(mode),
                    ))
                    .child(section(
                        mode,
                        "section-layout-boxes",
                        "Layout Boxes",
                        "Container surface, panel, border, radius, shadow and clipped scroll composition.",
                        layout_boxes_section(mode),
                    ))
                    .child(section(
                        mode,
                        "section-typography",
                        "Typography",
                        "UI text, muted text, mono text and paragraph sizing.",
                        typography_section(mode),
                    ))
                    .child(section(
                        mode,
                        "section-icons",
                        "Icons",
                        "Curated icon sources already exposed by the public API.",
                        icons_section(mode),
                    ))
                    .child(section(
                        mode,
                        "section-theme-tokens",
                        "Theme Tokens",
                        "Semantic colors used by the native controls.",
                        theme_tokens(mode),
                    ))
                    .child(section(
                        mode,
                        "section-scroll",
                        "Scroll",
                        "Nested ScrollView with long content.",
                        scroll_section(mode),
                    )),
            ),
        ))
        .key(mode.root_key())
}

/// Builds or computes the `section` deterministic showcase fixture.
fn section(
    mode: ShowcaseMode,
    key: &'static str,
    title: &'static str,
    subtitle: &'static str,
    child: impl IntoView<()>,
) -> View<()> {
    let colors = mode.palette();
    Container::new()
        .fill_width()
        .background(colors.surface)
        .border(1.0, colors.border)
        .radius(8.0)
        .shadow(BoxShadow::new(0.0, 8.0, 24.0, 0.0, colors.shadow))
        .padding(12.0)
        .child(
            Column::new()
                .gap(8.0)
                .child(
                    Column::new()
                        .gap(4.0)
                        .child(text(title, 15, colors.text))
                        .child(text(subtitle, 12, colors.muted)),
                )
                .child(child),
        )
        .key(key)
}

/// Builds or computes the `text` deterministic showcase fixture.
fn text(content: impl Into<String>, size: u16, color: Color) -> Text {
    Text::new(content.into()).style(TextStyle::new(FontId::Ui, size, color))
}

/// Builds or computes the `mono` deterministic showcase fixture.
fn mono(content: impl Into<String>, size: u16, color: Color) -> Text {
    Text::new(content.into()).style(TextStyle::new(FontId::Mono, size, color))
}

/// Builds or computes the `token card` deterministic showcase fixture.
fn token_card(label: &'static str, color: Color, text_color: Color, border: Color) -> View<()> {
    Container::new()
        .width(132.0)
        .height(82.0)
        .background(color)
        .border(1.0, border)
        .radius(8.0)
        .padding(10.0)
        .child(
            Column::new()
                .gap(6.0)
                .child(text(label, 12, text_color))
                .child(mono(
                    format!("{:?}", color.as_rgba8()),
                    11,
                    text_color.with_alpha(0.72),
                )),
        )
        .into_view()
}

/// Builds or computes the `theme tokens` deterministic showcase fixture.
fn theme_tokens(mode: ShowcaseMode) -> View<()> {
    let colors = mode.palette();
    Row::new()
        .gap(10.0)
        .child(token_card(
            "Background",
            colors.background,
            colors.text,
            colors.border,
        ))
        .child(token_card(
            "Surface",
            colors.surface,
            colors.text,
            colors.border,
        ))
        .child(token_card(
            "Elevated",
            colors.elevated,
            colors.text,
            colors.border,
        ))
        .child(token_card(
            "Border",
            colors.border,
            colors.text,
            colors.border,
        ))
        .child(token_card(
            "Accent",
            colors.accent,
            Color::WHITE,
            colors.accent,
        ))
        .child(token_card(
            "Danger",
            colors.danger,
            Color::WHITE,
            colors.danger,
        ))
        .child(token_card(
            "Success",
            colors.success,
            Color::WHITE,
            colors.success,
        ))
        .child(token_card(
            "Warning",
            colors.warning,
            Color::WHITE,
            colors.warning,
        ))
        .into_view()
}

/// Builds or computes the `buttons section` deterministic showcase fixture.
fn buttons_section(_mode: ShowcaseMode) -> View<()> {
    Column::new()
        .gap(10.0)
        .child(
            Row::new()
                .gap(8.0)
                .child(Button::<()>::with_label_variant(
                    "Primary",
                    ButtonVariant::Primary,
                ))
                .child(Button::<()>::with_label_variant(
                    "Secondary",
                    ButtonVariant::Secondary,
                ))
                .child(Button::<()>::with_label_variant(
                    "Outline",
                    ButtonVariant::Outline,
                ))
                .child(Button::<()>::with_label_variant(
                    "Ghost",
                    ButtonVariant::Ghost,
                ))
                .child(
                    Button::<()>::with_label_variant("Disabled", ButtonVariant::Secondary)
                        .disabled(true),
                ),
        )
        .child(
            Row::new()
                .gap(8.0)
                .child(Button::<()>::with_label_variant(
                    "Destructive",
                    ButtonVariant::Destructive,
                ))
                .child(Button::<()>::with_label_variant(
                    "Success",
                    ButtonVariant::Success,
                ))
                .child(Button::<()>::with_label_variant(
                    "Warning",
                    ButtonVariant::Warning,
                ))
                .child(Button::<()>::with_label_variant(
                    "Info",
                    ButtonVariant::Info,
                )),
        )
        .into_view()
}

/// Builds or computes the `badges chips tags section` deterministic showcase fixture.
fn badges_chips_tags_section() -> View<()> {
    Row::new()
        .gap(8.0)
        .child(Badge::new("Primary").tone(BadgeTone::Accent))
        .child(
            Badge::new("New")
                .tone(BadgeTone::Accent)
                .variant(BadgeVariant::Filled)
                .count(7),
        )
        .child(Badge::dot("Online").tone(BadgeTone::Success))
        .child(Badge::dot("Warning").tone(BadgeTone::Warning))
        .child(Badge::new("Danger").tone(BadgeTone::Danger))
        .child(Badge::new("Info").tone(BadgeTone::Info))
        .child(
            Badge::new("Checked")
                .leading_icon(IconId::Check)
                .tone(BadgeTone::Success)
                .variant(BadgeVariant::Outline),
        )
        .child(Tag::new("Filter"))
        .child(
            Tag::new("Muted")
                .tone(BadgeTone::Muted)
                .variant(BadgeVariant::Ghost),
        )
        .child(
            Chip::<()>::new("Closable")
                .tone(BadgeTone::Accent)
                .on_close_ctx(|_| {}),
        )
        .child(
            Chip::<()>::new("Disabled")
                .tone(BadgeTone::Danger)
                .on_close_ctx(|_| {})
                .disabled(true),
        )
        .into_view()
}

/// Builds or computes the `switches section` deterministic showcase fixture.
fn switches_section(mode: ShowcaseMode) -> View<()> {
    let colors = mode.palette();
    let mut focused_style = SwitchStyle::from_theme(Theme::default(), SwitchSize::Default);
    focused_style.border_on = focused_style.focus_ring;
    focused_style
        .shadows
        .push(BoxShadow::glow(colors.accent.with_alpha(0.22)));

    Row::new()
        .gap(18.0)
        .child(switch_sample(
            colors,
            "Off",
            Switch::<()>::new().checked(false),
        ))
        .child(switch_sample(
            colors,
            "On",
            Switch::<()>::new().checked(true),
        ))
        .child(switch_sample(
            colors,
            "Disabled off",
            Switch::<()>::new().checked(false).disabled(true),
        ))
        .child(switch_sample(
            colors,
            "Disabled on",
            Switch::<()>::new().checked(true).disabled(true),
        ))
        .child(switch_sample(
            colors,
            "Focused",
            Switch::<()>::new()
                .checked(true)
                .switch_style(focused_style),
        ))
        .child(switch_sample(
            colors,
            "Compact",
            Switch::<()>::new()
                .checked(true)
                .switch_size(SwitchSize::Compact),
        ))
        .into_view()
}

/// Builds or computes the `switch sample` deterministic showcase fixture.
fn switch_sample(colors: ShowcasePalette, label: &'static str, switch: Switch<()>) -> View<()> {
    Container::new()
        .width(146.0)
        .height(74.0)
        .background(colors.elevated)
        .border(1.0, colors.border)
        .radius(8.0)
        .padding(10.0)
        .child(
            Column::new()
                .gap(8.0)
                .child(switch)
                .child(text(label, 12, colors.text)),
        )
        .into_view()
}

#[derive(Clone, PartialEq)]
/// Closed scenario/value set used by the `ShowcaseChoice` showcase fixture.
enum ShowcaseChoice {
    /// First closed-set selection value.
    One,
    /// Second closed-set selection value.
    Two,
    /// Third closed-set selection value.
    Three,
}

/// Builds or computes the `radio groups section` deterministic showcase fixture.
fn radio_groups_section(mode: ShowcaseMode) -> View<()> {
    let colors = mode.palette();
    let mut focused_style = RadioStyle::from_theme(Theme::default(), RadioSize::Default);
    focused_style.selected_border = focused_style.focus_ring;

    Row::new()
        .gap(14.0)
        .child(radio_sample_card(
            colors,
            "Vertical",
            RadioGroup::<ShowcaseChoice>::new()
                .selected(ShowcaseChoice::One)
                .option(ShowcaseChoice::One, "Option one")
                .option(ShowcaseChoice::Two, "Option two")
                .radio_option(RadioOption::new(ShowcaseChoice::Three, "Disabled").disabled(true)),
        ))
        .child(radio_sample_card(
            colors,
            "Horizontal",
            RadioGroup::<ShowcaseChoice>::new()
                .selected(ShowcaseChoice::Two)
                .horizontal()
                .option(ShowcaseChoice::One, "Left")
                .option(ShowcaseChoice::Two, "Center")
                .option(ShowcaseChoice::Three, "Right"),
        ))
        .child(radio_sample_card(
            colors,
            "Disabled group",
            RadioGroup::<ShowcaseChoice>::new()
                .selected(ShowcaseChoice::One)
                .disabled(true)
                .option(ShowcaseChoice::One, "Enabled")
                .option(ShowcaseChoice::Two, "Muted"),
        ))
        .child(radio_sample_card(
            colors,
            "Focused",
            RadioGroup::<ShowcaseChoice>::new()
                .selected(ShowcaseChoice::Two)
                .radio_style(focused_style)
                .option(ShowcaseChoice::One, "First")
                .option(ShowcaseChoice::Two, "Focused"),
        ))
        .child(
            Container::new()
                .width(180.0)
                .min_height(112.0)
                .background(colors.elevated)
                .border(1.0, colors.border)
                .radius(8.0)
                .padding(10.0)
                .child(
                    Column::new()
                        .gap(10.0)
                        .child(text("Standalone", 12, colors.muted))
                        .child(RadioButton::<()>::new("Radio button").checked(true))
                        .child(
                            RadioButton::<()>::new("Compact")
                                .checked(false)
                                .radio_size(RadioSize::Compact),
                        ),
                ),
        )
        .into_view()
}

/// Builds or computes the `radio sample card` deterministic showcase fixture.
fn radio_sample_card(
    colors: ShowcasePalette,
    label: &'static str,
    group: RadioGroup<ShowcaseChoice>,
) -> View<()> {
    Container::new()
        .width(200.0)
        .min_height(112.0)
        .background(colors.elevated)
        .border(1.0, colors.border)
        .radius(8.0)
        .padding(10.0)
        .child(
            Column::new()
                .gap(8.0)
                .child(text(label, 12, colors.muted))
                .child(group),
        )
        .into_view()
}

/// Builds or computes the `segmented controls section` deterministic showcase fixture.
fn segmented_controls_section(mode: ShowcaseMode) -> View<()> {
    let colors = mode.palette();
    let mut focused_style = SegmentedStyle::from_theme(Theme::default(), SegmentedSize::Compact);
    focused_style.border = focused_style.focus_ring;

    Row::new()
        .gap(14.0)
        .child(segmented_sample_card(
            colors,
            "Default",
            SegmentedControl::<ShowcaseChoice>::new()
                .selected(ShowcaseChoice::Two)
                .width(280.0)
                .option(ShowcaseChoice::One, "Left")
                .option(ShowcaseChoice::Two, "Center")
                .option(ShowcaseChoice::Three, "Right"),
        ))
        .child(segmented_sample_card(
            colors,
            "Icons",
            SegmentedControl::<ShowcaseChoice>::new()
                .selected(ShowcaseChoice::One)
                .width(280.0)
                .segmented_option(
                    SegmentedOption::new(ShowcaseChoice::One, "List").leading_icon(IconId::Copy),
                )
                .segmented_option(
                    SegmentedOption::new(ShowcaseChoice::Two, "Add").leading_icon(IconId::Plus),
                )
                .segmented_option(
                    SegmentedOption::new(ShowcaseChoice::Three, "Done").leading_icon(IconId::Check),
                ),
        ))
        .child(segmented_sample_card(
            colors,
            "Disabled option",
            SegmentedControl::<ShowcaseChoice>::new()
                .selected(ShowcaseChoice::One)
                .width(280.0)
                .option(ShowcaseChoice::One, "Day")
                .segmented_option(SegmentedOption::new(ShowcaseChoice::Two, "Week").disabled(true))
                .option(ShowcaseChoice::Three, "Month"),
        ))
        .child(segmented_sample_card(
            colors,
            "Focused compact",
            SegmentedControl::<ShowcaseChoice>::new()
                .selected(ShowcaseChoice::Two)
                .width(240.0)
                .segmented_style(focused_style)
                .option(ShowcaseChoice::One, "One")
                .option(ShowcaseChoice::Two, "Two")
                .option(ShowcaseChoice::Three, "Three"),
        ))
        .child(segmented_sample_card(
            colors,
            "Disabled group",
            SegmentedControl::<ShowcaseChoice>::new()
                .selected(ShowcaseChoice::Three)
                .disabled(true)
                .width(240.0)
                .option(ShowcaseChoice::One, "Low")
                .option(ShowcaseChoice::Two, "Med")
                .option(ShowcaseChoice::Three, "High"),
        ))
        .into_view()
}

/// Builds or computes the `segmented sample card` deterministic showcase fixture.
fn segmented_sample_card(
    colors: ShowcasePalette,
    label: &'static str,
    control: SegmentedControl<ShowcaseChoice>,
) -> View<()> {
    Container::new()
        .width(300.0)
        .height(92.0)
        .background(colors.elevated)
        .border(1.0, colors.border)
        .radius(8.0)
        .padding(10.0)
        .child(
            Column::new()
                .gap(10.0)
                .child(text(label, 12, colors.muted))
                .child(control),
        )
        .into_view()
}

/// Builds or computes the `sliders section` deterministic showcase fixture.
fn sliders_section(mode: ShowcaseMode) -> View<()> {
    let colors = mode.palette();
    Column::new()
        .gap(10.0)
        .child(
            Row::new()
                .gap(14.0)
                .child(slider_sample_card(
                    colors,
                    "Default",
                    Column::new()
                        .gap(8.0)
                        .child(
                            Row::new()
                                .gap(8.0)
                                .child(text("72%", 12, colors.muted))
                                .child(
                                    Slider::<()>::new()
                                        .value(72.0)
                                        .range(0.0, 100.0)
                                        .width(260.0),
                                ),
                        )
                        .into_view(),
                ))
                .child(slider_sample_card(
                    colors,
                    "Range",
                    Column::new()
                        .gap(8.0)
                        .child(text("20% - 80%", 12, colors.muted))
                        .child(
                            RangeSlider::<()>::new()
                                .values(SliderRangeValue::new(20.0, 80.0))
                                .range(0.0, 100.0)
                                .width(260.0),
                        )
                        .into_view(),
                ))
                .child(slider_sample_card(
                    colors,
                    "Steps",
                    Column::new()
                        .gap(8.0)
                        .child(text("Step 25", 12, colors.muted))
                        .child(
                            Slider::<()>::new()
                                .value(50.0)
                                .range(0.0, 100.0)
                                .step(25.0)
                                .width(260.0),
                        )
                        .into_view(),
                )),
        )
        .child(
            Row::new()
                .gap(14.0)
                .child(slider_sample_card(
                    colors,
                    "Vertical",
                    Row::new()
                        .gap(14.0)
                        .child(Slider::<()>::vertical().value(35.0).height(150.0))
                        .child(Slider::<()>::vertical().value(85.0).height(150.0))
                        .into_view(),
                ))
                .child(slider_sample_card(
                    colors,
                    "Compact",
                    Column::new()
                        .gap(8.0)
                        .child(text("Compact size", 12, colors.muted))
                        .child(
                            Slider::<()>::new()
                                .value(42.0)
                                .slider_size(SliderSize::Compact)
                                .width(180.0),
                        )
                        .into_view(),
                ))
                .child(slider_sample_card(
                    colors,
                    "Disabled",
                    Column::new()
                        .gap(8.0)
                        .child(text("Disabled at 60%", 12, colors.muted))
                        .child(Slider::<()>::new().value(60.0).disabled(true).width(260.0))
                        .into_view(),
                )),
        )
        .into_view()
}

/// Builds or computes the `slider sample card` deterministic showcase fixture.
fn slider_sample_card(
    colors: ShowcasePalette,
    label: &'static str,
    content: impl IntoView<()>,
) -> View<()> {
    Container::new()
        .width(320.0)
        .height(120.0)
        .background(colors.elevated)
        .border(1.0, colors.border)
        .radius(8.0)
        .padding(10.0)
        .child(
            Column::new()
                .gap(10.0)
                .child(text(label, 12, colors.muted))
                .child(content),
        )
        .into_view()
}

/// Builds or computes the `progress section` deterministic showcase fixture.
fn progress_section(mode: ShowcaseMode) -> View<()> {
    let colors = mode.palette();
    Column::new()
        .gap(10.0)
        .child(
            Row::new()
                .gap(14.0)
                .child(progress_sample_card(
                    colors,
                    "Linear",
                    Column::new()
                        .gap(8.0)
                        .child(text("65%", 12, colors.muted))
                        .child(ProgressBar::new().value(0.65).width(260.0))
                        .into_view(),
                ))
                .child(progress_sample_card(
                    colors,
                    "Striped",
                    Column::new()
                        .gap(8.0)
                        .child(text("45%", 12, colors.muted))
                        .child(
                            ProgressBar::new()
                                .value(45.0)
                                .range(0.0, 100.0)
                                .variant(ProgressVariant::Striped)
                                .width(260.0),
                        )
                        .into_view(),
                ))
                .child(progress_sample_card(
                    colors,
                    "Sizes",
                    Column::new()
                        .gap(10.0)
                        .child(
                            ProgressBar::new()
                                .value(0.32)
                                .progress_size(ProgressSize::Compact)
                                .width(180.0),
                        )
                        .child(
                            ProgressBar::new()
                                .value(0.78)
                                .progress_size(ProgressSize::Large)
                                .width(260.0),
                        )
                        .into_view(),
                )),
        )
        .child(
            Row::new()
                .gap(14.0)
                .child(progress_sample_card(
                    colors,
                    "Disabled",
                    Column::new()
                        .gap(8.0)
                        .child(text("Disabled at 55%", 12, colors.muted))
                        .child(ProgressBar::new().value(0.55).disabled(true).width(260.0))
                        .into_view(),
                ))
                .child(progress_sample_card(
                    colors,
                    "Circular",
                    Row::new()
                        .gap(18.0)
                        .child(CircularProgress::new().value(0.25).show_label(true))
                        .child(CircularProgress::new().value(0.66).show_label(true))
                        .child(CircularProgress::new().value(1.0).show_label(true))
                        .into_view(),
                ))
                .child(progress_sample_card(
                    colors,
                    "Circular sizes",
                    Row::new()
                        .gap(18.0)
                        .child(
                            CircularProgress::new()
                                .value(0.42)
                                .progress_size(ProgressSize::Compact)
                                .show_label(true),
                        )
                        .child(
                            CircularProgress::new()
                                .value(0.85)
                                .progress_size(ProgressSize::Large)
                                .show_label(true),
                        )
                        .into_view(),
                )),
        )
        .into_view()
}

/// Builds or computes the `progress sample card` deterministic showcase fixture.
fn progress_sample_card(
    colors: ShowcasePalette,
    label: &'static str,
    content: impl IntoView<()>,
) -> View<()> {
    Container::new()
        .width(320.0)
        .height(122.0)
        .background(colors.elevated)
        .border(1.0, colors.border)
        .radius(8.0)
        .padding(10.0)
        .child(
            Column::new()
                .gap(10.0)
                .child(text(label, 12, colors.muted))
                .child(content),
        )
        .into_view()
}

/// Builds or computes the `select dropdowns section` deterministic showcase fixture.
fn select_dropdowns_section(mode: ShowcaseMode) -> View<()> {
    let colors = mode.palette();
    Column::new()
        .gap(10.0)
        .child(
            Row::new()
                .gap(14.0)
                .child(select_sample_card(
                    colors,
                    "Select closed",
                    Select::<ShowcaseChoice>::new()
                        .selected(ShowcaseChoice::One)
                        .option(ShowcaseChoice::One, "Apple")
                        .option(ShowcaseChoice::Two, "Apricot")
                        .select_option(
                            SelectOption::new(ShowcaseChoice::Three, "Avocado").disabled(true),
                        ),
                ))
                .child(select_sample_card(
                    colors,
                    "Select open",
                    Select::<ShowcaseChoice>::new()
                        .selected(ShowcaseChoice::Two)
                        .default_open(true)
                        .option(ShowcaseChoice::One, "Apple")
                        .option(ShowcaseChoice::Two, "Apricot")
                        .option(ShowcaseChoice::Three, "Banana"),
                ))
                .child(select_sample_card(
                    colors,
                    "Placeholder",
                    Select::<ShowcaseChoice>::new()
                        .placeholder("Select option")
                        .option(ShowcaseChoice::One, "North")
                        .option(ShowcaseChoice::Two, "South")
                        .option(ShowcaseChoice::Three, "West"),
                ))
                .child(select_sample_card(
                    colors,
                    "Disabled",
                    Select::<ShowcaseChoice>::new()
                        .selected(ShowcaseChoice::One)
                        .disabled(true)
                        .option(ShowcaseChoice::One, "Disabled select")
                        .option(ShowcaseChoice::Two, "Other"),
                )),
        )
        .child(
            Row::new()
                .gap(14.0)
                .child(select_sample_card(
                    colors,
                    "Long list",
                    long_select().default_open(true),
                ))
                .child(dropdown_sample_card(
                    colors,
                    "Dropdown open",
                    Dropdown::<()>::new("More")
                        .default_open(true)
                        .dropdown_item(
                            DropdownItem::new("Refresh")
                                .leading_icon(IconId::History)
                                .on_select(()),
                        )
                        .dropdown_item(
                            DropdownItem::new("Copy")
                                .leading_icon(IconId::Copy)
                                .on_select(()),
                        )
                        .dropdown_item(
                            DropdownItem::new("Delete")
                                .leading_icon(IconId::Trash)
                                .on_select(()),
                        )
                        .dropdown_item(DropdownItem::new("Disabled").disabled(true).on_select(())),
                ))
                .child(dropdown_sample_card(
                    colors,
                    "Dropdown closed",
                    Dropdown::<()>::new("Actions")
                        .dropdown_item(
                            DropdownItem::new("Add item")
                                .leading_icon(IconId::Plus)
                                .on_select(()),
                        )
                        .dropdown_item(
                            DropdownItem::new("Mark done")
                                .leading_icon(IconId::Check)
                                .on_select(()),
                        ),
                )),
        )
        .into_view()
}

/// Builds or computes the `long select` deterministic showcase fixture.
fn long_select() -> Select<ShowcaseChoice> {
    Select::<ShowcaseChoice>::new()
        .placeholder("Choose an item")
        .selected(ShowcaseChoice::One)
        .option(ShowcaseChoice::One, "Project Alpha")
        .option(ShowcaseChoice::Two, "Project Beta")
        .option(ShowcaseChoice::Three, "Project Gamma")
        .option(ShowcaseChoice::One, "Project Delta")
        .option(ShowcaseChoice::Two, "Project Epsilon")
        .option(ShowcaseChoice::Three, "Project Zeta")
        .option(ShowcaseChoice::One, "Project Eta")
        .option(ShowcaseChoice::Two, "Project Theta")
        .option(ShowcaseChoice::Three, "Project Iota")
}

/// Builds or computes the `select sample card` deterministic showcase fixture.
fn select_sample_card(
    colors: ShowcasePalette,
    label: &'static str,
    select: Select<ShowcaseChoice>,
) -> View<()> {
    Container::new()
        .width(260.0)
        .height(250.0)
        .background(colors.elevated)
        .border(1.0, colors.border)
        .radius(8.0)
        .padding(10.0)
        .child(
            Column::new()
                .gap(10.0)
                .child(text(label, 12, colors.muted))
                .child(select),
        )
        .into_view()
}

/// Builds or computes the `dropdown sample card` deterministic showcase fixture.
fn dropdown_sample_card(
    colors: ShowcasePalette,
    label: &'static str,
    dropdown: Dropdown<()>,
) -> View<()> {
    Container::new()
        .width(260.0)
        .height(250.0)
        .background(colors.elevated)
        .border(1.0, colors.border)
        .radius(8.0)
        .padding(10.0)
        .child(
            Column::new()
                .gap(10.0)
                .child(text(label, 12, colors.muted))
                .child(dropdown),
        )
        .into_view()
}

/// Builds or computes the `combobox autocomplete section` deterministic showcase fixture.
fn combobox_autocomplete_section(mode: ShowcaseMode) -> View<()> {
    let colors = mode.palette();
    Column::new()
        .gap(10.0)
        .child(
            Row::new()
                .gap(14.0)
                .child(combobox_sample_card(
                    colors,
                    "ComboBox closed",
                    ComboBox::<ShowcaseChoice>::new()
                        .selected(ShowcaseChoice::One)
                        .option(ShowcaseChoice::One, "Apple")
                        .option(ShowcaseChoice::Two, "Apricot")
                        .combo_option(
                            ComboBoxOption::new(ShowcaseChoice::Three, "Avocado").disabled(true),
                        ),
                ))
                .child(combobox_sample_card(
                    colors,
                    "Filtered open",
                    ComboBox::<ShowcaseChoice>::new()
                        .selected(ShowcaseChoice::One)
                        .default_query("ap")
                        .default_open(true)
                        .option(ShowcaseChoice::One, "Apple")
                        .option(ShowcaseChoice::Two, "Apricot")
                        .option(ShowcaseChoice::Three, "Banana"),
                ))
                .child(combobox_sample_card(
                    colors,
                    "No results",
                    ComboBox::<ShowcaseChoice>::new()
                        .default_query("zz")
                        .default_open(true)
                        .option(ShowcaseChoice::One, "Apple")
                        .option(ShowcaseChoice::Two, "Apricot")
                        .option(ShowcaseChoice::Three, "Banana"),
                )),
        )
        .child(
            Row::new()
                .gap(14.0)
                .child(autocomplete_sample_card(
                    colors,
                    "Suggestions open",
                    Autocomplete::<()>::new()
                        .bind(State::new("ap".to_string()))
                        .default_open(true)
                        .suggestion("Apple")
                        .autocomplete_item(AutocompleteItem::new("Apricot").disabled(true))
                        .suggestion("Avocado")
                        .suggestion("Banana"),
                ))
                .child(autocomplete_sample_card(
                    colors,
                    "Free text",
                    Autocomplete::<()>::new()
                        .bind(State::new("Custom value".to_string()))
                        .suggestion("Apple")
                        .suggestion("Apricot")
                        .suggestion("Banana"),
                ))
                .child(autocomplete_sample_card(
                    colors,
                    "Long list",
                    long_autocomplete().default_open(true),
                )),
        )
        .into_view()
}

/// Builds or computes the `long autocomplete` deterministic showcase fixture.
fn long_autocomplete() -> Autocomplete<()> {
    Autocomplete::<()>::new()
        .bind(State::new(String::new()))
        .placeholder("Search countries")
        .suggestion("Argentina")
        .suggestion("Australia")
        .suggestion("Austria")
        .suggestion("Belgium")
        .suggestion("Brazil")
        .suggestion("Canada")
        .suggestion("Denmark")
        .suggestion("France")
        .suggestion("Germany")
        .suggestion("Japan")
        .suggestion("United States")
}

/// Builds or computes the `combobox sample card` deterministic showcase fixture.
fn combobox_sample_card(
    colors: ShowcasePalette,
    label: &'static str,
    combo: ComboBox<ShowcaseChoice>,
) -> View<()> {
    Container::new()
        .width(280.0)
        .height(250.0)
        .background(colors.elevated)
        .border(1.0, colors.border)
        .radius(8.0)
        .padding(10.0)
        .child(
            Column::new()
                .gap(10.0)
                .child(text(label, 12, colors.muted))
                .child(combo),
        )
        .into_view()
}

/// Builds or computes the `autocomplete sample card` deterministic showcase fixture.
fn autocomplete_sample_card(
    colors: ShowcasePalette,
    label: &'static str,
    autocomplete: Autocomplete<()>,
) -> View<()> {
    Container::new()
        .width(280.0)
        .height(250.0)
        .background(colors.elevated)
        .border(1.0, colors.border)
        .radius(8.0)
        .padding(10.0)
        .child(
            Column::new()
                .gap(10.0)
                .child(text(label, 12, colors.muted))
                .child(autocomplete),
        )
        .into_view()
}

/// Builds or computes the `cards avatar status divider section` deterministic showcase fixture.
fn cards_avatar_status_divider_section(mode: ShowcaseMode) -> View<()> {
    let colors = mode.palette();
    Column::new()
        .gap(12.0)
        .child(
            Row::new()
                .gap(14.0)
                .child(profile_card(colors))
                .child(metric_card(colors))
                .child(media_like_card(colors)),
        )
        .child(
            Row::new()
                .gap(14.0)
                .child(avatar_samples(colors))
                .child(status_samples(colors))
                .child(divider_samples(colors)),
        )
        .into_view()
}

/// Builds or computes the `profile card` deterministic showcase fixture.
fn profile_card(colors: ShowcasePalette) -> View<()> {
    Card::<()>::elevated()
        .width(300.0)
        .height(150.0)
        .child(
            Column::new()
                .gap(12.0)
                .child(
                    Row::new()
                        .gap(10.0)
                        .child(
                            Avatar::new("Alex Rivera")
                                .tone(AvatarTone::Accent)
                                .size(46.0),
                        )
                        .child(
                            Column::new()
                                .gap(4.0)
                                .child(text("Alex Rivera", 14, colors.text))
                                .child(text("Product Designer", 12, colors.muted))
                                .child(text("San Francisco, CA", 11, colors.muted)),
                        ),
                )
                .child(
                    Row::new()
                        .gap(14.0)
                        .child(metric_text("2.4K", "Followers", colors))
                        .child(metric_text("847", "Following", colors))
                        .child(Badge::dot("Online").tone(BadgeTone::Success)),
                ),
        )
        .into_view()
}

/// Builds or computes the `metric card` deterministic showcase fixture.
fn metric_card(colors: ShowcasePalette) -> View<()> {
    Card::<()>::surface()
        .width(230.0)
        .height(150.0)
        .child(
            Column::new()
                .gap(10.0)
                .child(text("Total Revenue", 12, colors.muted))
                .child(text("$24,780", 24, colors.text))
                .child(Badge::new("+12.5%").tone(BadgeTone::Success))
                .child(
                    Row::new()
                        .gap(5.0)
                        .child(
                            StatusIndicator::new(StatusTone::Accent)
                                .variant(StatusVariant::Bars)
                                .size(22.0),
                        )
                        .child(
                            Divider::horizontal()
                                .length(128.0)
                                .thickness(2.0)
                                .color(colors.accent),
                        ),
                ),
        )
        .into_view()
}

/// Builds or computes the `media like card` deterministic showcase fixture.
fn media_like_card(colors: ShowcasePalette) -> View<()> {
    Card::<()>::new()
        .variant(CardVariant::Outline)
        .width(300.0)
        .height(150.0)
        .child(
            Column::new()
                .gap(10.0)
                .child(
                    Container::new()
                        .height(64.0)
                        .fill_width()
                        .background(colors.accent.with_alpha(0.18))
                        .border(1.0, colors.accent.with_alpha(0.34))
                        .radius(8.0)
                        .padding(12.0)
                        .child(
                            Row::new()
                                .gap(10.0)
                                .child(Icon::new(IconId::History).size(24.0).tint(colors.accent))
                                .child(text("Media card placeholder", 13, colors.text)),
                        ),
                )
                .child(text("Beyond the Horizon", 14, colors.text))
                .child(text(
                    "Image-backed cards remain planned for bitmap rendering.",
                    12,
                    colors.muted,
                )),
        )
        .into_view()
}

/// Builds or computes the `avatar samples` deterministic showcase fixture.
fn avatar_samples(colors: ShowcasePalette) -> View<()> {
    sample_panel(
        colors,
        "Avatars",
        Column::new()
            .gap(12.0)
            .child(
                Row::new()
                    .gap(8.0)
                    .child(Avatar::new("Alex Rivera").tone(AvatarTone::Accent))
                    .child(Avatar::new("Maya Chen").tone(AvatarTone::Success))
                    .child(Avatar::initials("JD").tone(AvatarTone::Warning))
                    .child(Avatar::icon(IconId::Check).tone(AvatarTone::Info)),
            )
            .child(
                Row::new()
                    .gap(6.0)
                    .child(
                        Avatar::new("Jordan Kim")
                            .tone(AvatarTone::Neutral)
                            .size(30.0),
                    )
                    .child(
                        Avatar::new("Taylor Smith")
                            .tone(AvatarTone::Muted)
                            .size(30.0),
                    )
                    .child(Tag::new("+3").tone(BadgeTone::Muted)),
            ),
    )
}

/// Builds or computes the `status samples` deterministic showcase fixture.
fn status_samples(colors: ShowcasePalette) -> View<()> {
    sample_panel(
        colors,
        "Status indicators",
        Column::new()
            .gap(10.0)
            .child(status_row(
                "Online",
                StatusTone::Success,
                StatusVariant::Dot,
                colors,
            ))
            .child(status_row(
                "Warning",
                StatusTone::Warning,
                StatusVariant::Ring,
                colors,
            ))
            .child(status_row(
                "Danger",
                StatusTone::Danger,
                StatusVariant::Dot,
                colors,
            ))
            .child(status_row(
                "Activity",
                StatusTone::Info,
                StatusVariant::Bars,
                colors,
            )),
    )
}

/// Builds or computes the `divider samples` deterministic showcase fixture.
fn divider_samples(colors: ShowcasePalette) -> View<()> {
    sample_panel(
        colors,
        "Dividers",
        Row::new()
            .gap(14.0)
            .child(
                Column::new()
                    .gap(10.0)
                    .child(Divider::horizontal().length(170.0).color(colors.border))
                    .child(
                        Divider::horizontal()
                            .variant(DividerVariant::Dashed)
                            .length(170.0)
                            .color(colors.muted),
                    )
                    .child(
                        Divider::horizontal()
                            .variant(DividerVariant::Dotted)
                            .thickness(2.0)
                            .length(170.0)
                            .color(colors.accent),
                    ),
            )
            .child(Divider::vertical().length(78.0).color(colors.border))
            .child(
                Divider::vertical()
                    .variant(DividerVariant::Dashed)
                    .length(78.0)
                    .color(colors.accent),
            ),
    )
}

/// Builds or computes the `metric text` deterministic showcase fixture.
fn metric_text(value: &'static str, label: &'static str, colors: ShowcasePalette) -> View<()> {
    Column::new()
        .gap(2.0)
        .child(text(value, 14, colors.text))
        .child(text(label, 11, colors.muted))
        .into_view()
}

/// Builds or computes the `status row` deterministic showcase fixture.
fn status_row(
    label: &'static str,
    tone: StatusTone,
    variant: StatusVariant,
    colors: ShowcasePalette,
) -> View<()> {
    Row::new()
        .gap(8.0)
        .child(StatusIndicator::new(tone).variant(variant).size(14.0))
        .child(text(label, 12, colors.text))
        .into_view()
}

/// Builds or computes the `sample panel` deterministic showcase fixture.
fn sample_panel(
    colors: ShowcasePalette,
    title: &'static str,
    content: impl IntoView<()>,
) -> View<()> {
    Container::new()
        .width(300.0)
        .height(128.0)
        .background(colors.elevated)
        .border(1.0, colors.border)
        .radius(8.0)
        .padding(10.0)
        .child(
            Column::new()
                .gap(10.0)
                .child(text(title, 12, colors.muted))
                .child(content),
        )
        .into_view()
}

/// Builds or computes the `navigation lists section` deterministic showcase fixture.
fn navigation_lists_section(mode: ShowcaseMode) -> View<()> {
    let colors = mode.palette();
    Row::new()
        .gap(14.0)
        .child(navigation_sample_panel(
            colors,
            "Sidebar",
            270.0,
            260.0,
            Sidebar::<()>::new()
                .title("Workspace")
                .nav_item(
                    NavItem::new("Dashboard")
                        .leading_icon(IconId::Plus)
                        .selected(true)
                        .on_select(()),
                )
                .nav_item(
                    NavItem::new("Messages")
                        .leading_icon(IconId::Copy)
                        .badge(3)
                        .on_select(()),
                )
                .nav_item(
                    NavItem::new("Archive")
                        .leading_icon(IconId::History)
                        .on_select(()),
                )
                .nav_item(
                    NavItem::new("Disabled")
                        .leading_icon(IconId::Check)
                        .disabled(true)
                        .on_select(()),
                )
                .nav_item(
                    NavItem::new("Trash")
                        .leading_icon(IconId::Trash)
                        .variant(NavItemVariant::Danger)
                        .on_select(()),
                ),
        ))
        .child(navigation_sample_panel(
            colors,
            "ListView",
            300.0,
            260.0,
            ListView::<()>::new()
                .item(
                    ListItem::new("Inbox")
                        .leading_icon(IconId::Copy)
                        .trailing_text("12")
                        .selected(true)
                        .on_select(()),
                )
                .item(
                    ListItem::new("Starred")
                        .leading_icon(IconId::Check)
                        .badge(2)
                        .on_select(()),
                )
                .item(
                    ListItem::new("Sent")
                        .leading_icon(IconId::Plus)
                        .on_select(()),
                )
                .item(ListItem::new("Drafts").subtitle("3 local changes"))
                .item(ListItem::new("Archive").leading_icon(IconId::History))
                .item(
                    ListItem::new("Trash")
                        .leading_icon(IconId::Trash)
                        .variant(ListItemVariant::Danger),
                ),
        ))
        .child(navigation_sample_panel(
            colors,
            "Disclosure Rows",
            330.0,
            260.0,
            Column::new()
                .gap(4.0)
                .child(
                    DisclosureRow::<()>::new("Wi-Fi")
                        .leading_icon(IconId::Check)
                        .trailing_text("Connected")
                        .selected(true)
                        .on_select(()),
                )
                .child(
                    DisclosureRow::<()>::new("Bluetooth")
                        .leading_icon(IconId::Plus)
                        .trailing_text("On")
                        .on_select(()),
                )
                .child(
                    DisclosureRow::<()>::new("Notifications")
                        .leading_icon(IconId::History)
                        .trailing_text("Off")
                        .on_select(()),
                )
                .child(
                    DisclosureRow::<()>::new("Privacy")
                        .leading_icon(IconId::Copy)
                        .on_select(()),
                )
                .child(
                    DisclosureRow::<()>::new("About")
                        .leading_icon(IconId::Trash)
                        .variant(DisclosureRowVariant::Danger)
                        .on_select(()),
                ),
        ))
        .into_view()
}

/// Builds or computes the `navigation sample panel` deterministic showcase fixture.
fn navigation_sample_panel(
    colors: ShowcasePalette,
    title: &'static str,
    width: f32,
    height: f32,
    content: impl IntoView<()>,
) -> View<()> {
    Container::new()
        .width(width)
        .height(height)
        .background(colors.elevated)
        .border(1.0, colors.border)
        .radius(8.0)
        .padding(10.0)
        .child(
            Column::new()
                .gap(10.0)
                .child(text(title, 12, colors.muted))
                .child(content),
        )
        .into_view()
}

/// Builds or computes the `accordion tree section` deterministic showcase fixture.
fn accordion_tree_section(mode: ShowcaseMode) -> View<()> {
    let colors = mode.palette();
    Row::new()
        .gap(14.0)
        .child(accordion_tree_sample_panel(
            colors,
            "Accordion single",
            300.0,
            260.0,
            Accordion::<()>::new()
                .single()
                .default_open("what")
                .item(AccordionItem::new("what", "What is this?").child(text(
                    "Accordion content can contain any public view.",
                    12,
                    colors.text,
                )))
                .item(AccordionItem::new("how", "How does it work?").child(text(
                    "Only one section is open in single mode.",
                    12,
                    colors.text,
                )))
                .item(
                    AccordionItem::new("disabled", "Disabled section")
                        .disabled(true)
                        .child(text("This should stay closed.", 12, colors.muted)),
                ),
        ))
        .child(accordion_tree_sample_panel(
            colors,
            "Accordion multiple",
            320.0,
            260.0,
            Accordion::<()>::new()
                .multiple()
                .default_open_many(["usage", "custom"])
                .item(AccordionItem::new("usage", "Usage").child(text(
                    "Multiple sections may stay expanded.",
                    12,
                    colors.text,
                )))
                .item(
                    AccordionItem::new("custom", "Can I customize it?").child(text(
                        "Styles come from Theme::default tokens.",
                        12,
                        colors.text,
                    )),
                )
                .item(
                    AccordionItem::new("compact", "Compact size")
                        .child(Badge::new("Planned sample").tone(BadgeTone::Accent)),
                ),
        ))
        .child(accordion_tree_sample_panel(
            colors,
            "TreeView",
            360.0,
            260.0,
            TreeView::<&'static str>::new()
                .selected("src")
                .default_expanded("root")
                .default_expanded("src")
                .node(showcase_tree())
                .width(320.0),
        ))
        .into_view()
}

/// Builds or computes the `tree edit drag section` deterministic showcase fixture.
fn tree_edit_drag_section(mode: ShowcaseMode) -> View<()> {
    let colors = mode.palette();
    let editable_nodes = State::new(vec![showcase_tree()]);
    Row::new()
        .gap(14.0)
        .child(accordion_tree_sample_panel(
            colors,
            "Mutable TreeView",
            360.0,
            280.0,
            TreeView::<&'static str>::new()
                .bind_nodes(editable_nodes)
                .selected("src")
                .default_expanded("root")
                .default_expanded("src")
                .draggable(true)
                .editable(true)
                .creatable(true)
                .deletable(true)
                .create_node_with(|request| {
                    Some(TreeNode::leaf(
                        if request.kind == TreeCreateKind::Child {
                            "new-child"
                        } else {
                            "new-sibling"
                        },
                        request.default_label,
                    ))
                })
                .width(320.0),
        ))
        .child(accordion_tree_sample_panel(
            colors,
            "Inline rename",
            300.0,
            280.0,
            Column::new()
                .gap(10.0)
                .child(text(
                    "F2 starts rename on the active row.",
                    12,
                    colors.muted,
                ))
                .child(
                    TextInput::new()
                        .bind(State::new("renaming.rs".to_string()))
                        .width(220.0),
                )
                .child(text(
                    "Enter commits, Escape cancels, blur commits.",
                    12,
                    colors.text,
                )),
        ))
        .child(accordion_tree_sample_panel(
            colors,
            "Drop targets",
            300.0,
            280.0,
            Column::new()
                .gap(10.0)
                .child(drop_sample_row(
                    colors,
                    TreeDropPosition::Before,
                    "Before row",
                ))
                .child(drop_sample_row(
                    colors,
                    TreeDropPosition::Inside,
                    "Inside row",
                ))
                .child(drop_sample_row(
                    colors,
                    TreeDropPosition::After,
                    "After row",
                ))
                .child(text(
                    "Disabled nodes reject drag, rename, create and delete.",
                    12,
                    colors.muted,
                )),
        ))
        .into_view()
}

/// Builds or computes the `table view section` deterministic showcase fixture.
fn table_view_section(mode: ShowcaseMode) -> View<()> {
    let colors = mode.palette();
    let selected = State::new("alex");
    Row::new()
        .gap(14.0)
        .child(
            Container::new()
                .width(700.0)
                .height(260.0)
                .background(colors.elevated)
                .border(1.0, colors.border)
                .radius(8.0)
                .padding(12.0)
                .child(
                    Column::new()
                        .gap(10.0)
                        .child(text("Data grid", 12, colors.muted))
                        .child(
                            TableView::<&'static str>::new()
                                .table_style(showcase_table_style(mode))
                                .width(650.0)
                                .max_body_height(150.0)
                                .bind_selected(selected)
                                .column(TableColumn::new("Name").width(150.0))
                                .column(TableColumn::new("Role").width(116.0))
                                .column(TableColumn::new("Status").width(104.0))
                                .column(TableColumn::new("Progress").width(126.0))
                                .column(
                                    TableColumn::new("Date").width(132.0).align(TableAlign::End),
                                )
                                .column(
                                    TableColumn::new("Score").width(78.0).align(TableAlign::End),
                                )
                                .row(table_row(
                                    "alex",
                                    "Alex Rivera",
                                    "Designer",
                                    "Active",
                                    BadgeTone::Success,
                                    0.72,
                                    "May 14, 2024",
                                    "72%",
                                    Some(IconId::Check),
                                    false,
                                ))
                                .row(table_row(
                                    "maya",
                                    "Maya Chen",
                                    "Developer",
                                    "Active",
                                    BadgeTone::Success,
                                    0.63,
                                    "May 13, 2024",
                                    "63%",
                                    None,
                                    false,
                                ))
                                .row(table_row(
                                    "jordan",
                                    "Jordan Kim",
                                    "Product",
                                    "Pending",
                                    BadgeTone::Warning,
                                    0.28,
                                    "May 12, 2024",
                                    "28%",
                                    None,
                                    false,
                                ))
                                .row(table_row(
                                    "taylor",
                                    "Taylor Smith",
                                    "Marketing",
                                    "Inactive",
                                    BadgeTone::Muted,
                                    0.0,
                                    "May 11, 2024",
                                    "0%",
                                    None,
                                    true,
                                ))
                                .row(table_row(
                                    "sam",
                                    "Sam Patel",
                                    "Operations",
                                    "Blocked",
                                    BadgeTone::Danger,
                                    0.14,
                                    "May 10, 2024",
                                    "14%",
                                    None,
                                    false,
                                )),
                        ),
                ),
        )
        .child(
            Container::new()
                .width(250.0)
                .height(260.0)
                .background(colors.elevated)
                .border(1.0, colors.border)
                .radius(8.0)
                .padding(12.0)
                .child(
                    Column::new()
                        .gap(8.0)
                        .child(text("V1 behavior", 12, colors.muted))
                        .child(Badge::new("Sticky header").tone(BadgeTone::Accent))
                        .child(Badge::new("Internal scroll").tone(BadgeTone::Info))
                        .child(Badge::new("No virtualization").tone(BadgeTone::Warning))
                        .child(text(
                            "Rows are static and cells are text, badge or progress primitives.",
                            12,
                            colors.text,
                        ))
                        .child(text(
                            "Future phases can add sort, resize, edit and virtualized data.",
                            12,
                            colors.muted,
                        )),
                ),
        )
        .into_view()
}

/// Builds or computes the `feedback overlays section` deterministic showcase fixture.
fn feedback_overlays_section(mode: ShowcaseMode) -> View<()> {
    let colors = mode.palette();
    Row::new()
        .gap(14.0)
        .child(
            Container::new()
                .width(470.0)
                .height(300.0)
                .background(colors.elevated)
                .border(1.0, colors.border)
                .radius(8.0)
                .clip_children(true)
                .child(
                    ToastHost::new()
                        .fill()
                        .toast_style(showcase_toast_style(mode))
                        .position(ToastPosition::TopRight)
                        .toasts(vec![
                            Toast::new("saved", "Changes saved")
                                .description("Project settings were updated successfully.")
                                .tone(ToastTone::Success)
                                .leading_icon(IconId::Check),
                            Toast::new("network", "Network connection lost")
                                .description("Trying to reconnect in the background.")
                                .tone(ToastTone::Warning)
                                .leading_icon(IconId::History),
                            Toast::new("deleted", "Item deleted")
                                .tone(ToastTone::Danger)
                                .leading_icon(IconId::Trash),
                        ])
                        .child(
                            Column::new()
                                .gap(10.0)
                                .padding(14.0)
                                .child(text("ToastHost", 12, colors.muted))
                                .child(text(
                                    "Toasts are top-level overlay commands with close hit regions.",
                                    12,
                                    colors.text,
                                ))
                                .child(Badge::new("No timer in V1").tone(BadgeTone::Info)),
                        ),
                ),
        )
        .child(
            Container::new()
                .width(470.0)
                .height(300.0)
                .background(colors.elevated)
                .border(1.0, colors.border)
                .radius(8.0)
                .clip_children(true)
                .child(
                    Dialog::<()>::new()
                        .fill()
                        .default_open(true)
                        .tone(DialogTone::Danger)
                        .dialog_style(showcase_dialog_style(mode, DialogTone::Danger))
                        .title("Delete Project")
                        .body("Are you sure you want to delete this project? This action cannot be undone.")
                        .cancel_label("Cancel")
                        .confirm_label("Delete")
                        .on_cancel(())
                        .on_confirm(())
                        .child(
                            Column::new()
                                .gap(10.0)
                                .padding(14.0)
                                .child(text("Dialog", 12, colors.muted))
                                .child(text(
                                    "Backdrop click and Escape cancel; buttons dispatch actions.",
                                    12,
                                    colors.text,
                                ))
                                .child(Badge::new("bind_open supported").tone(BadgeTone::Accent)),
                        ),
                ),
        )
        .into_view()
}

/// Builds or computes the `command palette section` deterministic showcase fixture.
fn command_palette_section(mode: ShowcaseMode) -> View<()> {
    let colors = mode.palette();
    Container::new()
        .width(960.0)
        .height(360.0)
        .background(colors.elevated)
        .border(1.0, colors.border)
        .radius(8.0)
        .clip_children(true)
        .child(
            CommandPalette::<()>::new()
                .fill()
                .default_open(true)
                .default_query("se")
                .placeholder("Type a command...")
                .command_style(showcase_command_palette_style(mode))
                .item(
                    CommandItem::new("Go to File")
                        .subtitle("Open a file by name")
                        .shortcut("Ctrl+P")
                        .keyword("search")
                        .leading_icon(IconId::Copy)
                        .on_select(()),
                )
                .item(
                    CommandItem::new("Search")
                        .subtitle("Find text in the workspace")
                        .shortcut("Ctrl+F")
                        .keyword("find")
                        .leading_icon(IconId::History)
                        .on_select(()),
                )
                .item(
                    CommandItem::new("Run Command")
                        .subtitle("Execute a task")
                        .shortcut("Ctrl+Shift+P")
                        .keyword("command")
                        .leading_icon(IconId::Plus)
                        .on_select(()),
                )
                .item(
                    CommandItem::new("Settings")
                        .subtitle("Open preferences")
                        .shortcut("Ctrl+,")
                        .keyword("preferences")
                        .leading_icon(IconId::Check)
                        .on_select(()),
                )
                .item(
                    CommandItem::new("Toggle Terminal")
                        .subtitle("Disabled sample item")
                        .shortcut("Ctrl+`")
                        .leading_icon(IconId::History)
                        .disabled(true)
                        .on_select(()),
                )
                .child(
                    Column::new()
                        .gap(10.0)
                        .padding(16.0)
                        .child(text("CommandPalette", 12, colors.muted))
                        .child(text(
                            "The overlay captures outside clicks, filters items and reuses the single-line text core.",
                            12,
                            colors.text,
                        ))
                        .child(
                            Row::new()
                                .gap(8.0)
                                .child(Badge::new("Arrow navigation").tone(BadgeTone::Accent))
                                .child(Badge::new("IME input role").tone(BadgeTone::Info))
                                .child(Badge::new("Disabled rows").tone(BadgeTone::Warning)),
                        ),
                ),
        )
        .into_view()
}

/// Builds or computes the `pickers upload section` deterministic showcase fixture.
fn pickers_upload_section(mode: ShowcaseMode) -> View<()> {
    let colors = mode.palette();
    let date = State::new(Some(DateValue::new(2026, 5, 29)));
    let time = State::new(Some(TimeValue::new(14, 30)));
    let color = State::new(Color::hex_rgb(0xFF5A00));
    let closed_date = State::new(None::<DateValue>);
    let closed_time = State::new(None::<TimeValue>);
    let closed_color = State::new(Color::hex_rgb(0x22C55E));

    Column::new()
        .gap(12.0)
        .child(
            Row::new()
                .gap(12.0)
                .child(picker_demo_card(
                    mode,
                    "DatePicker open",
                    DatePicker::<()>::new()
                        .bind(date)
                        .default_month(MonthValue::new(2026, 5))
                        .default_open(true)
                        .min(DateValue::new(2026, 1, 1))
                        .max(DateValue::new(2026, 12, 31))
                        .date_style(showcase_date_picker_style(mode)),
                ))
                .child(picker_demo_card(
                    mode,
                    "DatePicker closed",
                    DatePicker::<()>::new()
                        .bind(closed_date)
                        .placeholder("Select day")
                        .date_style(showcase_date_picker_style(mode)),
                ))
                .child(picker_demo_card(
                    mode,
                    "TimePicker open",
                    TimePicker::<()>::new()
                        .bind(time)
                        .default_open(true)
                        .step_minutes(5)
                        .format(TimeFormat::Hour24)
                        .time_style(showcase_time_picker_style(mode)),
                ))
                .child(picker_demo_card(
                    mode,
                    "TimePicker closed",
                    TimePicker::<()>::new()
                        .bind(closed_time)
                        .placeholder("Select time")
                        .time_style(showcase_time_picker_style(mode)),
                )),
        )
        .child(
            Row::new()
                .gap(12.0)
                .child(picker_demo_card(
                    mode,
                    "ColorPicker open",
                    ColorPicker::<()>::new()
                        .bind(color)
                        .default_open(true)
                        .swatch(colors.accent)
                        .swatch(colors.success)
                        .swatch(colors.warning)
                        .swatch(colors.danger)
                        .color_style(showcase_color_picker_style(mode)),
                ))
                .child(picker_demo_card(
                    mode,
                    "ColorPicker closed",
                    ColorPicker::<()>::new()
                        .bind(closed_color)
                        .swatch(colors.info)
                        .swatch(colors.success)
                        .color_style(showcase_color_picker_style(mode)),
                ))
                .child(upload_demo_card(
                    mode,
                    "UploadDropzone",
                    UploadDropzone::<()>::new()
                        .title("Drag & drop files here")
                        .description("PNG, JPG or PDF")
                        .multiple(true)
                        .accept([".png", ".jpg", ".pdf"])
                        .upload_style(showcase_upload_style(mode)),
                ))
                .child(upload_demo_card(
                    mode,
                    "Upload disabled",
                    UploadDropzone::<()>::new()
                        .title("Upload disabled")
                        .description("No files accepted")
                        .disabled(true)
                        .upload_style(showcase_upload_style(mode)),
                )),
        )
        .child(
            Row::new()
                .gap(8.0)
                .child(Badge::new("No native file dialog").tone(BadgeTone::Info))
                .child(Badge::new("No file IO").tone(BadgeTone::Warning))
                .child(Badge::new("Overlay popups").tone(BadgeTone::Accent)),
        )
        .into_view()
}

/// Builds or computes the `picker demo card` deterministic showcase fixture.
fn picker_demo_card(
    mode: ShowcaseMode,
    label: &'static str,
    picker: impl IntoView<()>,
) -> View<()> {
    let colors = mode.palette();
    Container::new()
        .width(230.0)
        .height(360.0)
        .background(colors.elevated)
        .border(1.0, colors.border)
        .radius(8.0)
        .padding(12.0)
        .clip_children(false)
        .child(
            Column::new()
                .gap(8.0)
                .child(text(label, 12, colors.muted))
                .child(picker),
        )
        .into_view()
}

/// Builds or computes the `upload demo card` deterministic showcase fixture.
fn upload_demo_card(
    mode: ShowcaseMode,
    label: &'static str,
    upload: impl IntoView<()>,
) -> View<()> {
    let colors = mode.palette();
    Container::new()
        .width(360.0)
        .height(190.0)
        .background(colors.elevated)
        .border(1.0, colors.border)
        .radius(8.0)
        .padding(12.0)
        .child(
            Column::new()
                .gap(8.0)
                .child(text(label, 12, colors.muted))
                .child(upload),
        )
        .into_view()
}

/// Builds or computes the `charts section` deterministic showcase fixture.
fn charts_section(mode: ShowcaseMode) -> View<()> {
    let colors = mode.palette();
    let default_style = showcase_chart_style(mode, ChartSize::Default);
    let compact_style = showcase_chart_style(mode, ChartSize::Compact);

    Column::new()
        .gap(12.0)
        .child(
            Row::new()
                .gap(12.0)
                .child(
                    BarChart::new()
                        .series("Revenue", [12.0, 18.0, 14.0, 24.0, 20.0, 29.0, 34.0])
                        .labels(["Mon", "Tue", "Wed", "Thu", "Fri", "Sat", "Sun"])
                        .chart_style(default_style.clone()),
                )
                .child(
                    LineChart::new()
                        .series(
                            "Sessions",
                            [
                                (0.0, 18.0),
                                (1.0, 28.0),
                                (2.0, 22.0),
                                (3.0, 38.0),
                                (4.0, 30.0),
                                (5.0, 46.0),
                                (6.0, 42.0),
                            ],
                        )
                        .show_points(true)
                        .chart_style(default_style.clone()),
                )
                .child(
                    RadialGauge::new()
                        .value(0.72)
                        .label("CPU Usage".to_string())
                        .show_value(true)
                        .chart_style(default_style.clone()),
                ),
        )
        .child(
            Row::new()
                .gap(12.0)
                .child(chart_metric_card(
                    colors,
                    "Sessions",
                    "12.4K",
                    "+8.2%",
                    BarChart::new()
                        .series("Sessions", [4.0, 9.0, 7.0, 13.0, 16.0, 12.0])
                        .chart_style(compact_style.clone())
                        .height(86.0),
                ))
                .child(chart_metric_card(
                    colors,
                    "Bounce Rate",
                    "28.6%",
                    "-3.1%",
                    LineChart::new()
                        .series(
                            "Bounce Rate",
                            [(0.0, 28.0), (1.0, 26.0), (2.0, 30.0), (3.0, 24.0)],
                        )
                        .tone(ChartTone::Success)
                        .show_points(true)
                        .chart_style(compact_style.clone())
                        .height(86.0),
                ))
                .child(chart_metric_card(
                    colors,
                    "CPU",
                    "72%",
                    "High",
                    RadialGauge::new()
                        .value(0.72)
                        .show_value(true)
                        .chart_style(compact_style)
                        .height(86.0),
                )),
        )
        .child(
            Row::new()
                .gap(8.0)
                .child(
                    Container::new()
                        .width(64.0)
                        .height(20.0)
                        .background(colors.accent)
                        .radius(6.0),
                )
                .child(Badge::new("DrawRect bars").tone(BadgeTone::Accent))
                .child(Badge::new("DrawCmd::Polyline line").tone(BadgeTone::Info))
                .child(Badge::new("DrawRingProgress gauge").tone(BadgeTone::Success)),
        )
        .into_view()
}

/// Builds or computes the `chart metric card` deterministic showcase fixture.
fn chart_metric_card(
    colors: ShowcasePalette,
    label: &'static str,
    value: &'static str,
    delta: &'static str,
    chart: impl IntoView<()>,
) -> View<()> {
    Container::new()
        .width(220.0)
        .height(156.0)
        .background(colors.elevated)
        .border(1.0, colors.border)
        .radius(8.0)
        .padding(10.0)
        .child(
            Column::new()
                .gap(4.0)
                .child(text(label, 12, colors.muted))
                .child(text(value, 20, colors.text))
                .child(text(delta, 11, colors.accent))
                .child(chart),
        )
        .into_view()
}

#[allow(clippy::too_many_arguments)]
/// Builds or computes the `table row` deterministic showcase fixture.
fn table_row(
    id: &'static str,
    name: &'static str,
    role: &'static str,
    status: &'static str,
    tone: BadgeTone,
    progress: f32,
    date: &'static str,
    score: &'static str,
    icon: Option<IconId>,
    disabled: bool,
) -> TableRow<&'static str> {
    let mut row = TableRow::new(id)
        .cell(TableCell::text(name))
        .cell(TableCell::muted(role))
        .cell(TableCell::badge(status, tone))
        .cell(TableCell::progress(progress))
        .cell(TableCell::muted(date).align(TableAlign::End))
        .cell(TableCell::text(score).align(TableAlign::End));
    if let Some(icon) = icon {
        row = row.leading_icon(icon);
    }
    if disabled {
        row = row.disabled(true);
    }
    row
}

/// Builds or computes the `showcase table style` deterministic showcase fixture.
fn showcase_table_style(mode: ShowcaseMode) -> TableViewStyle {
    let mut style = TableViewStyle::default();
    if mode == ShowcaseMode::White {
        let colors = mode.palette();
        style.background = colors.surface;
        style.header_background = colors.elevated;
        style.row_alt_background = Color::hex_rgb(0xF1F4F7);
        style.row_active_background = colors.accent.with_alpha(0.10);
        style.row_selected_background = colors.accent.with_alpha(0.16);
        style.grid_color = colors.border;
        style.border = ailloli_ui::core::style::Border::new(1.0, colors.border);
        style.text.color = colors.text;
        style.muted_text.color = colors.muted;
        style.header_text.color = colors.muted;
        style.badge_text.color = colors.text;
        style.progress_track = Color::hex_rgb(0xE4E8ED);
        style.shadows = Vec::new();
    }
    style
}

/// Builds or computes the `showcase toast style` deterministic showcase fixture.
fn showcase_toast_style(mode: ShowcaseMode) -> ToastStyle {
    let mut style = ToastStyle::default();
    if mode == ShowcaseMode::White {
        let colors = mode.palette();
        style.background = colors.surface;
        style.border = ailloli_ui::core::style::Border::new(1.0, colors.border);
        style.title_text.color = colors.text;
        style.description_text.color = colors.muted;
        style.close_tint = colors.muted;
        style.neutral = colors.muted;
        style.success = colors.success;
        style.warning = colors.warning;
        style.danger = colors.danger;
        style.info = colors.info;
        style.shadows = vec![BoxShadow::new(0.0, 8.0, 24.0, 0.0, colors.shadow)];
    }
    style
}

/// Builds or computes the `showcase dialog style` deterministic showcase fixture.
fn showcase_dialog_style(mode: ShowcaseMode, tone: DialogTone) -> DialogStyle {
    let mut style = DialogStyle::from_theme(Theme::default(), tone);
    if mode == ShowcaseMode::White {
        let colors = mode.palette();
        style.backdrop = Color::rgba(15, 23, 42, 0.20);
        style.panel_background = colors.surface;
        style.border = ailloli_ui::core::style::Border::new(1.0, colors.border);
        style.title_text.color = colors.text;
        style.body_text.color = colors.muted;
        style.button_text.color = Color::WHITE;
        style.cancel_background = colors.elevated;
        style.cancel_background_pressed = Color::hex_rgb(0xE9EDF2);
        style.primary_background = colors.accent;
        style.danger_background = colors.danger;
        style.button_border = ailloli_ui::core::style::Border::new(1.0, colors.border);
        style.shadows = vec![BoxShadow::new(0.0, 12.0, 30.0, 0.0, colors.shadow)];
    }
    style
}

/// Builds or computes the `showcase command palette style` deterministic showcase fixture.
fn showcase_command_palette_style(mode: ShowcaseMode) -> CommandPaletteStyle {
    let mut style = CommandPaletteStyle::default();
    if mode == ShowcaseMode::White {
        let colors = mode.palette();
        style.backdrop = Color::rgba(15, 23, 42, 0.16);
        style.panel_background = colors.surface;
        style.border = ailloli_ui::core::style::Border::new(1.0, colors.border);
        style.shadows = vec![BoxShadow::new(0.0, 14.0, 34.0, 0.0, colors.shadow)];
        style.title_text.color = colors.text;
        style.subtitle_text.color = colors.muted;
        style.shortcut_text.color = colors.muted;
        style.no_results_text.color = colors.muted;
        style.icon_tint = colors.muted;
        style.input.bg = colors.surface;
        style.input.border = colors.border;
        style.input.border_focused = colors.accent;
        style.input.placeholder = colors.muted;
        style.input.text.color = colors.text;
        style.popup.popup_background = colors.surface;
        style.popup.option_active = colors.accent.with_alpha(0.12);
        style.popup.option_selected = colors.accent.with_alpha(0.10);
        style.popup.popup_border = ailloli_ui::core::style::Border::new(1.0, colors.border);
        style.popup.text.color = colors.text;
        style.popup.disabled_text.color = colors.muted;
        style.popup.icon_tint = colors.muted;
        style.popup.selected_icon_tint = colors.accent;
    }
    style
}

/// Builds or computes the `showcase date picker style` deterministic showcase fixture.
fn showcase_date_picker_style(mode: ShowcaseMode) -> DatePickerStyle {
    let mut style = DatePickerStyle::default();
    if mode == ShowcaseMode::White {
        apply_white_picker_base(&mut style.base, mode.palette());
    }
    style
}

/// Builds or computes the `showcase time picker style` deterministic showcase fixture.
fn showcase_time_picker_style(mode: ShowcaseMode) -> TimePickerStyle {
    let mut style = TimePickerStyle::default();
    if mode == ShowcaseMode::White {
        apply_white_picker_base(&mut style.base, mode.palette());
    }
    style
}

/// Builds or computes the `showcase color picker style` deterministic showcase fixture.
fn showcase_color_picker_style(mode: ShowcaseMode) -> ColorPickerStyle {
    let mut style = ColorPickerStyle::default();
    if mode == ShowcaseMode::White {
        apply_white_picker_base(&mut style.base, mode.palette());
    }
    style
}

/// Builds or computes the `apply white picker base` deterministic showcase fixture.
fn apply_white_picker_base(
    base: &mut ailloli_ui::widgets::controls::pickers::PickerBaseStyle,
    colors: ShowcasePalette,
) {
    base.trigger_background = colors.surface;
    base.trigger_hovered = Color::hex_rgb(0xF1F4F7);
    base.popup_background = colors.surface;
    base.active = colors.accent.with_alpha(0.12);
    base.selected = colors.accent;
    base.disabled_fill = Color::hex_rgb(0xEEF2F6);
    base.border = ailloli_ui::core::style::Border::new(1.0, colors.border);
    base.popup_border = ailloli_ui::core::style::Border::new(1.0, colors.border);
    base.focus_ring = ailloli_ui::core::style::Border::new(2.0, colors.accent);
    base.shadows = vec![BoxShadow::new(0.0, 10.0, 24.0, 0.0, colors.shadow)];
    base.text.color = colors.text;
    base.muted_text.color = colors.muted;
    base.disabled_text.color = colors.muted.with_alpha(0.65);
    base.accent_text.color = colors.accent;
}

/// Builds or computes the `showcase upload style` deterministic showcase fixture.
fn showcase_upload_style(mode: ShowcaseMode) -> UploadDropzoneStyle {
    let mut style = UploadDropzoneStyle::default();
    if mode == ShowcaseMode::White {
        let colors = mode.palette();
        style.background = colors.surface;
        style.background_hovered = colors.accent.with_alpha(0.10);
        style.border = ailloli_ui::core::style::Border::new(1.0, colors.border);
        style.border_hovered = ailloli_ui::core::style::Border::new(1.0, colors.accent);
        style.focus_ring = ailloli_ui::core::style::Border::new(2.0, colors.accent);
        style.button_background = colors.accent;
        style.title_text.color = colors.text;
        style.description_text.color = colors.muted;
    }
    style
}

/// Builds or computes the `showcase chart style` deterministic showcase fixture.
fn showcase_chart_style(mode: ShowcaseMode, size: ChartSize) -> ChartStyle {
    let mut style = ChartStyle::from_theme(Theme::default(), size);
    if mode == ShowcaseMode::White {
        let colors = mode.palette();
        style.background = colors.surface;
        style.plot_background = Color::hex_rgb(0xF1F4F7);
        style.grid = colors.border.with_alpha(0.58);
        style.axis = colors.border;
        style.border = ailloli_ui::core::style::Border::new(1.0, colors.border);
        style.text.color = colors.text;
        style.muted_text.color = colors.muted;
        style.colors = [
            colors.accent,
            colors.success,
            colors.info,
            colors.warning,
            colors.danger,
            colors.muted,
        ];
    }
    style
}

/// Builds or computes the `drop sample row` deterministic showcase fixture.
fn drop_sample_row(
    colors: ShowcasePalette,
    position: TreeDropPosition,
    label: &'static str,
) -> View<()> {
    let row = Container::new()
        .fill_width()
        .height(30.0)
        .background(if position == TreeDropPosition::Inside {
            colors.accent.with_alpha(0.12)
        } else {
            colors.surface
        })
        .border(
            if position == TreeDropPosition::Inside {
                1.0
            } else {
                0.0
            },
            colors.accent,
        )
        .radius(6.0)
        .padding(6.0)
        .child(text(label, 12, colors.text));
    let line = Container::new()
        .fill_width()
        .height(2.0)
        .background(colors.accent);
    match position {
        TreeDropPosition::Before => Column::new().gap(4.0).child(line).child(row).into_view(),
        TreeDropPosition::After => Column::new().gap(4.0).child(row).child(line).into_view(),
        TreeDropPosition::Inside => row.into_view(),
    }
}

/// Builds or computes the `showcase tree` deterministic showcase fixture.
fn showcase_tree() -> TreeNode<&'static str> {
    TreeNode::branch("root", "Project Root")
        .leading_icon(IconId::History)
        .child(
            TreeNode::branch("src", "src")
                .leading_icon(IconId::Plus)
                .child(
                    TreeNode::branch("components", "components")
                        .leading_icon(IconId::Copy)
                        .child(TreeNode::leaf("button", "button.rs").leading_icon(IconId::Check))
                        .child(TreeNode::leaf("card", "card.rs").leading_icon(IconId::Check)),
                )
                .child(TreeNode::leaf("mod", "mod.rs").leading_icon(IconId::Check)),
        )
        .child(
            TreeNode::branch("views", "views")
                .leading_icon(IconId::Copy)
                .child(TreeNode::leaf("main", "main.rs").leading_icon(IconId::Check)),
        )
        .child(TreeNode::leaf("disabled", "disabled.rs").disabled(true))
        .child(TreeNode::leaf("cargo", "Cargo.toml").leading_icon(IconId::Check))
}

/// Builds or computes the `accordion tree sample panel` deterministic showcase fixture.
fn accordion_tree_sample_panel(
    colors: ShowcasePalette,
    title: &'static str,
    width: f32,
    height: f32,
    content: impl IntoView<()>,
) -> View<()> {
    Container::new()
        .width(width)
        .height(height)
        .background(colors.elevated)
        .border(1.0, colors.border)
        .radius(8.0)
        .padding(10.0)
        .child(
            Column::new()
                .gap(10.0)
                .child(text(title, 12, colors.muted))
                .child(content),
        )
        .into_view()
}

/// Builds or computes the `text inputs section` deterministic showcase fixture.
fn text_inputs_section(mode: ShowcaseMode) -> View<()> {
    let colors = mode.palette();
    let empty = State::new(String::new());
    let value = State::new("Filled input value".to_string());
    let long = State::new(
        "A very long single-line value that should clip and scroll horizontally inside the input"
            .to_string(),
    );
    let small = State::new("Small".to_string());

    Row::new()
        .gap(18.0)
        .child(
            Column::new()
                .gap(10.0)
                .child(
                    TextInput::new()
                        .bind(empty)
                        .placeholder("Placeholder")
                        .width(280.0),
                )
                .child(
                    TextInput::new()
                        .bind(value)
                        .placeholder("Filled")
                        .width(280.0),
                )
                .child(TextInput::new().bind(long).placeholder("Long").width(280.0)),
        )
        .child(
            Column::new()
                .gap(10.0)
                .child(text("Sizes", 12, colors.muted))
                .child(TextInput::new().bind(small).size(12.0).width(180.0))
                .child(
                    TextInput::new()
                        .bind(State::new("Larger text".to_string()))
                        .size(18.0)
                        .width(220.0),
                ),
        )
        .into_view()
}

/// Builds or computes the `layout boxes section` deterministic showcase fixture.
fn layout_boxes_section(mode: ShowcaseMode) -> View<()> {
    let colors = mode.palette();
    let theme = Theme::default();
    Row::new()
        .gap(14.0)
        .child(
            Container::new()
                .width(180.0)
                .height(92.0)
                .background(colors.elevated)
                .border(1.0, colors.border)
                .radius(8.0)
                .padding(12.0)
                .child(text("Surface box", 13, colors.text)),
        )
        .child(
            Container::panel(theme)
                .width(180.0)
                .height(92.0)
                .padding(12.0)
                .child(text("Panel helper", 13, theme.palette().text)),
        )
        .child(
            Container::new()
                .width(180.0)
                .height(92.0)
                .background(colors.surface)
                .border(2.0, colors.accent)
                .radius(14.0)
                .shadow(BoxShadow::glow(colors.accent.with_alpha(0.42)))
                .padding(12.0)
                .child(text("Accent border", 13, colors.text)),
        )
        .child(
            Container::new()
                .width(250.0)
                .height(92.0)
                .background(colors.elevated)
                .border(1.0, colors.border)
                .radius(8.0)
                .clip_children(true)
                .child(
                    ScrollView::vertical().child(
                        Column::new()
                            .gap(4.0)
                            .padding(10.0)
                            .child(text("Clipped ScrollView", 13, colors.text))
                            .child(text("row 01", 12, colors.muted))
                            .child(text("row 02", 12, colors.muted))
                            .child(text("row 03", 12, colors.muted))
                            .child(text("row 04", 12, colors.muted))
                            .child(text("row 05", 12, colors.muted)),
                    ),
                ),
        )
        .into_view()
}

/// Builds or computes the `typography section` deterministic showcase fixture.
fn typography_section(mode: ShowcaseMode) -> View<()> {
    let colors = mode.palette();
    Column::new()
        .gap(8.0)
        .child(text("Display text for section titles", 20, colors.text))
        .child(text(
            "Body text uses the UI font and should remain readable in both showcases.",
            14,
            colors.text,
        ))
        .child(text(
            "Muted supporting copy for dense panels.",
            13,
            colors.muted,
        ))
        .child(mono(
            "fn main() { println!(\"Ailloli UI\"); }",
            13,
            colors.info,
        ))
        .into_view()
}

/// Builds or computes the `icons section` deterministic showcase fixture.
fn icons_section(mode: ShowcaseMode) -> View<()> {
    let colors = mode.palette();
    Row::new()
        .gap(18.0)
        .child(icon_tile("Plus", IconId::Plus, colors))
        .child(icon_tile("Check", IconId::Check, colors))
        .child(icon_tile("Copy", IconId::Copy, colors))
        .child(icon_tile("Trash", IconId::Trash, colors))
        .child(icon_tile("History", IconId::History, colors))
        .into_view()
}

/// Builds or computes the `icon tile` deterministic showcase fixture.
fn icon_tile(label: &'static str, icon: IconId, colors: ShowcasePalette) -> View<()> {
    Container::new()
        .width(104.0)
        .height(74.0)
        .background(colors.elevated)
        .border(1.0, colors.border)
        .radius(8.0)
        .padding(10.0)
        .child(
            Column::new()
                .gap(8.0)
                .child(Icon::new(icon).size(22.0).tint(colors.accent))
                .child(text(label, 12, colors.text)),
        )
        .into_view()
}

/// Builds or computes the `editor section` deterministic showcase fixture.
fn editor_section(mode: ShowcaseMode) -> View<()> {
    let colors = mode.palette();
    let code = [
        "fn main() {",
        "    let app = ailloli_ui::App::new();",
        "    app.window(Window::new(\"main\"));",
        "}",
    ]
    .join("\n");
    let buffer = State::new(TextBuffer::from_string(code));

    Container::new()
        .width(760.0)
        .height(70.0)
        .background(colors.elevated)
        .border(1.0, colors.border)
        .radius(8.0)
        .clip_children(true)
        .child(Editor::new(buffer).width(760.0).height(70.0))
        .into_view()
}

/// Builds or computes the `code editor section` deterministic showcase fixture.
fn code_editor_section(mode: ShowcaseMode) -> View<()> {
    let colors = mode.palette();
    let code = [
        "use ailloli_ui::prelude::*;",
        "",
        "fn main() -> ailloli_ui::Result<()> {",
        "    let document = State::new(Document::new(DocumentId(1), TextBuffer::from_string(\"hello\")));",
        "    App::new().window(Window::new(\"code\").content(|| CodeEditor::new(document.clone()))).run()",
        "}",
    ]
    .join("\n");
    let document = State::new(
        Document::new(DocumentId(54), TextBuffer::from_string(code))
            .with_language(EditorLanguage::Rust),
    );

    Container::new()
        .width(820.0)
        .height(190.0)
        .background(colors.elevated)
        .border(1.0, colors.border)
        .radius(8.0)
        .clip_children(true)
        .child(
            CodeEditor::new(document)
                .language(EditorLanguage::Rust)
                .line_numbers(true)
                .width(820.0)
                .height(190.0),
        )
        .into_view()
}

#[cfg(test)]
/// Builds or computes the `code editor phase54 3 section` deterministic showcase fixture.
fn code_editor_phase54_3_section(mode: ShowcaseMode) -> View<()> {
    let colors = mode.palette();
    let code = phase54_3_code_editor_fixture();
    let document = State::new(
        Document::new(DocumentId(543), TextBuffer::from_string(code))
            .with_language(EditorLanguage::Rust)
            .with_path("src/phase54_3.rs"),
    );

    Container::new()
        .width(860.0)
        .height(230.0)
        .background(colors.elevated)
        .border(1.0, colors.border)
        .radius(8.0)
        .clip_children(true)
        .child(
            CodeEditor::new(document)
                .language(EditorLanguage::Rust)
                .line_numbers(true)
                .initial_scroll(210.0, 320.0)
                .width(860.0)
                .height(230.0),
        )
        .key("code-editor-phase54-3-widget")
        .into_view()
}

#[cfg(test)]
/// Builds or computes the `code editor phase54 3 baseline section` deterministic showcase fixture.
fn code_editor_phase54_3_baseline_section(mode: ShowcaseMode) -> View<()> {
    let colors = mode.palette();
    let code = [
        "fn main() {",
        "    const test: &str = \"test\";",
        "}",
        "",
        "pub fn next_line() -> usize {",
        "    let answer = 42;",
        "    answer",
        "}",
    ]
    .join("\n");
    let document = State::new(
        Document::new(DocumentId(544), TextBuffer::from_string(code))
            .with_language(EditorLanguage::Rust)
            .with_path("src/baseline.rs"),
    );

    Container::new()
        .width(860.0)
        .height(190.0)
        .background(colors.elevated)
        .border(1.0, colors.border)
        .radius(8.0)
        .clip_children(true)
        .child(
            CodeEditor::new(document)
                .language(EditorLanguage::Rust)
                .line_numbers(true)
                .width(860.0)
                .height(190.0),
        )
        .key("code-editor-phase54-3-baseline-widget")
        .into_view()
}

#[cfg(test)]
/// Builds or computes the `code editor phase54 3 active line section` deterministic showcase fixture.
fn code_editor_phase54_3_active_line_section(mode: ShowcaseMode) -> View<()> {
    let colors = mode.palette();
    let code = [
        "const test: &str = \"test\";",
        "fn main() {",
        "    let value = 42;",
        "    println!(\"{}\", test);",
        "}",
    ]
    .join("\n");
    let document = State::new(
        Document::new(DocumentId(545), TextBuffer::from_string(code))
            .with_language(EditorLanguage::Rust)
            .with_path("src/active_line.rs"),
    );

    Container::new()
        .width(860.0)
        .height(170.0)
        .background(colors.elevated)
        .border(1.0, colors.border)
        .radius(8.0)
        .clip_children(true)
        .child(
            CodeEditor::new(document)
                .language(EditorLanguage::Rust)
                .line_numbers(true)
                .width(860.0)
                .height(170.0),
        )
        .key("code-editor-phase54-3-active-line-widget")
        .into_view()
}

#[cfg(test)]
/// Builds or computes the `code editor phase54 3 tree sitter section` deterministic showcase fixture.
fn code_editor_phase54_3_tree_sitter_section(mode: ShowcaseMode) -> View<()> {
    let colors = mode.palette();
    let code = [
        "#[derive(Debug)]",
        "pub struct Parser<'a> {",
        "    source: &'a str,",
        "}",
        "",
        "impl<'a> Parser<'a> {",
        "    pub fn parse(&self) -> Result<(), Error> {",
        "        let value = 42;",
        "        println!(\"value = {}\", value);",
        "        let raw = r#\"raw string\"#;",
        "        let ch = 'x';",
        "        // tree-sitter primary, lexical gap-fill",
        "        Ok(())",
        "    }",
        "}",
    ]
    .join("\n");
    let document = State::new(
        Document::new(DocumentId(546), TextBuffer::from_string(code))
            .with_language(EditorLanguage::Rust)
            .with_path("src/tree_sitter_tokens.rs"),
    );

    Container::new()
        .width(860.0)
        .height(270.0)
        .background(colors.elevated)
        .border(1.0, colors.border)
        .radius(8.0)
        .clip_children(true)
        .child(
            CodeEditor::new(document)
                .language(EditorLanguage::Rust)
                .line_numbers(true)
                .width(860.0)
                .height(270.0),
        )
        .key("code-editor-phase54-3-tree-sitter-widget")
        .into_view()
}

#[cfg(test)]
/// Builds or computes the `code editor phase54 3 extension detection section` deterministic showcase fixture.
fn code_editor_phase54_3_extension_detection_section(mode: ShowcaseMode) -> View<()> {
    let colors = mode.palette();
    let code = [
        "pub struct ExtensionDetected {",
        "    value: usize,",
        "}",
        "",
        "pub fn extension_detected() -> usize {",
        "    // .rs path should enable Rust syntax without builder override",
        "    let value = 42;",
        "    println!(\"extension detected: {}\", value);",
        "    value",
        "}",
    ]
    .join("\n");
    let document = State::new(
        Document::new(DocumentId(547), TextBuffer::from_string(code))
            .with_path("src/extension_detected.rs"),
    );

    Container::new()
        .width(860.0)
        .height(220.0)
        .background(colors.elevated)
        .border(1.0, colors.border)
        .radius(8.0)
        .clip_children(true)
        .child(
            CodeEditor::new(document)
                .line_numbers(true)
                .width(860.0)
                .height(220.0),
        )
        .key("code-editor-phase54-3-extension-detection-widget")
        .into_view()
}

#[cfg(test)]
/// Builds or computes the `code editor phase54 3 symbol outline section` deterministic showcase fixture.
fn code_editor_phase54_3_symbol_outline_section(mode: ShowcaseMode) -> View<()> {
    let colors = mode.palette();
    let code = phase54_3_symbol_outline_fixture();
    let document = Document::new(DocumentId(548), TextBuffer::from_string(code.clone()))
        .with_language(EditorLanguage::Rust)
        .with_path("src/symbol_outline.rs");
    let mut indexer = TreeSitterRustSymbolIndexer;
    let summary = indexer.index_document(&document);
    let document = State::new(document);

    Row::new()
        .gap(12.0)
        .child(
            Container::new()
                .width(650.0)
                .height(330.0)
                .background(colors.elevated)
                .border(1.0, colors.border)
                .radius(8.0)
                .clip_children(true)
                .child(
                    CodeEditor::new(document)
                        .language(EditorLanguage::Rust)
                        .line_numbers(true)
                        .width(650.0)
                        .height(330.0),
                ),
        )
        .child(symbol_outline_panel(mode, &summary))
        .key("code-editor-phase54-3-symbol-outline-widget")
        .into_view()
}

#[cfg(test)]
/// Builds or computes the `code editor phase54 3 ctags fallback section` deterministic showcase fixture.
fn code_editor_phase54_3_ctags_fallback_section(mode: ShowcaseMode) -> View<()> {
    let colors = mode.palette();
    let code = [
        "struct App {",
        "    value: usize,",
        "}",
        "",
        "impl App {",
        "    fn run(&self) -> usize { self.value }",
        "}",
        "",
        "type Output = usize;",
        "macro_rules! trace_value { () => {} }",
    ]
    .join("\n");
    let document = Document::new(DocumentId(549), TextBuffer::from_string(code))
        .with_language(EditorLanguage::Rust)
        .with_path("src/ctags_fallback.rs");
    let fixture = r#"{"_type":"tag","name":"App","path":"src/ctags_fallback.rs","kind":"struct","line":1,"end":3,"signature":"struct App"}
{"_type":"tag","name":"value","path":"src/ctags_fallback.rs","kind":"field","line":2,"end":2,"scope":"App","scopeKind":"struct","typeref":"usize"}
{"_type":"tag","name":"run","path":"src/ctags_fallback.rs","kind":"method","line":6,"end":6,"scope":"App","scopeKind":"struct","signature":"(&self) -> usize"}
{"_type":"tag","name":"Output","path":"src/ctags_fallback.rs","kind":"type","line":9,"end":9,"typeref":"usize"}
{"_type":"tag","name":"trace_value","path":"src/ctags_fallback.rs","kind":"macro","line":10,"end":10}"#;
    let mut indexer = CtagsSymbolIndexer::from_json_lines(fixture);
    let summary = indexer.index_document(&document);
    let document = State::new(document);

    Row::new()
        .gap(12.0)
        .child(
            Container::new()
                .width(650.0)
                .height(260.0)
                .background(colors.elevated)
                .border(1.0, colors.border)
                .radius(8.0)
                .clip_children(true)
                .child(
                    CodeEditor::new(document)
                        .language(EditorLanguage::Rust)
                        .line_numbers(true)
                        .width(650.0)
                        .height(260.0),
                ),
        )
        .child(ctags_fallback_panel(mode, &summary))
        .key("code-editor-phase54-3-ctags-fallback-widget")
        .into_view()
}

#[cfg(test)]
/// Builds or computes the `code editor phase54 3 symbol graph section` deterministic showcase fixture.
fn code_editor_phase54_3_symbol_graph_section(mode: ShowcaseMode) -> View<()> {
    let colors = mode.palette();
    let code = phase54_3_symbol_graph_fixture();
    let document = Document::new(DocumentId(550), TextBuffer::from_string(code.clone()))
        .with_language(EditorLanguage::Rust)
        .with_path("src/symbol_graph.rs");
    let mut indexer = TreeSitterRustSymbolIndexer;
    let summary = indexer.index_document(&document);
    let document = State::new(document);

    Row::new()
        .gap(12.0)
        .child(
            Container::new()
                .width(650.0)
                .height(330.0)
                .background(colors.elevated)
                .border(1.0, colors.border)
                .radius(8.0)
                .clip_children(true)
                .child(
                    CodeEditor::new(document)
                        .language(EditorLanguage::Rust)
                        .line_numbers(true)
                        .width(650.0)
                        .height(330.0),
                ),
        )
        .child(symbol_graph_panel(mode, &summary))
        .key("code-editor-phase54-3-symbol-graph-widget")
        .into_view()
}

#[cfg(test)]
/// Builds or computes the `code editor phase54 3 search section` deterministic showcase fixture.
fn code_editor_phase54_3_search_section(mode: ShowcaseMode) -> View<()> {
    let colors = mode.palette();
    let code = phase54_3_search_fixture();
    let document = State::new(
        Document::new(DocumentId(551), TextBuffer::from_string(code))
            .with_language(EditorLanguage::Rust)
            .with_path("src/search.rs"),
    );

    Row::new()
        .gap(12.0)
        .child(
            Container::new()
                .width(760.0)
                .height(330.0)
                .background(colors.elevated)
                .border(1.0, colors.border)
                .radius(8.0)
                .clip_children(true)
                .child(
                    CodeEditor::new(document)
                        .language(EditorLanguage::Rust)
                        .line_numbers(true)
                        .initial_scroll(0.0, 34.0)
                        .search_query(SearchQuery::new("value").whole_word(true))
                        .search_active_match(2)
                        .width(760.0)
                        .height(330.0),
                ),
        )
        .child(search_panel(mode))
        .key("code-editor-phase54-3-search-widget")
        .into_view()
}

#[cfg(test)]
/// Builds or computes the `code editor phase54 3 multiclick selection section` deterministic showcase fixture.
fn code_editor_phase54_3_multiclick_selection_section(mode: ShowcaseMode) -> View<()> {
    let colors = mode.palette();
    let code = phase54_3_multiclick_selection_fixture();
    let word_start = code.find("foo_bar").expect("foo_bar fixture");
    let line_start = code.find("let r#async").expect("line fixture");
    let line_end = code[line_start..]
        .find('\n')
        .map(|idx| line_start + idx)
        .unwrap_or(code.len());
    let word_document = State::new(
        Document::new(DocumentId(559), TextBuffer::from_string(code.clone()))
            .with_language(EditorLanguage::Rust)
            .with_path("src/multiclick_word.rs"),
    );
    let line_document = State::new(
        Document::new(DocumentId(560), TextBuffer::from_string(code))
            .with_language(EditorLanguage::Rust)
            .with_path("src/multiclick_line.rs"),
    );

    Row::new()
        .gap(12.0)
        .child(
            Container::new()
                .width(510.0)
                .height(270.0)
                .background(colors.elevated)
                .border(1.0, colors.border)
                .radius(8.0)
                .clip_children(true)
                .child(
                    CodeEditor::new(word_document)
                        .language(EditorLanguage::Rust)
                        .line_numbers(true)
                        .initial_selection(word_start, word_start + "foo_bar".len())
                        .width(510.0)
                        .height(270.0),
                ),
        )
        .child(
            Container::new()
                .width(510.0)
                .height(270.0)
                .background(colors.elevated)
                .border(1.0, colors.border)
                .radius(8.0)
                .clip_children(true)
                .child(
                    CodeEditor::new(line_document)
                        .language(EditorLanguage::Rust)
                        .line_numbers(true)
                        .initial_selection(line_start, line_end)
                        .width(510.0)
                        .height(270.0),
                ),
        )
        .key("code-editor-phase54-3-multiclick-selection-widget")
        .into_view()
}

#[cfg(test)]
/// Builds or computes the `code editor phase54 3 diagnostics section` deterministic showcase fixture.
fn code_editor_phase54_3_diagnostics_section(mode: ShowcaseMode) -> View<()> {
    let colors = mode.palette();
    let code = phase54_3_diagnostics_fixture();
    let diagnostics = phase54_3_diagnostics_for_fixture(&code);
    let document = State::new(
        Document::new(DocumentId(552), TextBuffer::from_string(code))
            .with_language(EditorLanguage::Rust)
            .with_path("src/diagnostics.rs"),
    );

    Row::new()
        .gap(12.0)
        .child(
            Container::new()
                .width(760.0)
                .height(330.0)
                .background(colors.elevated)
                .border(1.0, colors.border)
                .radius(8.0)
                .clip_children(true)
                .child(
                    CodeEditor::new(document)
                        .language(EditorLanguage::Rust)
                        .line_numbers(true)
                        .diagnostics(diagnostics)
                        .active_diagnostic(0)
                        .width(760.0)
                        .height(330.0),
                ),
        )
        .child(diagnostics_panel(mode))
        .key("code-editor-phase54-3-diagnostics-widget")
        .into_view()
}

#[cfg(test)]
/// Builds or computes the `code editor phase54 3 folding section` deterministic showcase fixture.
fn code_editor_phase54_3_folding_section(mode: ShowcaseMode) -> View<()> {
    let colors = mode.palette();
    let code = phase54_3_folding_fixture();
    let document = State::new(
        Document::new(DocumentId(553), TextBuffer::from_string(code))
            .with_language(EditorLanguage::Rust)
            .with_path("src/folding.rs"),
    );

    Row::new()
        .gap(12.0)
        .child(
            Container::new()
                .width(760.0)
                .height(330.0)
                .background(colors.elevated)
                .border(1.0, colors.border)
                .radius(8.0)
                .clip_children(true)
                .child(
                    CodeEditor::new(document)
                        .language(EditorLanguage::Rust)
                        .line_numbers(true)
                        .fold_regions(vec![
                            FoldRegion::new(1, 5),
                            FoldRegion::new(8, 10).collapsed(true),
                        ])
                        .width(760.0)
                        .height(330.0),
                ),
        )
        .child(folding_panel(mode))
        .key("code-editor-phase54-3-folding-widget")
        .into_view()
}

#[cfg(test)]
/// Builds or computes the `code editor phase54 3 ide folding gutter section` deterministic showcase fixture.
fn code_editor_phase54_3_ide_folding_gutter_section(mode: ShowcaseMode) -> View<()> {
    let colors = mode.palette();
    let code = phase54_3_ide_folding_gutter_fixture();
    let document = State::new(
        Document::new(DocumentId(563), TextBuffer::from_string(code))
            .with_language(EditorLanguage::Rust)
            .with_path("src/ide_folding_gutter.rs"),
    );

    Row::new()
        .gap(12.0)
        .child(
            Container::new()
                .width(430.0)
                .height(430.0)
                .background(colors.elevated)
                .border(1.0, colors.border)
                .radius(8.0)
                .clip_children(true)
                .child(
                    CodeEditor::new(document)
                        .language(EditorLanguage::Rust)
                        .line_numbers(true)
                        .fold_regions(phase54_3_ide_folding_regions())
                        .initial_scroll(0.0, 29_295.0)
                        .width(430.0)
                        .height(430.0),
                ),
        )
        .child(ide_folding_gutter_panel(mode))
        .key("code-editor-phase54-3-ide-folding-gutter-widget")
        .into_view()
}

#[cfg(test)]
/// Builds or computes the `code editor phase54 3 lsp section` deterministic showcase fixture.
fn code_editor_phase54_3_lsp_section(mode: ShowcaseMode) -> View<()> {
    let colors = mode.palette();
    let code = phase54_3_lsp_fixture();
    let document_data = Document::new(DocumentId(554), TextBuffer::from_string(code.clone()))
        .with_language(EditorLanguage::Rust)
        .with_path("src/lsp_enrichment.rs");
    let enrichment = phase54_3_lsp_mock_enrichment(&document_data, &code);
    let document = State::new(document_data);

    Row::new()
        .gap(12.0)
        .child(
            Container::new()
                .width(760.0)
                .height(330.0)
                .background(colors.elevated)
                .border(1.0, colors.border)
                .radius(8.0)
                .clip_children(true)
                .child(
                    CodeEditor::new(document)
                        .language(EditorLanguage::Rust)
                        .line_numbers(true)
                        .diagnostics(enrichment.diagnostics.clone())
                        .active_diagnostic(0)
                        .width(760.0)
                        .height(330.0),
                ),
        )
        .child(lsp_enrichment_panel(mode, &enrichment))
        .key("code-editor-phase54-3-lsp-widget")
        .into_view()
}

#[cfg(test)]
/// Builds or computes the `code editor phase54 3 scip section` deterministic showcase fixture.
fn code_editor_phase54_3_scip_section(mode: ShowcaseMode) -> View<()> {
    let colors = mode.palette();
    let code = phase54_3_scip_code_fixture();
    let document = State::new(
        Document::new(DocumentId(555), TextBuffer::from_string(code))
            .with_language(EditorLanguage::Rust)
            .with_path("src/lib.rs"),
    );
    let project = phase54_3_scip_project_summary();

    Row::new()
        .gap(12.0)
        .child(
            Container::new()
                .width(760.0)
                .height(330.0)
                .background(colors.elevated)
                .border(1.0, colors.border)
                .radius(8.0)
                .clip_children(true)
                .child(
                    CodeEditor::new(document)
                        .language(EditorLanguage::Rust)
                        .line_numbers(true)
                        .width(760.0)
                        .height(330.0),
                ),
        )
        .child(scip_project_panel(mode, &project))
        .key("code-editor-phase54-3-scip-widget")
        .into_view()
}

#[cfg(test)]
/// Builds or computes the `code editor phase54 3 large file section` deterministic showcase fixture.
fn code_editor_phase54_3_large_file_section(mode: ShowcaseMode) -> View<()> {
    let colors = mode.palette();
    let code = phase54_3_large_file_fixture();
    let document = State::new(
        Document::new(DocumentId(556), TextBuffer::from_string(code))
            .with_language(EditorLanguage::Rust)
            .with_path("src/large_file.rs"),
    );

    Row::new()
        .gap(12.0)
        .child(
            Container::new()
                .width(820.0)
                .height(360.0)
                .background(colors.elevated)
                .border(1.0, colors.border)
                .radius(8.0)
                .clip_children(true)
                .child(
                    CodeEditor::new(document)
                        .language(EditorLanguage::Rust)
                        .line_numbers(true)
                        .initial_scroll(0.0, 8_980.0)
                        .width(820.0)
                        .height(360.0),
                ),
        )
        .child(large_file_metrics_panel(mode))
        .key("code-editor-phase54-3-large-file-widget")
        .into_view()
}

#[cfg(test)]
/// Builds or computes the `code editor phase54 3 theme variants section` deterministic showcase fixture.
fn code_editor_phase54_3_theme_variants_section(mode: ShowcaseMode) -> View<()> {
    let colors = mode.palette();
    let white_code = phase54_3_theme_variants_fixture("white");
    let right = State::new(
        Document::new(DocumentId(558), TextBuffer::from_string(white_code))
            .with_language(EditorLanguage::Rust)
            .with_path("src/theme_white.rs"),
    );

    Row::new()
        .gap(12.0)
        .child(code_theme_static_preview(
            code_theme_dark_variant(),
            colors.border,
            "default",
        ))
        .child(
            Container::new()
                .width(590.0)
                .height(330.0)
                .background(Color::rgb(255, 255, 255))
                .border(1.0, Color::rgba(15, 23, 42, 0.22))
                .radius(8.0)
                .clip_children(true)
                .child(
                    CodeEditor::new(right)
                        .theme(code_theme_white_variant())
                        .language(EditorLanguage::Rust)
                        .line_numbers(true)
                        .search_query(SearchQuery::new("value").whole_word(true))
                        .search_active_match(0)
                        .width(590.0)
                        .height(330.0)
                        .into_view()
                        .key("code-editor-phase54-3-theme-white-editor"),
                ),
        )
        .key("code-editor-phase54-3-theme-variants-widget")
        .into_view()
}

#[cfg(test)]
/// Builds or computes the `code theme static preview` deterministic showcase fixture.
fn code_theme_static_preview(theme: CodeTheme, border: Color, label: &'static str) -> View<()> {
    Container::new()
        .width(590.0)
        .height(330.0)
        .background(theme.background)
        .border(1.0, border)
        .radius(8.0)
        .clip_children(true)
        .child(
            Column::new()
                .gap(5.0)
                .padding(10.0)
                .child(mono(
                    "1  pub fn themed_editor(value: i32) -> i32 {",
                    12,
                    theme.syntax_keyword,
                ))
                .child(mono(
                    "2      let computed_value = value + 42;",
                    12,
                    theme.syntax_number,
                ))
                .child(mono(
                    "3      let label = \"value\";",
                    12,
                    theme.syntax_string,
                ))
                .child(mono(
                    format!("4      // {label} theme value should remain readable"),
                    12,
                    theme.syntax_comment,
                ))
                .child(mono("5      computed_value", 12, theme.syntax_identifier))
                .child(mono("6  }", 12, theme.syntax_punctuation)),
        )
        .key("code-editor-phase54-3-theme-dark-preview")
        .into_view()
}

#[cfg(test)]
/// Builds or computes the `folding panel` deterministic showcase fixture.
fn folding_panel(mode: ShowcaseMode) -> View<()> {
    let colors = mode.palette();
    Container::new()
        .width(430.0)
        .height(260.0)
        .background(colors.surface)
        .border(1.0, colors.border)
        .radius(8.0)
        .padding(10.0)
        .child(
            Column::new()
                .gap(8.0)
                .child(mono("Folding", 14, colors.text))
                .child(mono("line 2: open fn body", 11, colors.muted))
                .child(mono("line 9: collapsed visible block", 11, colors.muted))
                .child(mono(
                    "gutter markers stay outside text_rect",
                    11,
                    colors.muted,
                ))
                .child(mono(
                    "placeholder keeps hidden-line count visible",
                    11,
                    colors.info,
                )),
        )
        .into_view()
}

#[cfg(test)]
/// Builds or computes the `ide folding gutter panel` deterministic showcase fixture.
fn ide_folding_gutter_panel(mode: ShowcaseMode) -> View<()> {
    let colors = mode.palette();
    Container::new()
        .width(520.0)
        .height(300.0)
        .background(colors.surface)
        .border(1.0, colors.border)
        .radius(8.0)
        .padding(10.0)
        .child(
            Column::new()
                .gap(8.0)
                .child(mono("IDE folding gutter", 14, colors.text))
                .child(mono("line numbers around 1627-1645", 11, colors.muted))
                .child(mono(
                    "orange chevrons align to fold starts",
                    11,
                    colors.warning,
                ))
                .child(mono("thin guides stay inside the gutter", 11, colors.muted))
                .child(mono(
                    "line number reserve prevents chevron overlap",
                    11,
                    colors.info,
                )),
        )
        .into_view()
}

#[cfg(test)]
/// Builds or computes the `lsp enrichment panel` deterministic showcase fixture.
fn lsp_enrichment_panel(
    mode: ShowcaseMode,
    enrichment: &ailloli_ui_editor::code::LspEnrichment,
) -> View<()> {
    let colors = mode.palette();
    let mut symbols = Column::new().gap(5.0);
    for symbol in enrichment.symbols.iter().take(5) {
        symbols = symbols.child(mono(
            format!("Lsp {:?} {}", symbol.kind, symbol.name),
            11,
            colors.text,
        ));
    }
    let mut diagnostics = Column::new().gap(5.0);
    for diagnostic in enrichment.diagnostics.iter().take(5) {
        let color = match diagnostic.severity {
            DiagnosticSeverity::Error => colors.danger,
            DiagnosticSeverity::Warning => colors.warning,
            DiagnosticSeverity::Info => colors.accent,
            DiagnosticSeverity::Hint => colors.muted,
        };
        diagnostics = diagnostics.child(mono(
            format!("Lsp {:?}: {}", diagnostic.severity, diagnostic.message),
            11,
            color,
        ));
    }

    Container::new()
        .width(430.0)
        .height(300.0)
        .background(colors.surface)
        .border(1.0, colors.border)
        .radius(8.0)
        .padding(10.0)
        .clip_children(true)
        .child(
            Column::new()
                .gap(8.0)
                .child(mono("source = Lsp", 14, colors.accent))
                .child(mono(
                    "capabilities: symbols refs diagnostics",
                    11,
                    colors.muted,
                ))
                .child(symbols)
                .child(diagnostics)
                .child(mono(
                    "stale diagnostics ignored by version",
                    11,
                    colors.info,
                )),
        )
        .into_view()
}

#[cfg(test)]
/// Builds or computes the `scip project panel` deterministic showcase fixture.
fn scip_project_panel(
    mode: ShowcaseMode,
    project: &ailloli_ui_editor::code::ScipProjectSummary,
) -> View<()> {
    let colors = mode.palette();
    let mut rows = Column::new().gap(5.0);
    for document in &project.documents {
        let path = document
            .path
            .as_ref()
            .map(|path| path.display().to_string())
            .unwrap_or_else(|| "<memory>".into());
        rows = rows.child(mono(format!("Scip file {path}"), 11, colors.muted));
        for symbol in document.symbols.iter().take(4) {
            rows = rows.child(mono(
                format!(
                    "  {:?} {} source={:?}",
                    symbol.kind, symbol.name, symbol.source
                ),
                11,
                colors.text,
            ));
        }
    }
    for link in project.navigation.iter().take(4) {
        rows = rows.child(mono(
            format!("ref {} -> {}", link.from_path, link.to_path),
            11,
            colors.accent,
        ));
    }

    Container::new()
        .width(430.0)
        .height(310.0)
        .background(colors.surface)
        .border(1.0, colors.border)
        .radius(8.0)
        .padding(10.0)
        .clip_children(true)
        .child(
            Column::new()
                .gap(8.0)
                .child(mono("source = Scip", 14, colors.accent))
                .child(mono("project index, cross-file refs", 11, colors.muted))
                .child(rows),
        )
        .into_view()
}

#[cfg(test)]
/// Builds or computes the `large file metrics panel` deterministic showcase fixture.
fn large_file_metrics_panel(mode: ShowcaseMode) -> View<()> {
    let colors = mode.palette();
    Container::new()
        .width(380.0)
        .height(250.0)
        .background(colors.surface)
        .border(1.0, colors.border)
        .radius(8.0)
        .padding(10.0)
        .child(
            Column::new()
                .gap(8.0)
                .child(mono("Debug metrics", 14, colors.accent))
                .child(mono("fast path = NoWrap", 11, colors.text))
                .child(mono("paragraphs = 10k+", 11, colors.muted))
                .child(mono("scroll_y -> Fenwick/O(log n)", 11, colors.muted))
                .child(mono(
                    "content width includes offscreen longest line",
                    11,
                    colors.info,
                ))
                .child(mono(
                    "cache hits/misses tracked per frame",
                    11,
                    colors.warning,
                )),
        )
        .into_view()
}

#[cfg(test)]
/// Builds or computes the `ctags fallback panel` deterministic showcase fixture.
fn ctags_fallback_panel(
    mode: ShowcaseMode,
    summary: &ailloli_ui_editor::code::CodeFileSummary,
) -> View<()> {
    let colors = mode.palette();
    let mut list = Column::new().gap(5.0);
    for symbol in &summary.symbols {
        let depth = symbol_depth(summary, symbol.id).min(3);
        let label = format!(
            "{}Ctags {:?} {}",
            "  ".repeat(depth),
            symbol.kind,
            symbol.name
        );
        let detail = symbol.signature.as_deref().unwrap_or("source = Ctags");
        list = list.child(
            Column::new()
                .gap(1.0)
                .child(mono(label, 11, colors.text))
                .child(mono(detail, 10, colors.muted)),
        );
    }

    Container::new()
        .width(430.0)
        .height(260.0)
        .background(colors.surface)
        .border(1.0, colors.border)
        .radius(8.0)
        .padding(10.0)
        .clip_children(true)
        .child(
            Column::new()
                .gap(8.0)
                .child(text("Universal Ctags Fallback", 13, colors.text))
                .child(mono("source = Ctags", 11, colors.accent))
                .child(list),
        )
        .into_view()
}

#[cfg(test)]
/// Builds or computes the `search panel` deterministic showcase fixture.
fn search_panel(mode: ShowcaseMode) -> View<()> {
    let colors = mode.palette();
    Container::new()
        .width(320.0)
        .height(330.0)
        .background(colors.surface)
        .border(1.0, colors.border)
        .radius(8.0)
        .padding(10.0)
        .clip_children(true)
        .child(
            Column::new()
                .gap(8.0)
                .child(text("Search State", 13, colors.text))
                .child(mono("query = value", 11, colors.text))
                .child(mono("mode = whole word", 11, colors.muted))
                .child(mono("active match = 3 / many", 11, colors.accent))
                .child(mono("scope = current document", 11, colors.muted)),
        )
        .into_view()
}

#[cfg(test)]
/// Builds or computes the `diagnostics panel` deterministic showcase fixture.
fn diagnostics_panel(mode: ShowcaseMode) -> View<()> {
    let colors = mode.palette();
    Container::new()
        .width(320.0)
        .height(330.0)
        .background(colors.surface)
        .border(1.0, colors.border)
        .radius(8.0)
        .padding(10.0)
        .clip_children(true)
        .child(
            Column::new()
                .gap(8.0)
                .child(text("Diagnostics", 13, colors.text))
                .child(mono("error: unused value", 11, colors.danger))
                .child(mono("warning: shadowed binding", 11, colors.warning))
                .child(mono("info: inferred type", 11, colors.accent))
                .child(mono("hint: simplify expression", 11, colors.muted)),
        )
        .into_view()
}

#[cfg(test)]
/// Builds or computes the `symbol graph panel` deterministic showcase fixture.
fn symbol_graph_panel(
    mode: ShowcaseMode,
    summary: &ailloli_ui_editor::code::CodeFileSummary,
) -> View<()> {
    let colors = mode.palette();
    let mut list = Column::new().gap(5.0);
    for edge in summary.edges.iter().take(24) {
        let from = symbol_label(summary, edge.from);
        let to = symbol_label(summary, edge.to);
        let color = match edge.kind {
            SymbolEdgeKind::Calls => colors.accent,
            SymbolEdgeKind::Imports => colors.warning,
            _ => colors.text,
        };
        list = list.child(mono(
            format!("{:?}: {} -> {}", edge.kind, from, to),
            11,
            color,
        ));
    }

    Container::new()
        .width(430.0)
        .height(330.0)
        .background(colors.surface)
        .border(1.0, colors.border)
        .radius(8.0)
        .padding(10.0)
        .clip_children(true)
        .child(
            Column::new()
                .gap(8.0)
                .child(text("Octav Symbol Graph", 13, colors.text))
                .child(mono("Contains / Imports / Calls", 11, colors.muted))
                .child(list),
        )
        .into_view()
}

#[cfg(test)]
/// Builds or computes the `symbol outline panel` deterministic showcase fixture.
fn symbol_outline_panel(
    mode: ShowcaseMode,
    summary: &ailloli_ui_editor::code::CodeFileSummary,
) -> View<()> {
    let colors = mode.palette();
    let mut list = Column::new().gap(4.0);
    for symbol in summary.symbols.iter().take(22) {
        let depth = symbol_depth(summary, symbol.id).min(4);
        let label = format!("{}{:?} {}", "  ".repeat(depth), symbol.kind, symbol.name);
        let detail = symbol.signature.as_deref().unwrap_or("");
        list = list.child(
            Column::new()
                .gap(1.0)
                .child(mono(label, 11, colors.text))
                .child(mono(detail, 10, colors.muted)),
        );
    }

    Container::new()
        .width(430.0)
        .height(330.0)
        .background(colors.surface)
        .border(1.0, colors.border)
        .radius(8.0)
        .padding(10.0)
        .clip_children(true)
        .child(
            Column::new()
                .gap(8.0)
                .child(text("Octav IR Symbols", 13, colors.text))
                .child(list),
        )
        .into_view()
}

#[cfg(test)]
/// Builds or computes the `symbol label` deterministic showcase fixture.
fn symbol_label(summary: &ailloli_ui_editor::code::CodeFileSummary, id: SymbolId) -> String {
    if id == SymbolId(0) {
        return "root".to_string();
    }
    summary
        .symbols
        .iter()
        .find(|symbol| symbol.id == id)
        .map(|symbol| format!("{:?} {}", symbol.kind, symbol.name))
        .unwrap_or_else(|| format!("SymbolId({})", id.0))
}

#[cfg(test)]
/// Builds or computes the `symbol depth` deterministic showcase fixture.
fn symbol_depth(summary: &ailloli_ui_editor::code::CodeFileSummary, id: SymbolId) -> usize {
    let mut depth = 0;
    let mut current = summary
        .symbols
        .iter()
        .find(|symbol| symbol.id == id)
        .and_then(|symbol| symbol.parent);
    while let Some(parent) = current {
        depth += 1;
        current = summary
            .symbols
            .iter()
            .find(|symbol| symbol.id == parent)
            .and_then(|symbol| symbol.parent);
    }
    depth
}

#[cfg(test)]
/// Builds or computes the `phase54 3 symbol graph fixture` deterministic showcase fixture.
fn phase54_3_symbol_graph_fixture() -> String {
    [
        "use crate::runtime::build;",
        "",
        "pub struct Parser;",
        "",
        "impl Parser {",
        "    pub fn parse(&self) {",
        "        helper();",
        "        build();",
        "        nested::helper();",
        "        missing_macro!();",
        "        let _text = \"helper() build()\";",
        "        let _raw = r#\"nested::helper()\"#;",
        "        // helper()",
        "    }",
        "}",
        "",
        "mod nested {",
        "    pub fn caller() {",
        "        helper();",
        "    }",
        "",
        "    pub fn helper() {}",
        "}",
        "",
        "pub fn helper() {}",
        "pub fn build() {}",
    ]
    .join("\n")
}

#[cfg(test)]
/// Builds or computes the `phase54 3 search fixture` deterministic showcase fixture.
fn phase54_3_search_fixture() -> String {
    [
        "pub fn main() {",
        "    let value = 1;",
        "    let value_count = value + 1;",
        "    let other = compute(value);",
        "    // value in comments is highlighted by textual search",
        "    let text = \"value inside string\";",
        "    println!(\"{}\", value);",
        "}",
        "",
        "fn compute(value: i32) -> i32 {",
        "    value + 42",
        "}",
        "",
        "fn late_match() {",
        "    let value = compute(10);",
        "}",
    ]
    .join("\n")
}

#[cfg(test)]
/// Builds or computes the `phase54 3 multiclick selection fixture` deterministic showcase fixture.
fn phase54_3_multiclick_selection_fixture() -> String {
    [
        "pub fn demo<'a>() {",
        "    let foo_bar = parser.parse::<usize>();",
        "    let r#async = foo_bar + 42;",
        "    println!(\"value {foo_bar}\");",
        "    let lifetime: &'a str = \"token\";",
        "}",
    ]
    .join("\n")
}

#[cfg(test)]
/// Builds or computes the `phase54 3 diagnostics fixture` deterministic showcase fixture.
fn phase54_3_diagnostics_fixture() -> String {
    [
        "pub fn main() {",
        "    let unused_value = compute(1);",
        "    let shadowed = 2;",
        "    let shadowed = shadowed + 1;",
        "    let typed: i32 = shadowed;",
        "    let simplified = (typed + 0);",
        "}",
        "",
        "fn compute(input: i32) -> i32 {",
        "    input + 42",
        "}",
    ]
    .join("\n")
}

#[cfg(test)]
/// Builds or computes the `phase54 3 folding fixture` deterministic showcase fixture.
fn phase54_3_folding_fixture() -> String {
    [
        "pub mod phase54_3 {",
        "    pub fn collapsed_region(input: i32) -> i32 {",
        "        let doubled = input * 2;",
        "        let adjusted = doubled + 4;",
        "        adjusted",
        "    }",
        "",
        "    pub fn visible_call() -> i32 {",
        "        let value = helper_block();",
        "        value + 1",
        "    }",
        "",
        "    fn helper_block() -> i32 {",
        "        42",
        "    }",
        "}",
    ]
    .join("\n")
}

#[cfg(test)]
/// Builds or computes the `phase54 3 ide folding gutter fixture` deterministic showcase fixture.
fn phase54_3_ide_folding_gutter_fixture() -> String {
    let mut lines = Vec::with_capacity(1_655);
    for idx in 1..=1_655 {
        let line = match idx {
            1627 => "pub mod ide_folding_gutter {".to_string(),
            1628 => "    pub fn section_a(value: i32) -> i32 {".to_string(),
            1629 => "        let computed = value + 1;".to_string(),
            1630 => "        computed".to_string(),
            1631 => "    }".to_string(),
            1632 => "    pub fn section_b(value: i32) -> i32 {".to_string(),
            1633 => "        if value > 4 { value } else { 4 }".to_string(),
            1634 => "    }".to_string(),
            1635 => "    impl Runner {".to_string(),
            1636 => "        pub fn run(&self) {".to_string(),
            1637 => "            self.step();".to_string(),
            1638 => "        pub fn step(&self) {".to_string(),
            1639 => "            println!(\"fold guide\");".to_string(),
            1640 => "        }".to_string(),
            1641 => "    pub enum Mode {".to_string(),
            1642 => "        Fast,".to_string(),
            1643 => "        Slow,".to_string(),
            1644 => "    pub fn final_block() {".to_string(),
            1645 => "        let marker = section_a(42);".to_string(),
            1646 => "        println!(\"{}\", marker);".to_string(),
            1647 => "    }".to_string(),
            1648 => "}".to_string(),
            _ => format!("// filler line {idx:04} keeps deep IDE-style scroll deterministic"),
        };
        lines.push(line);
    }
    lines.join("\n")
}

#[cfg(test)]
/// Builds or computes the `phase54 3 ide folding regions` deterministic showcase fixture.
fn phase54_3_ide_folding_regions() -> Vec<FoldRegion> {
    vec![
        FoldRegion::new(1627, 1630),
        FoldRegion::new(1631, 1633).collapsed(true),
        FoldRegion::new(1634, 1640),
        FoldRegion::new(1635, 1637),
        FoldRegion::new(1640, 1642).collapsed(true),
        FoldRegion::new(1643, 1646),
    ]
}

#[cfg(test)]
/// Builds or computes the `phase54 3 lsp fixture` deterministic showcase fixture.
fn phase54_3_lsp_fixture() -> String {
    [
        "pub fn main() {",
        "    let value = compute(41);",
        "    let missing = unresolved_call(value);",
        "    println!(\"{}\", missing);",
        "}",
        "",
        "fn compute(input: i32) -> i32 {",
        "    input + 1",
        "}",
    ]
    .join("\n")
}

#[cfg(test)]
/// Builds or computes the `phase54 3 scip code fixture` deterministic showcase fixture.
fn phase54_3_scip_code_fixture() -> String {
    [
        "pub mod helper;",
        "",
        "pub fn main() {",
        "    let value = helper::compute(41);",
        "    println!(\"{}\", value);",
        "}",
    ]
    .join("\n")
}

#[cfg(test)]
/// Builds or computes the `phase54 3 large file fixture` deterministic showcase fixture.
fn phase54_3_large_file_fixture() -> String {
    let mut lines = Vec::with_capacity(10_050);
    lines.push("pub fn large_file_entry() {".to_string());
    for idx in 0..10_020 {
        if idx == 9_999 {
            lines.push(format!(
                "    let very_long_binding_{idx} = \"{}\";",
                "phase54_3_large_file_horizontal_width_probe_".repeat(18)
            ));
        } else {
            lines.push(format!(
                "    let value_{idx:05} = compute_large_file_value({idx});"
            ));
        }
    }
    lines.push("}".to_string());
    lines.push("fn compute_large_file_value(input: usize) -> usize { input + 1 }".to_string());
    lines.join("\n")
}

#[cfg(test)]
/// Builds or computes the `phase54 3 theme variants fixture` deterministic showcase fixture.
fn phase54_3_theme_variants_fixture(label: &str) -> String {
    [
        "pub fn themed_editor(value: i32) -> i32 {",
        "    let computed_value = value + 42;",
        "    let label = \"value\";",
        &format!("    // {label} theme value should remain readable"),
        "    computed_value",
        "}",
    ]
    .join("\n")
}

#[cfg(test)]
/// Builds or computes the `code theme white variant` deterministic showcase fixture.
fn code_theme_white_variant() -> CodeTheme {
    CodeTheme {
        background: Color::rgb(250, 250, 250),
        foreground: Color::rgb(15, 23, 42),
        gutter_bg: Color::rgb(241, 245, 249),
        line_number: Color::rgb(100, 116, 139),
        active_line_number: Color::rgb(15, 23, 42),
        active_line_bg: Color::rgba(2, 132, 199, 0.08),
        active_line_ring: Color::rgba(2, 132, 199, 0.24),
        search_match_bg: Color::rgba(245, 158, 11, 0.26),
        search_active_match_bg: Color::rgba(249, 115, 22, 0.34),
        diagnostic_error: Color::rgb(220, 38, 38),
        diagnostic_warning: Color::rgb(217, 119, 6),
        diagnostic_info: Color::rgb(2, 132, 199),
        diagnostic_hint: Color::rgb(71, 85, 105),
        diagnostic_active_bg: Color::rgba(220, 38, 38, 0.10),
        fold_marker: Color::rgb(217, 119, 6),
        fold_marker_active: Color::rgb(249, 115, 22),
        fold_guide: Color::rgba(217, 119, 6, 0.34),
        syntax_keyword: Color::rgb(126, 34, 206),
        syntax_type: Color::rgb(14, 116, 144),
        syntax_function: Color::rgb(180, 83, 9),
        syntax_string: Color::rgb(158, 64, 26),
        syntax_number: Color::rgb(21, 128, 61),
        syntax_comment: Color::rgb(77, 124, 15),
        syntax_operator: Color::rgb(51, 65, 85),
        syntax_punctuation: Color::rgb(100, 116, 139),
        syntax_identifier: Color::rgb(15, 23, 42),
    }
}

#[cfg(test)]
/// Builds or computes the `code theme dark variant` deterministic showcase fixture.
fn code_theme_dark_variant() -> CodeTheme {
    CodeTheme {
        background: Color::rgb(15, 17, 23),
        foreground: Color::rgb(245, 247, 250),
        gutter_bg: Color::rgb(8, 10, 14),
        line_number: Color::rgb(210, 220, 235),
        active_line_number: Color::rgb(255, 255, 255),
        active_line_bg: Color::rgba(255, 255, 255, 0.05),
        active_line_ring: Color::rgba(255, 255, 255, 0.14),
        search_match_bg: Color::rgba(244, 196, 48, 0.28),
        search_active_match_bg: Color::rgba(249, 115, 22, 0.48),
        diagnostic_error: Color::rgb(239, 68, 68),
        diagnostic_warning: Color::rgb(245, 158, 11),
        diagnostic_info: Color::rgb(59, 130, 246),
        diagnostic_hint: Color::rgb(148, 163, 184),
        diagnostic_active_bg: Color::rgba(239, 68, 68, 0.12),
        fold_marker: Color::rgb(245, 158, 11),
        fold_marker_active: Color::rgb(249, 115, 22),
        fold_guide: Color::rgba(245, 158, 11, 0.42),
        syntax_keyword: Color::rgb(255, 130, 220),
        syntax_type: Color::rgb(87, 231, 205),
        syntax_function: Color::rgb(255, 230, 150),
        syntax_string: Color::rgb(255, 170, 130),
        syntax_number: Color::rgb(195, 235, 180),
        syntax_comment: Color::rgb(130, 190, 105),
        syntax_operator: Color::rgb(245, 247, 250),
        syntax_punctuation: Color::rgb(220, 226, 235),
        syntax_identifier: Color::rgb(245, 247, 250),
    }
}

#[cfg(test)]
/// Builds or computes the `phase54 3 scip project summary` deterministic showcase fixture.
fn phase54_3_scip_project_summary() -> ailloli_ui_editor::code::ScipProjectSummary {
    let json = r#"{
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
                        "range": { "start": 17, "end": 91 },
                        "selection_range": { "start": 24, "end": 28 },
                        "signature": "pub fn main()",
                        "docs": null
                    }
                ],
                "occurrences": [
                    {
                        "symbol": "local src/lib.rs main().",
                        "range": { "start": 24, "end": 28 },
                        "role": "Definition"
                    },
                    {
                        "symbol": "local src/helper.rs compute().",
                        "range": { "start": 45, "end": 52 },
                        "role": "Reference"
                    }
                ],
                "relations": []
            },
            {
                "path": "src/helper.rs",
                "language": "Rust",
                "version": 1,
                "symbols": [
                    {
                        "symbol": "local src/helper.rs compute().",
                        "name": "compute",
                        "kind": "Function",
                        "range": { "start": 0, "end": 39 },
                        "selection_range": { "start": 7, "end": 14 },
                        "signature": "pub fn compute(input: i32) -> i32",
                        "docs": "shared project helper"
                    }
                ],
                "occurrences": [
                    {
                        "symbol": "local src/helper.rs compute().",
                        "range": { "start": 7, "end": 14 },
                        "role": "Definition"
                    }
                ],
                "relations": []
            }
        ]
    }"#;
    let index = ailloli_ui_editor::code::import_scip_json_str(json).expect("scip showcase fixture");
    ailloli_ui_editor::code::scip_project_to_summary(&index)
}

#[cfg(test)]
/// Builds or computes the `phase54 3 lsp mock enrichment` deterministic showcase fixture.
fn phase54_3_lsp_mock_enrichment(
    document: &Document,
    code: &str,
) -> ailloli_ui_editor::code::LspEnrichment {
    ailloli_ui_editor::code::LspEnrichment {
        document_version: document.version,
        capabilities: ailloli_ui_editor::code::LspCapabilities {
            document_symbols: true,
            references: true,
            diagnostics: true,
            ..ailloli_ui_editor::code::LspCapabilities::default()
        },
        symbols: vec![
            SemanticDocumentSymbol {
                name: "main".into(),
                kind: SymbolKind::Function,
                range: diagnostic_fixture_range(code, "pub fn main"),
                selection_range: diagnostic_fixture_range(code, "main"),
                detail: Some("pub fn main()".into()),
                source: SymbolSource::Lsp,
            },
            SemanticDocumentSymbol {
                name: "compute".into(),
                kind: SymbolKind::Function,
                range: diagnostic_fixture_range(code, "fn compute"),
                selection_range: diagnostic_fixture_range(code, "compute"),
                detail: Some("fn compute(input: i32) -> i32".into()),
                source: SymbolSource::Lsp,
            },
        ],
        references: vec![SemanticReference {
            from: SymbolId(1),
            to: SymbolId(2),
            kind: SymbolEdgeKind::References,
            source: SymbolSource::Lsp,
        }],
        diagnostics: vec![
            Diagnostic::lsp(
                diagnostic_fixture_range(code, "unresolved_call"),
                DiagnosticSeverity::Error,
                "unresolved function",
                document.version,
            ),
            Diagnostic::lsp(
                diagnostic_fixture_range(code, "missing"),
                DiagnosticSeverity::Info,
                "hover/type info available",
                document.version,
            ),
        ],
    }
}

#[cfg(test)]
/// Builds or computes the `phase54 3 diagnostics for fixture` deterministic showcase fixture.
fn phase54_3_diagnostics_for_fixture(code: &str) -> Vec<Diagnostic> {
    vec![
        Diagnostic::new(
            diagnostic_fixture_range(code, "unused_value = compute(1)"),
            DiagnosticSeverity::Error,
            "unused value",
        ),
        Diagnostic::new(
            diagnostic_fixture_range(code, "shadowed = 2"),
            DiagnosticSeverity::Warning,
            "shadowed binding",
        ),
        Diagnostic::new(
            diagnostic_fixture_range(code, "typed: i32"),
            DiagnosticSeverity::Info,
            "inferred type available",
        ),
        Diagnostic::new(
            diagnostic_fixture_range(code, "typed + 0"),
            DiagnosticSeverity::Hint,
            "simplify expression",
        ),
    ]
}

#[cfg(test)]
/// Builds or computes the `diagnostic fixture range` deterministic showcase fixture.
fn diagnostic_fixture_range(code: &str, needle: &str) -> std::ops::Range<usize> {
    let start = code.find(needle).expect("diagnostic fixture needle");
    start..start + needle.len()
}

#[cfg(test)]
/// Builds or computes the `phase54 3 symbol outline fixture` deterministic showcase fixture.
fn phase54_3_symbol_outline_fixture() -> String {
    [
        "use crate::fmt::Display;",
        "",
        "/// Parser docs",
        "#[doc = \"Parser attribute docs\"]",
        "pub struct Parser<'a> {",
        "    source: &'a str,",
        "    count: usize,",
        "}",
        "",
        "pub enum Mode {",
        "    Fast,",
        "    Slow,",
        "}",
        "",
        "pub trait Runnable {",
        "    fn run(&self) -> usize;",
        "}",
        "",
        "impl<'a> Parser<'a> {",
        "    pub const LIMIT: usize = 42;",
        "    pub static NAME: &str = \"parser\";",
        "    pub type Output = usize;",
        "    pub fn new(source: &'a str) -> Self { Self { source, count: 0 } }",
        "    pub fn parse(&self) -> usize { helper(); self.count }",
        "}",
        "",
        "macro_rules! trace_value {",
        "    ($value:expr) => { println!(\"{}\", $value) };",
        "}",
        "",
        "mod nested {",
        "    pub fn helper() {}",
        "}",
        "",
        "fn helper() {}",
    ]
    .join("\n")
}

#[cfg(test)]
/// Builds or computes the `phase54 3 code editor fixture` deterministic showcase fixture.
fn phase54_3_code_editor_fixture() -> String {
    let mut lines = vec![
        "use ailloli_ui::prelude::*;".to_string(),
        "use ailloli_ui_widgets::editor::{CodeEditor, Document, DocumentId};".to_string(),
        String::new(),
        "pub fn build_phase54_3_editor() -> View<()> {".to_string(),
        "    // styled spans should color comments without changing layout".to_string(),
        "    let document = State::new(Document::new(DocumentId(54), TextBuffer::from_string(String::new())));".to_string(),
        "    let very_long_binding_name_for_horizontal_scroll_validation = \"this line is intentionally long so the visual test can confirm text scrolls under a fixed gutter\";".to_string(),
    ];
    for i in 0..48 {
        lines.push(format!(
            "    let row_{i:02} = format!(\"phase54_3 deterministic row {i:02}: {{}}\", very_long_binding_name_for_horizontal_scroll_validation);"
        ));
    }
    lines.extend([
        "    CodeEditor::new(document).line_numbers(true).into_view()".to_string(),
        "}".to_string(),
    ]);
    lines.join("\n")
}

/// Builds or computes the `scroll section` deterministic showcase fixture.
fn scroll_section(mode: ShowcaseMode) -> View<()> {
    let colors = mode.palette();
    let mut rows = Column::new().gap(6.0).padding(10.0);
    for i in 1..=22 {
        rows = rows.child(text(format!("Scrollable row {i:02}"), 12, colors.text));
    }

    Container::new()
        .width(420.0)
        .height(180.0)
        .background(colors.elevated)
        .border(1.0, colors.border)
        .radius(8.0)
        .clip_children(true)
        .child(ScrollView::vertical().child(rows))
        .into_view()
}

/// Builds or computes the `terminal view section` deterministic showcase fixture.
fn terminal_view_section(mode: ShowcaseMode, scenario: TerminalShowcaseScenario) -> View<()> {
    let colors = mode.palette();
    let (query, selection, scroll_y) = match scenario {
        TerminalShowcaseScenario::Default => {
            ("target", Some(TerminalSelection::lines(10, 11)), 96.0)
        }
        TerminalShowcaseScenario::Search => ("ailloli_ui_widgets", None, 0.0),
        TerminalShowcaseScenario::Selection => ("", Some(TerminalSelection::lines(13, 15)), 132.0),
    };
    let terminal_key = match scenario {
        TerminalShowcaseScenario::Default => "terminal-view-widget-default",
        TerminalShowcaseScenario::Search => "terminal-view-widget-search",
        TerminalShowcaseScenario::Selection => "terminal-view-widget-selection",
    };
    let mut terminal_style = TerminalViewStyle::from_theme(Theme::default());
    terminal_style.height = 278.0;
    terminal_style.width = 760.0;
    let mut terminal = TerminalView::new()
        .terminal_style(terminal_style)
        .fill_width()
        .lines(terminal_phase54_1_fixture())
        .search_query(query)
        .initial_scroll_y(scroll_y);
    if let Some(selection) = selection {
        terminal = terminal.selection(selection);
    }

    Column::new()
        .gap(8.0)
        .child(
            Row::new()
                .gap(8.0)
                .child(terminal_badge(colors, "read-only"))
                .child(terminal_badge(colors, "external stream"))
                .child(terminal_badge(colors, "history/search/selection")),
        )
        .child(terminal.key(terminal_key))
        .into_view()
}

#[cfg(test)]
/// Builds or computes the `terminal phase77 section` deterministic showcase fixture.
fn terminal_phase77_section(mode: ShowcaseMode) -> View<()> {
    let colors = mode.palette();
    let mut terminal_style = TerminalWidgetStyle::from_theme(Theme::default());
    terminal_style.height = 292.0;
    terminal_style.width = 820.0;

    Column::new()
        .gap(8.0)
        .child(
            Row::new()
                .gap(8.0)
                .child(terminal_badge(colors, "state-backed"))
                .child(terminal_badge(colors, "ANSI/OSC parsed fixture"))
                .child(terminal_badge(colors, "cursor/input-ready")),
        )
        .child(
            Terminal::new(State::new(terminal_phase77_fixture()))
                .terminal_style(terminal_style)
                .auto_resize(false)
                .selection(TerminalSelection::lines(12, 14))
                .initial_scroll_y(118.0)
                .fill_width()
                .key("terminal-widget-v2"),
        )
        .into_view()
}

#[cfg(test)]
/// Builds or computes the `terminal phase77 fixture` deterministic showcase fixture.
fn terminal_phase77_fixture() -> ailloli_ui::terminal_core::TerminalState {
    let mut state = ailloli_ui::terminal_core::TerminalState::with_config(
        ailloli_ui::terminal_core::TerminalConfig {
            size: ailloli_ui::terminal_core::TerminalSize::new(14, 76),
            scrollback_limit: 80,
            security: ailloli_ui::terminal_core::TerminalSecurityPolicy::default(),
        },
    );
    let mut parser = ailloli_ui::terminal_core::VteTerminalParser::new();
    let fixture = concat!(
        "\x1b]2;Ailloli UI Phase 77 terminal\x07",
        "\x1b[1;36mailloli_ui terminal fixture - phase 77\x1b[0m\r\n",
        "\x1b[32m$ cargo test -p ailloli_ui_terminal_core parser\x1b[0m\r\n",
        "running 9 tests\r\n",
        "\x1b[32mtest parser::osc8_hyperlink_on_cells ... ok\x1b[0m\r\n",
        "\x1b[32mtest parser::sgr_truecolor_and_indexed ... ok\x1b[0m\r\n",
        "\x1b[33mwarning: PTY backend intentionally not attached in widget V2\x1b[0m\r\n",
        "\x1b[34m$ printf '\\x1b[38;5;196mred\\x1b[0m + truecolor'\x1b[0m\r\n",
        "\x1b[38;5;196mred indexed foreground\x1b[0m\r\n",
        "\x1b[48;5;24mblue background block    \x1b[0m\r\n",
        "\x1b[38;2;255;180;64mtruecolor amber text\x1b[0m\r\n",
        "\x1b[7minverse video sample\x1b[0m\r\n",
        "$ rg TerminalState ailloli_ui_widgets/src/controls\r\n",
        "terminal_widget.rs: Terminal::new(State<TerminalState>)\r\n",
        "terminal_widget.rs: render normal screen + scrollback\r\n",
        "terminal_widget.rs: map keyboard bytes for external PTY\r\n",
        "$ echo 'cursor visible at prompt'\r\n",
        "cursor visible at prompt\r\n",
        "$ _"
    );
    ailloli_ui::terminal_core::TerminalParser::advance(&mut parser, &mut state, fixture.as_bytes());
    state
}

#[cfg(test)]
/// Builds or computes the `terminal phase78 style` deterministic showcase fixture.
fn terminal_phase78_style(height: f32) -> TerminalWidgetStyle {
    let mut terminal_style = TerminalWidgetStyle::from_theme(Theme::default());
    terminal_style.height = height;
    terminal_style.width = 820.0;
    terminal_style
}

#[cfg(test)]
/// Builds or computes the `terminal phase78 scrollback section` deterministic showcase fixture.
fn terminal_phase78_scrollback_section(mode: ShowcaseMode) -> View<()> {
    let colors = mode.palette();
    Column::new()
        .gap(8.0)
        .child(
            Row::new()
                .gap(8.0)
                .child(terminal_badge(colors, "follow-output"))
                .child(terminal_badge(colors, "scrollback"))
                .child(terminal_badge(colors, "selection/copy")),
        )
        .child(
            Terminal::new(State::new(terminal_phase78_scrollback_fixture()))
                .terminal_style(terminal_phase78_style(236.0))
                .auto_resize(false)
                .follow_output(false)
                .selection(TerminalSelection::new(
                    TerminalPosition::new(18, 8),
                    TerminalPosition::new(20, 42),
                ))
                .selection_mode(TerminalSelectionMode::Line)
                .initial_scroll_y(190.0)
                .fill_width()
                .key("terminal-phase78-scrollback-widget"),
        )
        .into_view()
}

#[cfg(test)]
/// Builds or computes the `terminal phase78 tui section` deterministic showcase fixture.
fn terminal_phase78_tui_section(mode: ShowcaseMode) -> View<()> {
    let colors = mode.palette();
    Column::new()
        .gap(8.0)
        .child(
            Row::new()
                .gap(8.0)
                .child(terminal_badge(colors, "alternate screen"))
                .child(terminal_badge(colors, "bracketed paste"))
                .child(terminal_badge(colors, "mouse tracking")),
        )
        .child(
            Terminal::new(State::new(terminal_phase78_tui_fixture()))
                .terminal_style(terminal_phase78_style(236.0))
                .auto_resize(false)
                .follow_output(true)
                .fill_width()
                .key("terminal-phase78-tui-widget"),
        )
        .into_view()
}

#[cfg(test)]
/// Builds or computes the `terminal phase78 scrollback fixture` deterministic showcase fixture.
fn terminal_phase78_scrollback_fixture() -> ailloli_ui::terminal_core::TerminalState {
    let mut state = ailloli_ui::terminal_core::TerminalState::with_config(
        ailloli_ui::terminal_core::TerminalConfig {
            size: ailloli_ui::terminal_core::TerminalSize::new(10, 76),
            scrollback_limit: 120,
            security: ailloli_ui::terminal_core::TerminalSecurityPolicy::default(),
        },
    );
    let mut parser = ailloli_ui::terminal_core::VteTerminalParser::new();
    let fixture = concat!(
        "\x1b[1;36mphase 78 scrollback fixture\x1b[0m\r\n",
        "$ cargo test -p ailloli_ui_widgets terminal\r\n",
        "running terminal viewport tests\r\n",
        "01 append while following keeps the prompt visible\r\n",
        "02 user scroll disables follow-output\r\n",
        "03 jump bottom restores follow-output\r\n",
        "04 selection keeps viewport stable\r\n",
        "\x1b[32m05 copy uses runtime clipboard\x1b[0m\r\n",
        "\x1b[33m06 paste uses bracketed paste when enabled\x1b[0m\r\n",
        "07 wide char selection: e\u{301} + 界 stays normalized\r\n",
        "08 mouse selection remains local with Shift\r\n",
        "09 scrollback trim preserves order\r\n",
        "10 viewport maps pixels to terminal rows\r\n",
        "11 terminal_selection_text extracts selected ranges\r\n",
        "12 clipboard and OSC 52 security remain separate\r\n",
        "13 follow-output=false preserves inspection position\r\n",
        "14 scrollbar thumb indicates historical output\r\n",
        "15 selected block is intentionally highlighted\r\n",
        "$ _"
    );
    ailloli_ui::terminal_core::TerminalParser::advance(&mut parser, &mut state, fixture.as_bytes());
    state
}

#[cfg(test)]
/// Builds or computes the `terminal phase78 tui fixture` deterministic showcase fixture.
fn terminal_phase78_tui_fixture() -> ailloli_ui::terminal_core::TerminalState {
    let mut state = ailloli_ui::terminal_core::TerminalState::with_config(
        ailloli_ui::terminal_core::TerminalConfig {
            size: ailloli_ui::terminal_core::TerminalSize::new(10, 76),
            scrollback_limit: 80,
            security: ailloli_ui::terminal_core::TerminalSecurityPolicy::default(),
        },
    );
    let mut parser = ailloli_ui::terminal_core::VteTerminalParser::new();
    let fixture = concat!(
        "\x1b[?1049h",
        "\x1b[?1h\x1b=\x1b[?1002h\x1b[?1006h\x1b[?2004h",
        "\x1b[1;1H\x1b[48;5;24m                                                                            \x1b[0m",
        "\x1b[2;1H\x1b[1;37m Ailloli UI TUI mode fixture                                             \x1b[0m",
        "\x1b[3;1H\x1b[38;5;82m application cursor: ON  keypad: ON  bracketed paste: ON\x1b[0m",
        "\x1b[4;1H\x1b[38;5;214m mouse tracking: button motion + SGR protocol\x1b[0m",
        "\x1b[5;1H┌──────────────────────────────┬──────────────────────────────────────┐",
        "\x1b[6;1H│ alternate screen only          │ scrollback hidden in TUI             │",
        "\x1b[7;1H│ Shift+mouse selects locally     │ mouse events forward without Shift   │",
        "\x1b[8;1H│ Paste wraps with ESC[200~/201~  │ cursor remains visible               │",
        "\x1b[9;1H└──────────────────────────────┴──────────────────────────────────────┘",
        "\x1b[10;3H_"
    );
    ailloli_ui::terminal_core::TerminalParser::advance(&mut parser, &mut state, fixture.as_bytes());
    state
}

#[cfg(test)]
/// Builds or computes the `terminal phase80 section` deterministic showcase fixture.
fn terminal_phase80_section(mode: ShowcaseMode) -> View<()> {
    let colors = mode.palette();
    Column::new()
        .gap(8.0)
        .child(
            Row::new()
                .gap(8.0)
                .child(terminal_badge(colors, "diagnostics"))
                .child(terminal_badge(colors, "rustc/cargo"))
                .child(terminal_badge(colors, "IDE links")),
        )
        .child(
            Terminal::new(State::new(terminal_phase80_fixture()))
                .terminal_style(terminal_phase78_style(292.0))
                .auto_resize(false)
                .follow_output(false)
                .initial_scroll_y(0.0)
                .fill_width()
                .key("terminal-phase80-diagnostics-widget"),
        )
        .into_view()
}

#[cfg(test)]
/// Builds or computes the `terminal phase80 fixture` deterministic showcase fixture.
fn terminal_phase80_fixture() -> ailloli_ui::terminal_core::TerminalState {
    let mut state = ailloli_ui::terminal_core::TerminalState::with_config(
        ailloli_ui::terminal_core::TerminalConfig {
            size: ailloli_ui::terminal_core::TerminalSize::new(14, 76),
            scrollback_limit: 120,
            security: ailloli_ui::terminal_core::TerminalSecurityPolicy::default(),
        },
    );
    let mut parser = ailloli_ui::terminal_core::VteTerminalParser::new();
    let fixture = concat!(
        "\x1b]9001;ailloli_ui:cwd;uri=file:///workspace/ailloli_ui\x07",
        "\x1b]9001;ailloli_ui:command_start;cmd=cargo%20test%20-p%20ailloli_ui_terminal_core\x07",
        "\x1b[1;36m$ cargo test -p ailloli_ui_terminal_core diagnostics\x1b[0m\r\n",
        "running diagnostics fixtures\r\n",
        "\x1b[31merror[E0502]: cannot borrow `state` as mutable\x1b[0m\r\n",
        "  --> src/main.rs:42:13\r\n",
        "thread 'main' panicked at src/lib.rs:88:5: boom\r\n",
        "test result: FAILED. 1 passed; 1 failed\r\n",
        "\x1b]9001;ailloli_ui:command_end;exit=101\x07",
        "\x1b[33mnpm ERR! missing script: build\x1b[0m\r\n",
        "\x1b[33mCONFLICT (content): Merge conflict in src/app.rs\x1b[0m\r\n",
        "The authenticity of host 'github.com' can't be established.\r\n",
        "[sudo] password for chaos:\r\n",
        "docs: https://example.test/report\r\n",
        "$ _"
    );
    ailloli_ui::terminal_core::TerminalParser::advance(&mut parser, &mut state, fixture.as_bytes());
    state.classify_terminal_output();
    state
}

/// Builds or computes the `terminal badge` deterministic showcase fixture.
fn terminal_badge(colors: ShowcasePalette, label: &'static str) -> View<()> {
    Container::new()
        .height(24.0)
        .background(colors.elevated)
        .border(1.0, colors.border)
        .radius(6.0)
        .padding(6.0)
        .child(mono(label, 11, colors.muted))
        .into_view()
}

/// Builds or computes the `terminal phase54 1 fixture` deterministic showcase fixture.
fn terminal_phase54_1_fixture() -> Vec<TerminalLine> {
    vec![
        TerminalLine::system("ailloli_ui terminal fixture - phase 54.1"),
        TerminalLine::prompt(
            "dev@example:~/projects/ailloli_ui$ cargo check -p ailloli_ui_widgets terminal",
        ),
        TerminalLine::new("   Compiling ailloli_ui_core v0.1.0"),
        TerminalLine::new("   Compiling ailloli_ui_text v0.1.0"),
        TerminalLine::new("   Compiling ailloli_ui_widgets v0.1.0"),
        TerminalLine::new("    Finished dev target(s) in 1.42s"),
        TerminalLine::prompt("dev@example:~/projects/ailloli_ui$ git status --short"),
        TerminalLine::new(" M ailloli_ui_widgets/src/controls/terminal.rs"),
        TerminalLine::new(" M ailloli_ui_winit/examples/support/ui_bundle_showcase.rs"),
        TerminalLine::prompt("dev@example:~/projects/ailloli_ui$ rg TerminalView README.md"),
        TerminalLine::new("1021:### Phase 54.1 - TerminalView"),
        TerminalLine::new("1022:- [x] finished Implementer TerminalView read-only monospace"),
        TerminalLine::new(
            "1025:- [x] finished Ajouter capture opt-in ui_bundle_phase54_1_terminal_view.png",
        ),
        TerminalLine::prompt(
            "dev@example:~/projects/ailloli_ui$ cargo test -p ailloli_ui_widgets terminal",
        ),
        TerminalLine::new("running 4 tests"),
        TerminalLine::success("test controls::terminal::tests::buffer_trims_history ... ok"),
        TerminalLine::success(
            "test controls::terminal::tests::chunk_events_keep_partial_line_visible ... ok",
        ),
        TerminalLine::success(
            "test controls::terminal::tests::search_finds_ascii_case_insensitive_matches ... ok",
        ),
        TerminalLine::success(
            "test controls::terminal::tests::selection_normalizes_and_clamps ... ok",
        ),
        TerminalLine::new("test result: ok. 4 passed; 0 failed; 0 ignored"),
        TerminalLine::prompt(
            "dev@example:~/projects/ailloli_ui$ ./scripts/build-ui --target terminal",
        ),
        TerminalLine::new("[stream] received stdout chunk: build started"),
        TerminalLine::warning(
            "[stream] warning: ANSI colors are intentionally metadata-only in v1",
        ),
        TerminalLine::new("[stream] target terminal-readonly produced 27 visible history rows"),
        TerminalLine::new("[search] cache hit: target/debug/incremental/ailloli_ui_widgets"),
        TerminalLine::new("[search] cache hit: target/debug/deps/ailloli_ui_widgets_terminal"),
        TerminalLine::stderr("stderr: no PTY backend attached in phase 54.1"),
        TerminalLine::system("status: waiting for external consumer events"),
        TerminalLine::prompt("dev@example:~/projects/ailloli_ui$ _"),
    ]
}

/// Builds or computes the `planned widgets section` deterministic showcase fixture.
fn planned_widgets_section(mode: ShowcaseMode) -> View<()> {
    let colors = mode.palette();
    let items = [("Final visual audit", "Phase 55", "missing")];

    let mut outer = Column::new().gap(8.0);
    for chunk in items.chunks(4) {
        let mut row = Row::new().gap(8.0);
        for (name, phase, status) in chunk {
            row = row.child(planned_card(name, phase, status, colors));
        }
        outer = outer.child(row);
    }
    outer.into_view()
}

/// Builds or computes the `planned card` deterministic showcase fixture.
fn planned_card(
    name: &'static str,
    phase: &'static str,
    status: &'static str,
    colors: ShowcasePalette,
) -> View<()> {
    let status_color = if status == "partial" {
        colors.warning
    } else {
        colors.muted
    };
    Container::new()
        .width(200.0)
        .height(70.0)
        .background(colors.elevated)
        .border(1.0, colors.border)
        .radius(8.0)
        .padding(8.0)
        .child(
            Column::new()
                .gap(4.0)
                .child(text(name, 13, colors.text))
                .child(text(phase, 12, colors.accent))
                .child(text(status, 11, status_color)),
        )
        .into_view()
}
