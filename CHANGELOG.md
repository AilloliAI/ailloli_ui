# Changelog

All notable changes to Ailloli UI will be documented in this file. The format
is based on [Keep a Changelog](https://keepachangelog.com/en/1.1.0/), and the
project follows Semantic Versioning for version identifiers while pre-1.0 APIs
remain subject to change.

## Unreleased

No changes have been recorded after the first beta candidate.

## 0.1.0-beta.1 - 2026-08-26

First public beta of Ailloli UI.

### Added

- Retained-mode application façade, targeted build/layout/paint invalidation,
  reactive state, layouts, controls, navigation, overlays, and virtualized
  trees.
- Native `winit` window hosting and the primary `wgpu` renderer.
- Text shaping, editable buffers, code-editor primitives, filesystem providers,
  terminal contracts, DevTools, application storage, icon tooling, packaging,
  and performance regression support.
- Experimental Vulkan rendering and OpenXR host integration.
- Public governance, security, contribution, support, sponsorship, release,
  architecture, migration, and benchmarking documentation.
- Rust 1.88 CI, strict Rustdoc and Clippy gates, RustSec auditing, CodeQL,
  Dependabot, GitHub Pages preparation, and Rust release tooling.

### Changed

- All 22 framework crates are synchronized at `0.1.0-beta.1` and use exact
  first-party registry requirements during the beta series.
- The public sandbox links to canonical project, documentation, contribution,
  and sponsorship destinations while unpublished resources remain disabled.

### Security

- `lru` is pinned to `0.18.2`, the first release containing the fix for
  RUSTSEC-2026-0253.
- Repository and package audits reject credentials, private paths, unreviewed
  workflow actions, and internal development markers.

### Known limitations

- APIs remain pre-1.0 and can change between beta releases.
- Vulkan and OpenXR support is Experimental and may evolve more quickly than
  the desktop façade.
- crates.io packages, the Book, and the example guide become reachable only
  after their respective publication steps complete.
- GitHub Pages must be enabled from the repository workflow before the hosted
  API documentation URL becomes available.

### Compatibility

- Minimum supported Rust version: Rust 1.88.
- Cargo feature names use underscores; see [MIGRATION.md](MIGRATION.md).

[Unreleased]: https://github.com/AilloliAI/ailloli_ui/compare/v0.1.0-beta.1...HEAD
[0.1.0-beta.1]: https://github.com/AilloliAI/ailloli_ui/releases/tag/v0.1.0-beta.1
