# Changelog

All notable changes to Ailloli UI will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.1.0/).
The project uses Semantic Versioning for release identifiers, while pre-1.0
APIs remain subject to change.

## [Unreleased]

### Added

- Reactive dependencies are now tracked precisely across Build, Layout, and
  Paint, reducing unnecessary work and ensuring affected windows and popups
  update correctly.
- Layout results, cache entries, and reactive dependencies are now committed
  atomically, preventing paint operations from using stale geometry.
- Shared scrollbar geometry and interaction primitives are now used by
  `ScrollView`, editors, text inputs, tables, terminals, and popup lists.

### Changed

- Scrolling now handles line and pixel wheel deltas consistently, supports
  Shift-based horizontal scrolling, centers the thumb on track clicks, and
  preserves pointer capture during drag.
- Caret reveal and other geometry-dependent effects are now applied only after
  an authoritative layout pass.

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
- Added the public framework banner, release badges, and canonical GitHub
  Sponsors funding metadata.
- Repository and release audits now require the canonical `AilloliAI` funding
  beneficiary.
- Release validation now enforces bracketed Keep a Changelog headings and the
  canonical comparison and release links.
- Public documentation, Rustdoc, UI labels, fixtures, and reports now follow
  the project's contextual punctuation policy. En dashes remain valid for
  ranges, while verbatim third-party legal text is preserved where required.

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

[Unreleased]: https://github.com/AilloliAI/ailloli_ui/compare/v0.1.0-beta.1...HEAD
[0.1.0-beta.1]: https://github.com/AilloliAI/ailloli_ui/releases/tag/v0.1.0-beta.1
