//! Native presentation and documentation showcase for the Ailloli UI framework.
//!
//! The application intentionally depends only on the public [`ailloli_ui`]
//! façade. It presents real framework contracts, a copyable quick start,
//! reactive state, a documentation explorer, and canonical learning resources;
//! it does not define compatibility helpers or framework mechanisms.

mod content;
mod view;

#[cfg(test)]
mod visual_tests;

/// Opens the framework showcase and blocks in the native event loop.
///
/// Window dimensions and all layout values are logical pixels. Application
/// identity and the conventional SVG icon are inherited by the single window.
///
/// # Errors
///
/// Propagates identity/icon validation, native window or renderer creation,
/// event-loop, capture, persistence, and benchmark finalization failures from
/// [`ailloli_ui::AppBuilder::run`].
fn main() -> ailloli_ui::Result<()> {
    view::showcase::run()
}
