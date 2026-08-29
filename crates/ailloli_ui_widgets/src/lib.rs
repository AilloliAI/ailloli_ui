//! Generic UI widget catalogue for Ailloli UI.
//!
//! This crate implements the retained [`ailloli_ui_runtime::Widget`] trait for
//! layout, controls, text, chrome, and overlays. It contains **no app-specific**
//! screens (chat, IDE shells, etc.) — those belong in the application crate.
//!
//! # Modules
//!
//! | Module | Widgets |
//! |--------|---------|
//! | [`layout`] | `Row`, `Column`, `Container`, `ScrollView`, `Align`, margin/padding |
//! | [`controls`] | `Button`, `TextInput`, `Checkbox`, `Tabs`, lists |
//! | [`text`] | `Text`, `RichText`, editable line helpers |
//! | [`primitives`] | `Icon`, rects, spacers |
//! | [`editor`] | Multi-paragraph `Editor` with virtual scroll |
//! | [`chrome`] | Default title bar, resize/drag hit regions |
//! | [`overlay`] | Modals, tooltips, base+overlay scene helpers |

/// Window chrome widgets (title bar, resize, drag regions).
pub mod chrome;
/// Interactive controls (button, text field, tabs, …).
pub mod controls;
/// Code editor view with rope buffer and IME.
pub mod editor;
/// File-oriented widgets built from VFS snapshots.
#[cfg(feature = "files")]
pub mod files;
/// Flex layout and box model wrappers.
pub mod layout;
/// Modal, tooltip, and layered scene helpers.
pub mod overlay;
/// Low-level draw helpers (icon, rect, spacer).
pub mod primitives;
mod scrollbar;
/// Static and rich text widgets.
pub mod text;
