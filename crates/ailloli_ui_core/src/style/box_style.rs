use crate::Rect;

use super::{Background, Border, BoxShadow, Opacity, Radius};

fn union_rect(a: Rect, b: Rect) -> Rect {
    let x0 = a.x.min(b.x);
    let y0 = a.y.min(b.y);
    let x1 = a.right().max(b.right());
    let y1 = a.bottom().max(b.bottom());
    Rect::new(x0, y0, x1 - x0, y1 - y0)
}

/// Visual box decoration: background, border, radius, shadows, opacity.
#[derive(Debug, Clone, PartialEq)]
pub struct BoxStyle {
    pub background: Background,
    pub border: Border,
    pub radius: Radius,
    pub shadows: Vec<BoxShadow>,
    pub opacity: Opacity,
}

impl Default for BoxStyle {
    fn default() -> Self {
        Self {
            background: Background::None,
            border: Border::default(),
            radius: Radius::zero(),
            shadows: Vec::new(),
            opacity: Opacity::default(),
        }
    }
}

impl BoxStyle {
    /// Default transparent box.
    pub fn new() -> Self {
        Self::default()
    }

    pub fn background(mut self, background: Background) -> Self {
        self.background = background;
        self
    }

    pub fn border(mut self, border: Border) -> Self {
        self.border = border;
        self
    }

    pub fn radius(mut self, radius: impl Into<Radius>) -> Self {
        self.radius = radius.into();
        self
    }

    pub fn shadow(mut self, shadow: BoxShadow) -> Self {
        self.shadows.push(shadow);
        self
    }

    pub fn shadows(mut self, shadows: Vec<BoxShadow>) -> Self {
        self.shadows = shadows;
        self
    }

    pub fn clear_shadows(mut self) -> Self {
        self.shadows.clear();
        self
    }

    pub fn visual_bounds(&self, rect: Rect) -> Rect {
        self.shadows.iter().fold(rect, |bounds, shadow| {
            union_rect(bounds, shadow.paint_bounds(rect))
        })
    }

    pub fn opacity(mut self, opacity: Opacity) -> Self {
        self.opacity = opacity;
        self
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::Color;

    #[test]
    fn shadow_builder_appends_and_clear_shadows_removes_all() {
        let style = BoxStyle::new()
            .shadow(BoxShadow::sm())
            .shadow(BoxShadow::glow(Color::WHITE));

        assert_eq!(style.shadows.len(), 2);
        assert!(style.clone().clear_shadows().shadows.is_empty());
    }

    #[test]
    fn shadows_builder_replaces_shadow_layers() {
        let style = BoxStyle::new()
            .shadow(BoxShadow::sm())
            .shadows(vec![BoxShadow::md()]);

        assert_eq!(style.shadows, vec![BoxShadow::md()]);
    }

    #[test]
    fn visual_bounds_include_outer_shadows() {
        let style = BoxStyle::new().shadow(BoxShadow::new(5.0, -2.0, 8.0, 1.0, Color::BLACK));

        assert_eq!(
            style.visual_bounds(Rect::new(10.0, 20.0, 30.0, 10.0)),
            Rect::new(6.0, 9.0, 48.0, 28.0)
        );
    }
}
