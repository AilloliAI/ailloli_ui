//! Canonical English copy and external-resource policy for the showcase.
//!
//! Keeping visible text separate from layout makes the sandbox auditable as
//! documentation: every claim maps to a public framework capability and every
//! external destination has an explicit availability state.

/// Public GitHub README opened by the primary documentation link.
pub const GITHUB_README_URL: &str = "https://github.com/AilloliAI/ailloli_ui#readme";

/// Future crates.io page for the public façade package.
pub const CRATES_IO_URL: &str = "https://crates.io/crates/ailloli_ui";

/// Meaningful initial value shared by the reactive input and preview.
pub const INITIAL_REACTIVE_HEADLINE: &str = "Build native Rust interfaces";

/// Copyable Rust quick start kept byte-for-byte in sync with the public README.
pub const QUICK_START_RUST: &str = r#"use ailloli_ui::prelude::*;

fn main() -> ailloli_ui::Result<()> {
    let headline = State::new(
        "Build native Rust interfaces".to_string(),
    );
    let preview = headline.clone();

    App::new()
        .window(
            Window::new("main")
                .title("Hello")
                .size(800.0, 600.0)
                .ailloli_ui_chrome()
                .content(move || {
                    Column::new()
                        .padding(16.0)
                        .gap(12.0)
                        .child(TextInput::<()>::new().bind(headline.clone()))
                        .child(Text::new(preview.clone()).size(24.0))
                }),
        )
        .run()
}"#;

/// Availability policy for one resource presented by the showcase.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ResourceAvailability {
    /// The resource has a canonical HTTP(S) destination.
    Live(&'static str),
    /// The resource is intentionally visible but cannot be activated yet.
    ComingSoon,
}

/// One public learning resource rendered in the header and resource section.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Resource {
    /// Stable human-facing title.
    pub title: &'static str,
    /// Short explanation of what the destination will provide.
    pub description: &'static str,
    /// Whether the resource is active or explicitly unavailable.
    pub availability: ResourceAvailability,
}

/// Resources shown in stable display order.
///
/// The first two entries are live and are reused by the compact header. Future
/// resources follow them so rendering can keep unavailable destinations
/// visible without manufacturing placeholder URLs.
pub const RESOURCES: &[Resource] = &[
    Resource {
        title: "GitHub README",
        description: "Start here for setup, architecture, features, and validation gates.",
        availability: ResourceAvailability::Live(GITHUB_README_URL),
    },
    Resource {
        title: "crates.io",
        description: "The public package page for the ailloli_ui façade.",
        availability: ResourceAvailability::Live(CRATES_IO_URL),
    },
    Resource {
        title: "API Documentation",
        description: "Hosted Rust API reference with linked types and examples.",
        availability: ResourceAvailability::ComingSoon,
    },
    Resource {
        title: "The Ailloli UI Book",
        description: "A guided path from first window to advanced native applications.",
        availability: ResourceAvailability::ComingSoon,
    },
];

/// One verified framework capability used by the editorial card grid.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Capability {
    /// Compact category label.
    pub eyebrow: &'static str,
    /// Capability title.
    pub title: &'static str,
    /// Public behavior described without implementation-only promises.
    pub description: &'static str,
}

/// Curated capabilities that collectively present the framework rather than a
/// raw widget inventory.
pub const CAPABILITIES: &[Capability] = &[
    Capability {
        eyebrow: "Retained runtime",
        title: "Targeted work by design",
        description: "Build, layout, and paint are distinct invalidation levels, so stable sibling branches stay reusable.",
    },
    Capability {
        eyebrow: "Native desktop",
        title: "One public application model",
        description: "Typed windows, client chrome, input, clipboard, popups, capture, and lifecycle flow through the façade.",
    },
    Capability {
        eyebrow: "GPU rendering",
        title: "WGPU first, extensible by host",
        description: "The default native path uses WGPU, with optional Vulkan and OpenXR integrations for specialized hosts.",
    },
    Capability {
        eyebrow: "Developer surfaces",
        title: "Editors, files, and terminals",
        description: "Composable widgets cover code editing, provider-neutral filesystems, terminal state, charts, and DevTools.",
    },
];

/// Documentation topic displayed by the real TreeView explorer.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct FrameworkTopic {
    /// Stable TreeView identity.
    pub id: &'static str,
    /// Visible row and detail title.
    pub title: &'static str,
    /// Concise documentation summary shown when selected.
    pub summary: &'static str,
}

/// Real architectural topics represented by the documentation explorer.
///
/// The façade entry is the canonical fallback returned for an unknown topic ID.
pub const FRAMEWORK_TOPICS: &[FrameworkTopic] = &[
    FrameworkTopic {
        id: "foundations",
        title: "Foundations",
        summary: "Core values, retained reconciliation, layout, text shaping, and application storage form the platform-neutral base.",
    },
    FrameworkTopic {
        id: "facade",
        title: "ailloli_ui façade",
        summary: "Applications start from ailloli_ui::prelude and keep direct dependencies focused on one public entry point.",
    },
    FrameworkTopic {
        id: "runtime",
        title: "Retained runtime",
        summary: "Stable identity, reactive dependencies, targeted invalidation, layout, painting, and input routing live in the retained runtime.",
    },
    FrameworkTopic {
        id: "text-editor",
        title: "Text and editor",
        summary: "Text shaping and editable buffers stay backend-neutral, while Editor and CodeEditor expose application-facing composition.",
    },
    FrameworkTopic {
        id: "desktop",
        title: "Desktop UI",
        summary: "Widgets and the native host compose controls, overlays, window lifecycle, accessibility-oriented focus, and capture.",
    },
    FrameworkTopic {
        id: "widgets",
        title: "Widget library",
        summary: "Layouts, controls, navigation, data views, charts, editors, files, and terminals share one retained widget contract.",
    },
    FrameworkTopic {
        id: "winit",
        title: "Native winit host",
        summary: "The winit adapter owns native event translation, surfaces, wake delivery, clipboard, popups, and window capture.",
    },
    FrameworkTopic {
        id: "rendering",
        title: "WGPU and Vulkan",
        summary: "WGPU is the primary renderer; the Vulkan backend provides an explicit lower-level path for specialized integrations.",
    },
    FrameworkTopic {
        id: "systems",
        title: "Application systems",
        summary: "Provider-neutral filesystem, terminal, storage, tooling, and immersive building blocks remain reusable outside product code.",
    },
    FrameworkTopic {
        id: "filesystem",
        title: "Filesystem contracts",
        summary: "Providers deliver owned snapshots and deltas through bounded workers; rendering never performs filesystem I/O.",
    },
    FrameworkTopic {
        id: "terminal",
        title: "Terminal model",
        summary: "Terminal parsing and state are process-independent, while PTY support is explicit and feature-gated.",
    },
    FrameworkTopic {
        id: "openxr",
        title: "OpenXR integration",
        summary: "Optional immersive hosts reuse the same core, runtime, widgets, and renderer contracts without changing desktop consumers.",
    },
];

/// Returns the canonical topic for `id`, falling back to the façade overview.
pub fn topic(id: &str) -> &'static FrameworkTopic {
    FRAMEWORK_TOPICS
        .iter()
        .find(|topic| topic.id == id)
        .unwrap_or(&FRAMEWORK_TOPICS[1])
}

/// One accurate frequently asked question rendered by the interactive guide.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct GuideEntry {
    /// Stable accordion ID.
    pub id: &'static str,
    /// Visible question.
    pub question: &'static str,
    /// Public answer.
    pub answer: &'static str,
}

/// Short guide grounded in the same contracts as the public README.
///
/// This list remains non-empty because its first entry is expanded by default.
pub const GUIDE_ENTRIES: &[GuideEntry] = &[
    GuideEntry {
        id: "entry-point",
        question: "Where should an application start?",
        answer: "Depend on the ailloli_ui façade and import ailloli_ui::prelude::*; lower-level crates remain available for custom hosts and framework extensions.",
    },
    GuideEntry {
        id: "renderer",
        question: "Which renderer powers native windows?",
        answer: "The default winit feature uses the WGPU renderer. Vulkan and OpenXR are optional integrations, not hidden requirements for ordinary desktop apps.",
    },
    GuideEntry {
        id: "performance",
        question: "How does retained work stay bounded?",
        answer: "Targeted invalidation reuses stable branches, virtualized views visit their viewport plus overscan, and filesystem workers deliver owned deltas outside rendering.",
    },
];

#[cfg(test)]
mod tests {
    //! Content-policy tests for destinations, examples, and visible copy.

    use super::*;

    #[test]
    fn live_resources_use_the_reserved_public_destinations() {
        assert_eq!(
            RESOURCES[0].availability,
            ResourceAvailability::Live("https://github.com/AilloliAI/ailloli_ui#readme")
        );
        assert_eq!(
            RESOURCES[1].availability,
            ResourceAvailability::Live("https://crates.io/crates/ailloli_ui")
        );
        assert!(RESOURCES[..2].iter().all(|resource| matches!(
            resource.availability,
            ResourceAvailability::Live(url) if url.starts_with("https://")
        )));
    }

    #[test]
    fn future_documentation_resources_have_no_placeholder_url() {
        assert!(RESOURCES[2..]
            .iter()
            .all(|resource| { matches!(resource.availability, ResourceAvailability::ComingSoon) }));
    }

    #[test]
    fn quick_start_is_the_public_readme_example() {
        let readme = std::fs::read_to_string(
            std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR"))
                .join("../..")
                .join("README.md"),
        )
        .expect("read framework README");
        assert!(readme.contains(QUICK_START_RUST));
    }

    #[test]
    fn visible_documentation_copy_has_no_legacy_fixture_language() {
        let mut visible = Vec::new();
        visible.extend(
            RESOURCES
                .iter()
                .flat_map(|resource| [resource.title, resource.description]),
        );
        visible.extend(
            CAPABILITIES.iter().flat_map(|capability| {
                [capability.eyebrow, capability.title, capability.description]
            }),
        );
        visible.extend(
            FRAMEWORK_TOPICS
                .iter()
                .flat_map(|topic| [topic.title, topic.summary]),
        );
        visible.extend(
            GUIDE_ENTRIES
                .iter()
                .flat_map(|entry| [entry.question, entry.answer]),
        );

        for text in visible {
            let normalized = text.to_ascii_lowercase();
            for forbidden in [
                "example.com",
                "virtual row",
                "development",
                "staging",
                "production",
                "lorem ipsum",
                "phase 12",
            ] {
                assert!(
                    !normalized.contains(forbidden),
                    "visible copy contains forbidden fixture text {forbidden:?}: {text}"
                );
            }
        }
    }
}
