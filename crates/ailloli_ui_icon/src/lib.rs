//! Deterministic, UI-independent validation and rasterization of application SVG icons.

use ailloli_ui_core::{AppIcon, SvgSource};
use std::collections::HashMap;
use std::sync::{Arc, Mutex, OnceLock};

pub const MAX_SVG_BYTES: usize = 4 * 1024 * 1024;
pub const MAX_SVG_DIMENSION: f32 = 8192.0;
pub const SQUARE_RELATIVE_EPSILON: f64 = 1.0e-6;

/// A validated source icon.
#[derive(Debug, Clone, PartialEq)]
pub struct ValidatedIcon {
    pub sha256: String,
    pub width: f32,
    pub height: f32,
}

/// RGBA8 raster output.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RasterizedIcon {
    pub width: u32,
    pub height: u32,
    pub rgba: Arc<[u8]>,
}

#[derive(Debug, thiserror::Error)]
pub enum IconError {
    #[error("SVG source exceeds the 4 MiB application icon limit")]
    TooLarge,
    #[error("application icon SVG is not valid UTF-8/XML: {0}")]
    InvalidXml(String),
    #[error("application icon SVG must declare a positive viewBox")]
    MissingViewBox,
    #[error("application icon SVG viewBox must be square (got {0}x{1})")]
    NonSquare(f64, f64),
    #[error("application icon SVG contains forbidden element or external resource `{0}`")]
    ExternalResource(String),
    #[error("application icon SVG dimensions exceed {MAX_SVG_DIMENSION}px")]
    DimensionsTooLarge,
    #[error("application icon SVG could not be parsed: {0}")]
    InvalidSvg(String),
    #[error("application icon SVG rendered fully transparent")]
    FullyTransparent,
    #[error("failed to allocate a {0}px application icon raster")]
    Allocation(u32),
}

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

/// Validates structure, portability, dimensions and visible output.
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

static RASTER_CACHE: OnceLock<Mutex<HashMap<(String, u32), RasterizedIcon>>> = OnceLock::new();

/// Validates and rasterizes an app icon, caching by source digest and size.
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

/// Backward-compatible renderer adapter for arbitrary SVG sources.
pub fn rasterize_svg_source(source: &SvgSource, px: u32) -> Option<Vec<u8>> {
    let tree = usvg::Tree::from_data(source.as_bytes(), &usvg::Options::default()).ok()?;
    rasterize_tree(&tree, px)
        .ok()
        .map(|raster| raster.rgba.to_vec())
}

#[cfg(test)]
mod tests {
    use super::*;

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
