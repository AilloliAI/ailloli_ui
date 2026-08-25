//! Premium native showcase assembled entirely from the public façade.

use crate::content::{
    topic, Capability, Resource, ResourceAvailability, CAPABILITIES, GUIDE_ENTRIES,
    INITIAL_REACTIVE_HEADLINE, QUICK_START_RUST, RESOURCES,
};

use super::prelude::*;

/// Stable application logo reused as visible brand artwork.
const APP_LOGO_SVG: &str = include_str!("../assets/icons/icon.svg");

/// Small external-link glyph used by active resource pills.
const EXTERNAL_LINK_SVG: &str = r#"<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 24 24" fill="none" stroke="white" stroke-width="2" stroke-linecap="round" stroke-linejoin="round"><path d="M15 3h6v6"/><path d="M10 14 21 3"/><path d="M18 13v6a2 2 0 0 1-2 2H5a2 2 0 0 1-2-2V8a2 2 0 0 1 2-2h6"/></svg>"#;

/// Shared retained state used by the editable quick start and documentation explorer.
#[derive(Clone)]
pub(crate) struct ShowcaseState {
    /// Two-way headline demonstrated by both a TextInput and Text widget.
    reactive_headline: State<String>,
    /// Writable selected TreeView identity.
    selected_topic: State<String>,
    /// Selected topic title rendered in the adjacent detail panel.
    selected_topic_title: State<String>,
    /// Selected topic summary rendered in the adjacent detail panel.
    selected_topic_summary: State<String>,
    /// Editable Rust quick-start document.
    quick_start: State<Document>,
}

impl ShowcaseState {
    /// Creates the deterministic initial state shared by the app and visual tests.
    pub(crate) fn new() -> Self {
        let initial_topic = topic("facade");
        Self {
            reactive_headline: State::new(INITIAL_REACTIVE_HEADLINE.to_string()),
            selected_topic: State::new(initial_topic.id.to_string()),
            selected_topic_title: State::new(initial_topic.title.to_string()),
            selected_topic_summary: State::new(initial_topic.summary.to_string()),
            quick_start: State::new(
                Document::new(DocumentId(129), TextBuffer::from_string(QUICK_START_RUST))
                    .with_language(EditorLanguage::Rust)
                    .with_path("src/main.rs"),
            ),
        }
    }
}

/// Runs the interactive, single-window public framework showcase.
///
/// # Errors
///
/// Returns any application identity, icon, native host, renderer, capture,
/// persistence, or event-loop failure reported by
/// [`ailloli_ui::AppBuilder::run`].
pub(crate) fn run() -> ailloli_ui::Result<()> {
    let state = ShowcaseState::new();
    let window_state = state.clone();

    App::new()
        .identity(
            AppIdentity::new()
                .id("org.ailloli.sandboxapp")
                .name("Ailloli UI Framework Showcase")
                .icon(app_icon!()),
        )
        .window(
            Window::new("main")
                .title("Ailloli UI — Framework Showcase")
                .size(1280.0, 900.0)
                .ailloli_ui_chrome()
                .content(move || showcase_root(window_state.clone())),
        )
        .run()
}

/// Builds the full scrollable showcase used by the real application and top capture.
pub(crate) fn showcase_root(state: ShowcaseState) -> View<Action> {
    let theme = Theme::default();
    let palette = theme.palette();

    Container::new()
        .fill()
        .background(palette.background)
        .child(
            ScrollView::vertical()
                .child(
                    Column::new()
                        .fill_width()
                        .align_items(AlignItems::Center)
                        .child(
                            Column::new()
                                .fill_width()
                                .max_width(1200.0)
                                .padding(32.0)
                                .gap(34.0)
                                .child(header(theme))
                                .child(hero(theme, state.quick_start.clone()))
                                .child(capabilities_section(theme))
                                .child(reactive_section(theme, state.clone()))
                                .child(documentation_explorer(theme, state.clone()))
                                .child(guide_section(theme))
                                .child(resources_section(theme))
                                .child(footer(theme)),
                        ),
                )
                .fill(),
        )
        .into_view()
        .key("sandbox-showcase-root")
}

/// Builds the lower documentation surface directly for deterministic capture.
#[cfg(test)]
pub(crate) fn documentation_capture_root(state: ShowcaseState) -> View<Action> {
    let theme = Theme::default();
    let palette = theme.palette();
    Container::new()
        .fill()
        .background(palette.background)
        .padding(16.0)
        .child(
            Column::new()
                .fill()
                .gap(10.0)
                .child(header(theme))
                .child(section_heading(
                    theme,
                    "PUBLIC ARCHITECTURE",
                    "Navigate the actual public architecture",
                    "A real TreeView and retained selection turn the framework itself into useful documentation.",
                ))
                .child(documentation_explorer_panel(theme, state, 200.0, true))
                .child(section_heading(
                    theme,
                    "LEARNING RESOURCES",
                    "Live destinations and an honest roadmap",
                    "Available resources are active; future documentation stays visibly disabled until a canonical site exists.",
                ))
                .child(resource_cards(theme, 130.0)),
        )
        .into_view()
        .key("sandbox-documentation-capture-root")
}

/// Builds the compact brand row and immediately useful live resources.
fn header(theme: Theme) -> View<Action> {
    let palette = theme.palette();
    let mut live_resources = Row::new().gap(10.0).align_items(AlignItems::Center);
    for resource in &RESOURCES[..2] {
        live_resources = live_resources.child(resource_pill(theme, *resource));
    }

    Row::new()
        .fill_width()
        .align_items(AlignItems::Center)
        .child(
            Row::new()
                .gap(12.0)
                .align_items(AlignItems::Center)
                .child(Icon::svg_str(APP_LOGO_SVG).size(42.0))
                .child(
                    Column::new()
                        .gap(2.0)
                        .child(styled_text("Ailloli UI", 18, palette.text))
                        .child(styled_text(
                            "Native retained UI for Rust",
                            12,
                            palette.text_muted,
                        )),
                ),
        )
        .child(Container::new().flex_grow())
        .child(live_resources)
        .into_view()
        .key("sandbox-showcase-header")
}

/// Builds the editorial hero and its editable, copyable quick-start code.
fn hero(theme: Theme, quick_start: State<Document>) -> View<Action> {
    let palette = theme.palette();
    Row::new()
        .fill_width()
        .gap(34.0)
        .align_items(AlignItems::Stretch)
        .child(
            Container::new().width(505.0).child(
                Column::new()
                    .gap(18.0)
                    .child(
                        Badge::new("PUBLIC FRAMEWORK")
                            .tone(BadgeTone::Accent)
                            .variant(BadgeVariant::Outline),
                    )
                    .child(styled_text(
                        "Build expressive native interfaces in Rust.",
                        42,
                        palette.text,
                    ))
                    .child(styled_text(
                        "Ailloli UI combines a retained runtime, composable widgets, native windows, GPU rendering, and application-grade developer surfaces behind one façade.",
                        17,
                        palette.text_muted,
                    ))
                    .child(
                        Row::new()
                            .gap(8.0)
                            .child(Badge::new("Retained"))
                            .child(Badge::new("Native"))
                            .child(Badge::new("GPU rendered"))
                            .child(Badge::new("Composable")),
                    )
                    .child(resource_pill(theme, RESOURCES[0]))
                    .child(styled_text(
                        "Every surface on this page is built through ailloli_ui::prelude::*.",
                        12,
                        palette.text_muted,
                    )),
            ),
        )
        .child(
            Container::panel(theme)
                .width(595.0)
                .height(390.0)
                .padding(0.0)
                .clip_children(true)
                .child(
                    Column::new()
                        .fill()
                        .child(
                            Row::new()
                                .fill_width()
                                .padding(14.0)
                                .align_items(AlignItems::Center)
                                .child(styled_text("src/main.rs", 13, palette.text))
                                .child(Container::new().flex_grow())
                                .child(
                                    Badge::new("Editable quick start")
                                        .tone(BadgeTone::Info)
                                        .variant(BadgeVariant::Outline),
                                ),
                        )
                        .child(
                            CodeEditor::new(quick_start)
                                .language(EditorLanguage::Rust)
                                .line_numbers(true)
                                .wrap_mode(EditorWrapMode::SoftWrap)
                                .fill(),
                        ),
                ),
        )
        .into_view()
        .key("sandbox-showcase-hero")
}

/// Builds the curated framework capability grid.
fn capabilities_section(theme: Theme) -> View<Action> {
    let mut cards = Row::new()
        .fill_width()
        .gap(14.0)
        .align_items(AlignItems::Stretch);
    for (index, capability) in CAPABILITIES.iter().enumerate() {
        cards = cards.child(capability_card(theme, *capability, index == 0));
    }

    Column::new()
        .fill_width()
        .gap(18.0)
        .child(section_heading(
            theme,
            "FRAMEWORK CAPABILITIES",
            "A coherent stack, not a loose component catalog",
            "Each layer owns a clear responsibility and remains available to external consumers through explicit public contracts.",
        ))
        .child(cards)
        .into_view()
        .key("sandbox-capabilities-section")
}

/// Builds one capability card using only theme-derived colors and spacing.
fn capability_card(theme: Theme, capability: Capability, accent: bool) -> View<Action> {
    let palette = theme.palette();
    Card::new()
        .variant(if accent {
            CardVariant::Accent
        } else {
            CardVariant::Surface
        })
        .width(268.0)
        .height(198.0)
        .padding(18.0)
        .child(
            Column::new()
                .gap(12.0)
                .child(styled_text(capability.eyebrow, 11, palette.accent))
                .child(styled_text(capability.title, 18, palette.text))
                .child(styled_text(capability.description, 13, palette.text_muted)),
        )
        .into_view()
}

/// Builds a real two-way state example with visible retained output.
fn reactive_section(theme: Theme, state: ShowcaseState) -> View<Action> {
    let palette = theme.palette();
    Row::new()
        .fill_width()
        .gap(22.0)
        .align_items(AlignItems::Stretch)
        .child(
            Container::new().width(410.0).child(section_heading(
                theme,
                "LIVE RETAINED STATE",
                "Edit once, update every subscriber",
                "The input and preview share the same State<String>. Editing advances retained state and refreshes only dependent work.",
            )),
        )
        .child(
            Card::elevated()
                .width(700.0)
                .padding(20.0)
                .child(
                    Column::new()
                        .gap(14.0)
                        .child(styled_text("Shared state value", 12, palette.text_muted))
                        .child(
                            TextInput::<Action>::new()
                                .bind(state.reactive_headline.clone())
                                .fill_width(),
                        )
                        .child(
                            Container::new()
                                .fill_width()
                                .height(94.0)
                                .background(palette.accent.with_alpha(0.10))
                                .border(1.0, palette.accent.with_alpha(0.45))
                                .radius(theme.radius().lg)
                                .padding(20.0)
                                .child(styled_bound_text(
                                    state.reactive_headline,
                                    26,
                                    palette.text,
                                )),
                        ),
                ),
        )
        .into_view()
        .key("sandbox-reactive-state-section")
}

/// Builds a real architecture navigator whose selection updates adjacent copy.
fn documentation_explorer(theme: Theme, state: ShowcaseState) -> View<Action> {
    Column::new()
        .fill_width()
        .gap(18.0)
        .child(section_heading(
            theme,
            "DOCUMENTATION EXPLORER",
            "Navigate the actual public architecture",
            "Select a subsystem to read its role. The tree contains framework-owned concepts only—no product layer or private dependency.",
        ))
        .child(documentation_explorer_panel(theme, state, 380.0, false))
        .into_view()
        .key("sandbox-documentation-explorer")
}

/// Builds the selectable architecture tree and its adjacent retained detail.
fn documentation_explorer_panel(
    theme: Theme,
    state: ShowcaseState,
    height: f32,
    compact: bool,
) -> View<Action> {
    let palette = theme.palette();
    let selected_summary = state.selected_topic_summary.clone();
    let selected_title = state.selected_topic_title.clone();
    let tree = TreeView::<String, Action>::new()
        .nodes(framework_tree_nodes())
        .bind_selected(state.selected_topic)
        .default_expanded_many([
            "foundations".to_string(),
            "desktop".to_string(),
            "systems".to_string(),
        ])
        .on_select_ctx(move |ctx, id| {
            let selected = topic(&id);
            selected_title.set(selected.title.to_string());
            selected_summary.set(selected.summary.to_string());
            ctx.request_layout();
        })
        .fill_width();

    let details = if compact {
        Card::new()
            .variant(CardVariant::Accent)
            .width(720.0)
            .padding(18.0)
            .child(
                Column::new()
                    .gap(10.0)
                    .child(
                        Badge::new("Selected public subsystem")
                            .tone(BadgeTone::Accent)
                            .variant(BadgeVariant::Outline),
                    )
                    .child(styled_bound_text(
                        state.selected_topic_title,
                        24,
                        palette.text,
                    ))
                    .child(styled_bound_text(
                        state.selected_topic_summary,
                        14,
                        palette.text_muted,
                    )),
            )
            .into_view()
    } else {
        Card::new()
            .variant(CardVariant::Accent)
            .width(720.0)
            .padding(28.0)
            .child(
                Column::new()
                    .gap(16.0)
                    .child(
                        Badge::new("Selected public subsystem")
                            .tone(BadgeTone::Accent)
                            .variant(BadgeVariant::Outline),
                    )
                    .child(styled_bound_text(
                        state.selected_topic_title,
                        28,
                        palette.text,
                    ))
                    .child(styled_bound_text(
                        state.selected_topic_summary,
                        16,
                        palette.text_muted,
                    ))
                    .child(
                        Divider::horizontal()
                            .color(palette.accent.with_alpha(0.55))
                            .length(220.0),
                    )
                    .child(styled_text(
                        "The public dependency direction remains one-way: consumer → façade → framework layers.",
                        13,
                        palette.text,
                    ))
                    .child(resource_pill(theme, RESOURCES[0])),
            )
            .into_view()
    };

    Row::new()
        .fill_width()
        .height(height)
        .gap(18.0)
        .align_items(AlignItems::Stretch)
        .child(
            Container::panel(theme)
                .width(380.0)
                .padding(12.0)
                .clip_children(true)
                .child(ScrollView::vertical().child(tree).fill()),
        )
        .child(details)
        .into_view()
        .key("sandbox-documentation-explorer-panel")
}

/// Returns the real framework hierarchy rendered by the explorer TreeView.
fn framework_tree_nodes() -> Vec<TreeNode<String>> {
    vec![
        TreeNode::branch("foundations".to_string(), topic("foundations").title).children([
            topic_node("facade"),
            topic_node("runtime"),
            topic_node("text-editor"),
        ]),
        TreeNode::branch("desktop".to_string(), topic("desktop").title).children([
            topic_node("widgets"),
            topic_node("winit"),
            topic_node("rendering"),
        ]),
        TreeNode::branch("systems".to_string(), topic("systems").title).children([
            topic_node("filesystem"),
            topic_node("terminal"),
            topic_node("openxr"),
        ]),
    ]
}

/// Converts one canonical documentation topic into a TreeView leaf.
fn topic_node(id: &'static str) -> TreeNode<String> {
    let topic = topic(id);
    TreeNode::leaf(topic.id.to_string(), topic.title)
}

/// Builds the retained accordion used as a compact framework guide.
fn guide_section(theme: Theme) -> View<Action> {
    let mut guide = Accordion::<Action>::new()
        .single()
        .default_open(GUIDE_ENTRIES[0].id)
        .fill_width();
    for entry in GUIDE_ENTRIES {
        guide = guide.item(
            AccordionItem::new(entry.id, entry.question).child(styled_text(
                entry.answer,
                14,
                theme.palette().text_muted,
            )),
        );
    }

    Column::new()
        .fill_width()
        .gap(18.0)
        .child(section_heading(
            theme,
            "GUIDED ANSWERS",
            "The shortest path through the core contracts",
            "These answers mirror the public README and expose the Accordion as useful documentation rather than decorative state.",
        ))
        .child(guide)
        .into_view()
        .key("sandbox-guide-section")
}

/// Builds active and explicitly unavailable documentation resource cards.
fn resources_section(theme: Theme) -> View<Action> {
    Column::new()
        .fill_width()
        .gap(18.0)
        .child(section_heading(
            theme,
            "LEARNING RESOURCES",
            "Start now, then go deeper",
            "Documentation, source, contribution, and voluntary sponsorship use canonical destinations. crates.io and the Book remain visibly unavailable until they exist.",
        ))
        .child(resource_cards(theme, 184.0))
        .into_view()
        .key("sandbox-resources-section")
}

/// Builds two stable three-card resource rows at a caller-selected logical-pixel height.
fn resource_cards(theme: Theme, height: f32) -> View<Action> {
    let mut rows = Column::new().fill_width().gap(14.0);
    for resources in RESOURCES.chunks(3) {
        let mut row = Row::new()
            .fill_width()
            .gap(14.0)
            .align_items(AlignItems::Stretch);
        for resource in resources {
            row = row.child(resource_card(theme, *resource, height));
        }
        rows = rows.child(row);
    }
    rows.into_view()
}

/// Builds one resource card with the exact availability policy from content.rs.
fn resource_card(theme: Theme, resource: Resource, height: f32) -> View<Action> {
    let palette = theme.palette();
    Card::new()
        .variant(match resource.availability {
            ResourceAvailability::Live(_) => CardVariant::Surface,
            ResourceAvailability::ComingSoon => CardVariant::Outline,
        })
        .width(352.0)
        .height(height)
        .padding(12.0)
        .child(
            Column::new()
                .gap(8.0)
                .child(styled_text(resource.title, 17, palette.text))
                .child(styled_text(resource.description, 12, palette.text_muted))
                .child(resource_pill(theme, resource)),
        )
        .into_view()
}

/// Builds a compact enabled or disabled link without a fake fallback URL.
fn resource_pill(theme: Theme, resource: Resource) -> View<Action> {
    let palette = theme.palette();
    let (label, foreground, border) = match resource.availability {
        ResourceAvailability::Live(_) => (resource.title, palette.text, palette.accent),
        ResourceAvailability::ComingSoon => (
            "Coming soon",
            palette.text_muted.with_alpha(0.72),
            palette.border,
        ),
    };
    let mut content = Row::new()
        .gap(7.0)
        .align_items(AlignItems::Center)
        .child(styled_text(label, 12, foreground));
    if matches!(resource.availability, ResourceAvailability::Live(_)) {
        content = content.child(Icon::svg_str(EXTERNAL_LINK_SVG).size(13.0));
    }
    let child = Container::new()
        .background(palette.surface_elevated)
        .border(1.0, border.with_alpha(0.72))
        .radius(999.0)
        .padding(9.0)
        .child(content);

    match resource.availability {
        ResourceAvailability::Live(url) => Link::new().child(child).href(url).into_view(),
        ResourceAvailability::ComingSoon => Link::new().child(child).disabled(true).into_view(),
    }
}

/// Builds a consistent eyebrow, title, and supporting paragraph group.
fn section_heading(
    theme: Theme,
    eyebrow: &'static str,
    title: &'static str,
    description: &'static str,
) -> View<Action> {
    let palette = theme.palette();
    Column::new()
        .fill_width()
        .gap(8.0)
        .child(styled_text(eyebrow, 11, palette.accent))
        .child(styled_text(title, 27, palette.text))
        .child(styled_text(description, 14, palette.text_muted))
        .into_view()
}

/// Builds the truthful consumer-boundary footer.
fn footer(theme: Theme) -> View<Action> {
    let palette = theme.palette();
    Column::new()
        .fill_width()
        .gap(16.0)
        .child(Divider::horizontal().color(palette.border))
        .child(
            Row::new()
                .fill_width()
                .child(styled_text(
                    "Built entirely through the public ailloli_ui façade.",
                    12,
                    palette.text_muted,
                ))
                .child(Container::new().flex_grow())
                .child(styled_text(
                    "Ailloli UI · Rust native UI framework",
                    12,
                    palette.text_muted,
                )),
        )
        .into_view()
}

/// Creates one statically styled UI-font label.
fn styled_text(text: impl Into<String>, size: u16, color: Color) -> Text {
    Text::new(text.into()).style(TextStyle::new(FontId::Ui, size, color))
}

/// Creates one reactively bound UI-font label.
fn styled_bound_text(text: State<String>, size: u16, color: Color) -> Text {
    Text::new(text).style(TextStyle::new(FontId::Ui, size, color))
}

#[cfg(test)]
mod tests {
    //! Structural tests for the public architecture explorer.

    use super::*;
    use crate::content::FRAMEWORK_TOPICS;

    #[test]
    fn explorer_contains_only_canonical_topics() {
        let nodes = framework_tree_nodes();
        assert_eq!(nodes.len(), 3);
        for topic in FRAMEWORK_TOPICS {
            assert_eq!(super::topic(topic.id), topic);
        }
    }
}
