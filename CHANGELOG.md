# Changelog

All notable changes to Ailloli UI will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.1.0/).
The project uses Semantic Versioning for release identifiers, while pre-1.0
APIs remain subject to change.

## [Unreleased]

## [0.1.0-beta.2] - 2026-09-03

Second public beta of Ailloli UI, focused on retained consistency, interactive
scrolling, and repeatable release validation.

### Added

- Public scrollbar contracts now expose `ScrollbarAxis`, `ScrollbarPart`,
  `ScrollbarGeometrySpec`, `ScrollbarGeometry`, and `ScrollbarDrag` through the
  façade and prelude. `ScrollBehavior::wheel_delta_with_modifiers` and
  `code_scrollbar_geometries` support custom integrations.
- `Dialog::modal_content` mounts retained declarative content as a
  `PopupRole::Dialog` modal, and `Dialog::on_submit` provides an Enter fallback
  after descendants receive the event.
- `Editor::caret_follow_margin_lines` and
  `CodeEditor::caret_follow_margin_lines` configure the visible safety margin
  maintained around a revealed caret.
- `Component` and the `component` helper now accept capturing render closures.
  `LayoutPass` is public for custom widgets that must distinguish measurement
  from authoritative layout.
- `InputRouter::cancel_pointer_state` lets host adapters deliver cancellation
  to active capture owners before clearing retained pointer state.

### Changed

- Values created with `State::new` establish precise reactive dependencies when
  retained Build, Layout, or Paint callbacks read them. Successful callbacks
  replace conditional dependencies atomically, while failed or provisional
  work cannot publish a partial dependency set.
- Layout results, cache entries, geometry-derived widget state, and reactive
  dependencies are committed as one transaction. Paint consumes only artifacts
  from the matching committed layout.
- Scrolling now handles line and pixel wheel deltas consistently, supports
  Shift-based horizontal scrolling, centers the thumb on track clicks, and
  shares geometry and interaction rules across `ScrollView`, editors, text
  inputs, tables, terminals, and popup lists.
- Caret reveal and other geometry-dependent effects are now applied only after
  an authoritative layout pass.
- Reactive work is aggregated per native presentation, so every affected
  window or popup is woken without scheduling unrelated presentations.
- `Context::register_ui_service` now requests `Build` only for its current
  mounted owner when a callback reports a change. Low-level
  `RuntimeHandle::register_ui_service` registrations no longer infer a
  presentation or redraw, so those callbacks must explicitly request the
  required Build, Layout, or Paint invalidation.
- Reusing a `StateStore::signal`, `StateStore::signal_scoped`, or
  `StateStore::signal_scoped_with` slot now preserves the invalidator installed
  when the slot was first created. Later handles share that source and do not
  replace its invalidator until the slot is removed.

### Fixed

- Reactive state changes can no longer be hidden by a clean parent layout
  cache, and updated text is no longer painted into stale bounds.
- Stable sibling components are no longer rebuilt or laid out again when an
  unrelated reactive consumer changes.
- Scrollbar drags now remain stable across retained relayouts, popups, and
  native host event routing.
- The editor no longer consumes a pending caret reveal during a provisional
  measurement pass.
- Windows now request their first drawable frame after becoming visible,
  preventing them from remaining blank until another input or resize event.
- Replacing a retained widget or component with a different concrete type at
  the same position or key now starts a fresh mount and clears the previous
  state slots and reactive dependencies.

### Security

- The four informational RustSec exceptions remain limited to their reviewed
  dependency chains after the 2026-09-02 lockfile review. Vulnerabilities, new
  warnings, and unreviewed advisory IDs still fail the release gate.
- `lru` remains pinned to `0.18.2`, so the resolved fix for RUSTSEC-2026-0253
  cannot regress silently.

### Project

- Public CI now uses context-aware routing, explicit Windows validation,
  release workflow validation, and deterministic policy fixtures.
- Superseded CI runs are cancelled automatically, and exhaustive Rust jobs cap
  Cargo compilation at four workers to prevent linker memory exhaustion.
- Package validation now works correctly on fresh CI runners without
  pre-existing build artifacts.
- GitHub Pages now publishes library documentation only.
- The public sandbox now links to the published `ailloli_ui` crate on
  crates.io while keeping unavailable documentation destinations visibly
  disabled.
- The sandbox identifies the beta.2 candidate and links to these changelog
  notes without claiming that an unverified registry version or GitHub Release
  is already available.
- Added the public framework banner, release badges, and canonical GitHub
  Sponsors funding metadata.
- Repository and release audits now require the canonical `AilloliAI` funding
  beneficiary.
- Release validation now enforces bracketed Keep a Changelog headings and the
  canonical comparison and release links.
- Public documentation, Rustdoc, UI labels, fixtures, and reports now follow
  the project's contextual punctuation policy. En dashes remain valid for
  ranges, while verbatim third-party legal text is preserved where required.

### Compatibility

- The audited delta removes or renames no documented beta.1 façade item. Most
  façade consumers require no source migration; update the Cargo requirement
  to `0.1.0-beta.2` to select this release.
- Low-level users of `RuntimeHandle::register_ui_service` must explicitly
  request the required invalidation when a service reports changed state.
  `Context::register_ui_service` performs the owner-scoped `Build` request
  automatically.
- The minimum supported Rust version remains Rust 1.88.

### Known Limitations

- APIs remain pre-1.0 and may change between beta releases.
- Vulkan and OpenXR support remain experimental. Public CI validates Linux and
  Windows, while macOS and OpenXR hardware are not validated for this beta.

## [0.1.0-beta.1] - 2026-08-26

First public beta of Ailloli UI.

### Added

- Retained-mode application model with targeted Build, Layout, and Paint
  invalidation and reactive state tracking.
- Core layouts, controls, navigation, overlays, and virtualized tree
  components.
- Native `winit` window hosting and the primary `wgpu` renderer.
- Text shaping, editable buffers, and code-editor primitives.
- Filesystem providers, terminal contracts, DevTools, application storage,
  and icon tooling.
- Packaging utilities and performance regression infrastructure.
- Experimental Vulkan rendering and OpenXR host integration.
- Public documentation for governance, security, contribution, support,
  sponsorship, releases, architecture, migration, and benchmarking.

### Changed

- All 22 framework crates are synchronized at `0.1.0-beta.1` and use exact
  first-party registry requirements throughout the beta series.
- The public sandbox links to canonical project, documentation, contribution,
  and sponsorship destinations while unpublished resources remain disabled.

### Security

- `lru` is pinned to `0.18.2`, the first release containing the fix for
  RUSTSEC-2026-0253.
- Repository and package audits reject credentials, private paths, unreviewed
  workflow actions, and internal development markers.

### Project

- Added Rust 1.88 CI with strict Rustdoc and Clippy gates.
- Added RustSec auditing, CodeQL, Dependabot, GitHub Pages preparation, and
  Rust release tooling.

### Known Limitations

- APIs remain pre-1.0 and may change between beta releases.
- Vulkan and OpenXR support remain experimental and may evolve more quickly
  than the primary desktop stack.

### Compatibility

- Minimum supported Rust version: Rust 1.88.
- Cargo feature names use underscores. See [MIGRATION.md](MIGRATION.md).

[Unreleased]: https://github.com/AilloliAI/ailloli_ui/compare/v0.1.0-beta.2...HEAD
[0.1.0-beta.2]: https://github.com/AilloliAI/ailloli_ui/releases/tag/v0.1.0-beta.2
[0.1.0-beta.1]: https://github.com/AilloliAI/ailloli_ui/releases/tag/v0.1.0-beta.1
