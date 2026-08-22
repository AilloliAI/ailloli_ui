//! Deterministic, UI-independent validation and rasterization of application SVG icons.
//!
//! Application icons have stricter size, square-shape, external-resource, and
//! visible-alpha requirements than the legacy arbitrary-SVG adapter. Successful
//! rasters are square premultiplied RGBA8 buffers. The process performs no network
//! or filesystem I/O through this crate.
//!
//! # Examples
//!
//! ```
//! use ailloli_ui_core::AppIcon;
//! use ailloli_ui_icon::{rasterize_app_icon, validate_app_icon};
//! const SVG: &[u8] = br#"<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 10 10"><rect width="10" height="10"/></svg>"#;
//! let icon = AppIcon::from_static_svg(SVG, "assets/icon.svg");
//! assert_eq!(validate_app_icon(&icon).unwrap().width, 10.0);
//! assert_eq!(rasterize_app_icon(&icon, 32).unwrap().rgba.len(), 32 * 32 * 4);
//! ```

use ailloli_ui_core::{AppIcon, SvgSource};
use std::collections::HashMap;
use std::sync::{Arc, Mutex, OnceLock};

/// Maximum accepted application-icon source length: 4 MiB (4,194,304 bytes).
///
/// The boundary itself is accepted; only larger sources return
/// [`IconError::TooLarge`].
///
/// # Examples
///
/// ```
/// use ailloli_ui_icon::MAX_SVG_BYTES;
/// assert_eq!(MAX_SVG_BYTES, 4_194_304);
/// ```
pub const MAX_SVG_BYTES: usize = 4 * 1024 * 1024;
/// Maximum accepted SVG view-box and parsed-tree dimension, in SVG/CSS pixels.
///
/// Exactly 8,192 is accepted; larger width or height is rejected.
///
/// # Examples
///
/// ```
/// use ailloli_ui_icon::MAX_SVG_DIMENSION;
/// assert_eq!(MAX_SVG_DIMENSION, 8_192.0);
/// ```
pub const MAX_SVG_DIMENSION: f32 = 8192.0;
/// Maximum relative width/height delta treated as numerically square.
///
/// Validation computes `abs(width - height) / max(width, height)` in `f64` and
/// rejects only values strictly greater than `1e-6`.
///
/// # Examples
///
/// ```
/// use ailloli_ui_icon::SQUARE_RELATIVE_EPSILON;
/// assert_eq!(SQUARE_RELATIVE_EPSILON, 0.000_001);
/// ```
pub const SQUARE_RELATIVE_EPSILON: f64 = 1.0e-6;

/// Metadata produced after source, geometry, parse, and visible-alpha validation.
///
/// `width` and `height` are the parsed [`usvg`] tree size in SVG/CSS pixels,
/// which may differ from raw view-box values due to root sizing rules. Public
/// construction permits arbitrary strings, infinities, and NaNs; only values
/// returned by [`validate_app_icon`] carry the validation guarantee.
///
/// # Examples
///
/// ```
/// use ailloli_ui_core::AppIcon;
/// use ailloli_ui_icon::validate_app_icon;
/// const SVG: &[u8] = br#"<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 2 2"><path d="M0 0h2v2H0z"/></svg>"#;
/// let validated = validate_app_icon(&AppIcon::from_static_svg(SVG, "icon.svg")).unwrap();
/// assert_eq!((validated.width, validated.height), (2.0, 2.0));
/// assert_eq!(validated.sha256.len(), 64);
/// ```
#[derive(Debug, Clone, PartialEq)]
pub struct ValidatedIcon {
    /// Lowercase 64-character SHA-256 digest of the exact source bytes.
    pub sha256: String,
    /// Parsed tree width in SVG/CSS pixels.
    pub width: f32,
    /// Parsed tree height in SVG/CSS pixels.
    pub height: f32,
}

/// Square premultiplied RGBA8 raster output.
///
/// Pixels are row-major, four bytes per pixel in red, green, blue, alpha order.
/// Constructor-produced length is `width * height * 4`, and both dimensions are
/// equal and at least one. Public fields can bypass all of those invariants.
/// Cloning shares the allocation through [`Arc`].
///
/// # Examples
///
/// ```
/// use ailloli_ui_core::AppIcon;
/// use ailloli_ui_icon::rasterize_app_icon;
/// const SVG: &[u8] = br#"<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 1 1"><rect width="1" height="1" fill="red"/></svg>"#;
/// let raster = rasterize_app_icon(&AppIcon::from_static_svg(SVG, "icon.svg"), 8).unwrap();
/// assert_eq!((raster.width, raster.height, raster.rgba.len()), (8, 8, 256));
/// ```
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RasterizedIcon {
    /// Raster width in pixels; constructor-produced values are `px.max(1)`.
    pub width: u32,
    /// Raster height in pixels, equal to [`Self::width`] for produced values.
    pub height: u32,
    /// Shared row-major premultiplied RGBA8 bytes.
    pub rgba: Arc<[u8]>,
}

/// Application-icon validation and raster allocation failures.
///
/// XML/SVG/backend messages and rejected resource strings are included verbatim
/// and can vary with dependency versions. Match variants for control flow.
///
/// # Examples
///
/// ```
/// use ailloli_ui_icon::IconError;
/// assert_eq!(IconError::MissingViewBox.to_string(), "application icon SVG must declare a positive viewBox");
/// ```
#[derive(Debug, thiserror::Error)]
pub enum IconError {
    /// Source length is strictly greater than [`MAX_SVG_BYTES`].
    #[error("SVG source exceeds the 4 MiB application icon limit")]
    TooLarge,
    /// Source is not UTF-8, is malformed XML, or its root is not `<svg>`.
    #[error("application icon SVG is not valid UTF-8/XML: {0}")]
    InvalidXml(String),
    /// View box is absent/malformed, not four numbers, or has nonpositive size.
    #[error("application icon SVG must declare a positive viewBox")]
    MissingViewBox,
    /// Relative view-box width/height delta exceeds square tolerance.
    #[error("application icon SVG viewBox must be square (got {0}x{1})")]
    NonSquare(f64, f64),
    /// A forbidden element, reference, or recognized external CSS value was found.
    #[error("application icon SVG contains forbidden element or external resource `{0}`")]
    ExternalResource(String),
    /// Raw view box or parsed tree width/height exceeds 8,192 pixels.
    #[error("application icon SVG dimensions exceed {MAX_SVG_DIMENSION}px")]
    DimensionsTooLarge,
    /// [`usvg`] rejected otherwise structurally prevalidated bytes.
    #[error("application icon SVG could not be parsed: {0}")]
    InvalidSvg(String),
    /// A 64-by-64 validation preview contained no pixel with nonzero alpha.
    #[error("application icon SVG rendered fully transparent")]
    FullyTransparent,
    /// Square RGBA pixmap allocation failed for the normalized side length.
    #[error("failed to allocate a {0}px application icon raster")]
    Allocation(u32),
}

/// Parses positive view-box dimensions and rejects recognized active resources.
///
/// XML element matching is exact (`script`, `image`, `foreignObject`). Nonempty
/// `href`/`xlink:href` values must start with `#`. Every attribute is also scanned
/// case-insensitively for exact substrings `url(http:`, `url(https:`, `url(file:`,
/// or `@import`. This is a deliberately specific syntactic screen, followed by
/// `usvg` parsing; it is not a general sanitizer for arbitrary SVG consumers.
fn parse_view_box(bytes: &[u8]) -> Result<(f64, f64), IconError> {
    let text = std::str::from_utf8(bytes).map_err(|err| IconError::InvalidXml(err.to_string()))?;
    let doc =
        roxmltree::Document::parse(text).map_err(|err| IconError::InvalidXml(err.to_string()))?;
    let root = doc.root_element();
    if root.tag_name().name() != "svg" {
        return Err(IconError::InvalidXml(
            "root element is not <svg>".to_string(),
        ));
    }
    for node in doc.descendants().filter(|node| node.is_element()) {
        let name = node.tag_name().name();
        if matches!(name, "script" | "image" | "foreignObject") {
            return Err(IconError::ExternalResource(name.to_string()));
        }
        for attribute in node.attributes() {
            let value = attribute.value().trim();
            let lower = value.to_ascii_lowercase();
            if matches!(attribute.name(), "href" | "xlink:href")
                && !value.is_empty()
                && !value.starts_with('#')
            {
                return Err(IconError::ExternalResource(value.to_string()));
            }
            if lower.contains("url(http:")
                || lower.contains("url(https:")
                || lower.contains("url(file:")
                || lower.contains("@import")
            {
                return Err(IconError::ExternalResource(value.to_string()));
            }
        }
    }
    let values: Vec<f64> = root
        .attribute("viewBox")
        .or_else(|| root.attribute("viewbox"))
        .ok_or(IconError::MissingViewBox)?
        .split(|ch: char| ch.is_ascii_whitespace() || ch == ',')
        .filter(|value| !value.is_empty())
        .map(|value| value.parse::<f64>().map_err(|_| IconError::MissingViewBox))
        .collect::<Result<_, _>>()?;
    if values.len() != 4 || values[2] <= 0.0 || values[3] <= 0.0 {
        return Err(IconError::MissingViewBox);
    }
    Ok((values[2], values[3]))
}

/// Validates source size, XML/resource structure, square geometry, and visibility.
///
/// Validation accepts at most 4 MiB, requires an exact `viewBox` or `viewbox`
/// attribute with four comma/ASCII-whitespace-separated `f64` values and positive
/// width/height, enforces relative squareness and both raw/parsed 8,192-pixel
/// limits, parses through [`usvg`], then requires nonzero alpha in a 64-pixel
/// preview. View-box origins may be any parseable numbers. The returned digest
/// covers exact source bytes; the `AppIcon` source-path label is irrelevant.
///
/// # Errors
///
/// Returns the corresponding [`IconError`] at the first failed stage, including
/// preview allocation. Error strings can embed parser text or rejected attributes.
///
/// # Examples
///
/// ```
/// use ailloli_ui_core::AppIcon;
/// use ailloli_ui_icon::{validate_app_icon, IconError};
/// const GOOD: &[u8] = br#"<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 4 4"><circle cx="2" cy="2" r="2"/></svg>"#;
/// let validated = validate_app_icon(&AppIcon::from_static_svg(GOOD, "good.svg")).unwrap();
/// assert_eq!((validated.width, validated.height), (4.0, 4.0));
/// const BAD: &[u8] = br#"<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 4 2"><rect width="4" height="2"/></svg>"#;
/// assert!(matches!(
///     validate_app_icon(&AppIcon::from_static_svg(BAD, "bad.svg")),
///     Err(IconError::NonSquare(width, height)) if width == 4.0 && height == 2.0
/// ));
/// ```
pub fn validate_app_icon(icon: &AppIcon) -> Result<ValidatedIcon, IconError> {
    if icon.bytes().len() > MAX_SVG_BYTES {
        return Err(IconError::TooLarge);
    }
    let (view_width, view_height) = parse_view_box(icon.bytes())?;
    let relative_delta = (view_width - view_height).abs() / view_width.max(view_height);
    if relative_delta > SQUARE_RELATIVE_EPSILON {
        return Err(IconError::NonSquare(view_width, view_height));
    }
    if view_width > MAX_SVG_DIMENSION as f64 || view_height > MAX_SVG_DIMENSION as f64 {
        return Err(IconError::DimensionsTooLarge);
    }
    let tree = usvg::Tree::from_data(icon.bytes(), &usvg::Options::default())
        .map_err(|err| IconError::InvalidSvg(err.to_string()))?;
    let size = tree.size();
    if size.width() > MAX_SVG_DIMENSION || size.height() > MAX_SVG_DIMENSION {
        return Err(IconError::DimensionsTooLarge);
    }
    let preview = rasterize_tree(&tree, 64)?;
    if !preview.rgba.chunks_exact(4).any(|pixel| pixel[3] != 0) {
        return Err(IconError::FullyTransparent);
    }
    Ok(ValidatedIcon {
        sha256: icon.sha256(),
        width: size.width(),
        height: size.height(),
    })
}

/// Uniformly scales and centers a parsed tree into a transparent square pixmap.
///
/// `px` clamps to one. Aspect ratio is preserved and unused space stays
/// transparent. Output uses tiny-skia's premultiplied RGBA byte order. This
/// helper does not check source policy, squareness, or visible alpha.
fn rasterize_tree(tree: &usvg::Tree, px: u32) -> Result<RasterizedIcon, IconError> {
    let px = px.max(1);
    let mut pixmap = tiny_skia::Pixmap::new(px, px).ok_or(IconError::Allocation(px))?;
    let size = tree.size();
    let scale = px as f32 / size.width().max(size.height());
    let offset_x = (px as f32 - size.width() * scale) * 0.5;
    let offset_y = (px as f32 - size.height() * scale) * 0.5;
    let transform =
        tiny_skia::Transform::from_translate(offset_x, offset_y).post_scale(scale, scale);
    resvg::render(tree, transform, &mut pixmap.as_mut());
    Ok(RasterizedIcon {
        width: px,
        height: px,
        rgba: Arc::from(pixmap.data().to_vec()),
    })
}

/// Process-global, lazily initialized, unbounded digest-and-size raster cache.
static RASTER_CACHE: OnceLock<Mutex<HashMap<(String, u32), RasterizedIcon>>> = OnceLock::new();

/// Validates and rasterizes an app icon, caching by source digest and side length.
///
/// `px` clamps to one for both key and output. Validation (including a 64-pixel
/// preview) runs before every cache lookup. On a miss, source bytes are parsed a
/// second time for final rendering. Sequential cache hits clone the
/// [`RasterizedIcon`] and share its pixel [`Arc`]; concurrent misses can render
/// redundantly and return different allocations before one cached value wins.
/// The process-global cache has no eviction or byte/count bound.
///
/// # Errors
///
/// Returns any [`validate_app_icon`] error, a second-stage [`IconError::InvalidSvg`],
/// or [`IconError::Allocation`] for the requested normalized raster size.
///
/// # Panics
///
/// Panics with `"app icon cache poisoned"` if another thread poisons the global
/// cache mutex. Allocation failures may also abort rather than return depending
/// on the allocator.
///
/// # Examples
///
/// ```
/// use std::sync::Arc;
/// use ailloli_ui_core::AppIcon;
/// use ailloli_ui_icon::rasterize_app_icon;
/// const SVG: &[u8] = br#"<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 1 1"><rect width="1" height="1"/></svg>"#;
/// let icon = AppIcon::from_static_svg(SVG, "icon.svg");
/// let first = rasterize_app_icon(&icon, 0).unwrap();
/// let second = rasterize_app_icon(&icon, 1).unwrap();
/// assert_eq!((first.width, first.height), (1, 1));
/// assert!(Arc::ptr_eq(&first.rgba, &second.rgba));
/// ```
pub fn rasterize_app_icon(icon: &AppIcon, px: u32) -> Result<RasterizedIcon, IconError> {
    let validated = validate_app_icon(icon)?;
    let key = (validated.sha256, px.max(1));
    let cache = RASTER_CACHE.get_or_init(|| Mutex::new(HashMap::new()));
    if let Some(raster) = cache
        .lock()
        .expect("app icon cache poisoned")
        .get(&key)
        .cloned()
    {
        return Ok(raster);
    }
    let tree = usvg::Tree::from_data(icon.bytes(), &usvg::Options::default())
        .map_err(|err| IconError::InvalidSvg(err.to_string()))?;
    let raster = rasterize_tree(&tree, key.1)?;
    cache
        .lock()
        .expect("app icon cache poisoned")
        .insert(key, raster.clone());
    Ok(raster)
}

/// Rasterizes an arbitrary legacy SVG source without application-icon validation.
///
/// This adapter bypasses source-byte, resource-syntax, square-shape, dimension,
/// visible-alpha, and cache checks. It uses default [`usvg`] parsing and returns
/// a newly allocated, square, premultiplied RGBA8 vector; `px == 0` becomes one.
/// Invalid SVG or raster allocation failure returns `None` with no diagnostic.
/// Consumers requiring the application-icon security/portability policy must use
/// [`rasterize_app_icon`] instead.
///
/// # Examples
///
/// ```
/// use ailloli_ui_core::SvgSource;
/// use ailloli_ui_icon::rasterize_svg_source;
/// let source = SvgSource::Static(br#"<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 2 1"><rect width="2" height="1"/></svg>"#);
/// assert_eq!(rasterize_svg_source(&source, 3).unwrap().len(), 3 * 3 * 4);
/// assert!(rasterize_svg_source(&SvgSource::Static(b"not svg"), 3).is_none());
/// ```
pub fn rasterize_svg_source(source: &SvgSource, px: u32) -> Option<Vec<u8>> {
    let tree = usvg::Tree::from_data(source.as_bytes(), &usvg::Options::default()).ok()?;
    rasterize_tree(&tree, px)
        .ok()
        .map(|raster| raster.rgba.to_vec())
}

#[cfg(test)]
/// Unit tests for validation, tolerance, security filtering, visibility, and cache sharing.
mod tests {
    use super::*;

    /// Wraps static fixture bytes with the conventional test source-path label.
    fn icon(svg: &'static [u8]) -> AppIcon {
        AppIcon::from_static_svg(svg, "src/assets/icons/icon.svg")
    }

    #[test]
    fn validates_and_upscales_visible_square_svg() {
        let source = icon(br##"<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 10 10"><rect width="10" height="10" fill="#ff0000"/></svg>"##);
        let validated = validate_app_icon(&source).unwrap();
        assert_eq!((validated.width, validated.height), (10.0, 10.0));
        let raster = rasterize_app_icon(&source, 256).unwrap();
        assert_eq!(raster.rgba.len(), 256 * 256 * 4);
        assert!(raster
            .rgba
            .chunks_exact(4)
            .any(|pixel| pixel[0] > 200 && pixel[3] > 0));
    }

    #[test]
    fn permits_only_numerical_square_tolerance() {
        let source = icon(br#"<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 79.375001 79.374999"><rect width="100%" height="100%"/></svg>"#);
        validate_app_icon(&source).unwrap();
        let source = icon(br#"<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 100 99"><rect width="100" height="99"/></svg>"#);
        assert!(matches!(
            validate_app_icon(&source),
            Err(IconError::NonSquare(..))
        ));
    }

    #[test]
    fn rejects_active_external_and_transparent_sources() {
        for svg in [
            br#"<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 10 10"><script/></svg>"#.as_slice(),
            br#"<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 10 10"><image href="https://example.test/a.png"/></svg>"#.as_slice(),
            br#"<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 10 10"/>"#.as_slice(),
        ] {
            assert!(validate_app_icon(&icon(svg)).is_err());
        }
    }

    #[test]
    fn raster_cache_reuses_the_same_pixel_allocation() {
        let source = icon(br#"<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 10 10"><rect width="10" height="10"/></svg>"#);
        let first = rasterize_app_icon(&source, 32).unwrap();
        let second = rasterize_app_icon(&source, 32).unwrap();
        assert!(Arc::ptr_eq(&first.rgba, &second.rgba));
    }
}
