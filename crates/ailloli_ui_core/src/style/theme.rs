use crate::FontId;

use super::{BoxShadow, Color, Radius, TextStyle};

/// Semantic color tokens for built-in widgets.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct ThemePalette {
    pub background: Color,
    pub surface: Color,
    pub surface_elevated: Color,
    pub border: Color,
    pub text: Color,
    pub text_muted: Color,
    pub accent: Color,
    pub danger: Color,
    pub success: Color,
    pub warning: Color,
    pub info: Color,
    pub focus: Color,
}

/// Standard radii used by native widgets.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct ThemeRadius {
    pub sm: f32,
    pub md: f32,
    pub lg: f32,
    pub xl: f32,
}

impl ThemeRadius {
    pub const fn button(self) -> Radius {
        Radius::uniform(self.md)
    }

    pub const fn input(self) -> Radius {
        Radius::uniform(self.md)
    }

    pub const fn panel(self) -> Radius {
        Radius::uniform(self.lg)
    }
}

/// Standard spacing scale in logical pixels.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct ThemeSpacing {
    pub xs: f32,
    pub sm: f32,
    pub md: f32,
    pub lg: f32,
    pub xl: f32,
}

/// Standard text styles used by native widgets and showcases.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct ThemeTypography {
    pub ui_sm: TextStyle,
    pub ui_md: TextStyle,
    pub ui_lg: TextStyle,
    pub mono_md: TextStyle,
}

/// Standard shadow presets for native boxes.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct ThemeShadows {
    pub sm: BoxShadow,
    pub md: BoxShadow,
    pub lg: BoxShadow,
}

/// Shared semantic widget states for theme-driven controls.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ThemeState {
    Normal,
    Hovered,
    Pressed,
    Focused,
    Disabled,
    Selected,
    Invalid,
}

/// Semantic color tokens for built-in chrome and controls.
#[derive(Debug, Clone, Copy)]
pub struct Theme {
    /// Client window background.
    pub window_bg: Color,
    /// Title bar background (client chrome).
    pub titlebar_bg: Color,

    pub button_bg: Color,
    pub button_bg_hover: Color,
    pub button_bg_pressed: Color,

    pub close_bg_hover: Color,
    pub close_bg_pressed: Color,

    /// Hover fill for minimize/maximize title-bar controls.
    pub titlebar_control_hover: Color,
    /// Pressed fill for minimize/maximize title-bar controls.
    pub titlebar_control_pressed: Color,

    /// Default icon foreground on chrome.
    pub icon_fg: Color,
}

impl Theme {
    /// Built-in dark theme used by default title bar and window surface.
    pub fn dark() -> Self {
        Self {
            window_bg: Color::rgba(9, 11, 12, 1.0),
            titlebar_bg: Color::rgba(17, 20, 22, 1.0),

            button_bg: Color::rgba(255, 90, 0, 1.0),
            button_bg_hover: Color::rgba(255, 111, 26, 1.0),
            button_bg_pressed: Color::rgba(217, 72, 0, 1.0),

            close_bg_hover: Color::rgba(239, 68, 68, 1.0),
            close_bg_pressed: Color::rgba(185, 28, 28, 1.0),

            titlebar_control_hover: Color::rgba(23, 26, 29, 1.0),
            titlebar_control_pressed: Color::rgba(32, 37, 42, 1.0),

            icon_fg: Color::rgba(244, 247, 248, 1.0),
        }
    }

    pub fn palette(self) -> ThemePalette {
        ThemePalette {
            background: Color::rgba(22, 22, 22, 1.0),
            surface: Color::rgba(17, 20, 22, 0.5),
            surface_elevated: Color::rgba(23, 26, 29, 1.0),
            border: Color::rgba(42, 47, 52, 1.0),
            text: Color::rgba(244, 247, 248, 1.0),
            text_muted: Color::rgba(139, 148, 158, 1.0),
            accent: Color::rgba(255, 90, 0, 1.0),
            danger: Color::rgba(239, 68, 68, 1.0),
            success: Color::rgba(34, 197, 94, 1.0),
            warning: Color::rgba(245, 158, 11, 1.0),
            info: Color::rgba(56, 189, 248, 1.0),
            focus: Color::rgba(255, 138, 61, 1.0),
        }
    }

    pub const fn radius(self) -> ThemeRadius {
        ThemeRadius {
            sm: 4.0,
            md: 8.0,
            lg: 12.0,
            xl: 16.0,
        }
    }

    pub const fn spacing(self) -> ThemeSpacing {
        ThemeSpacing {
            xs: 4.0,
            sm: 8.0,
            md: 12.0,
            lg: 16.0,
            xl: 24.0,
        }
    }

    pub fn typography(self) -> ThemeTypography {
        let palette = self.palette();
        ThemeTypography {
            ui_sm: TextStyle::new(FontId::Ui, 12, palette.text),
            ui_md: TextStyle::new(FontId::Ui, 14, palette.text),
            ui_lg: TextStyle::new(FontId::Ui, 18, palette.text),
            mono_md: TextStyle::new(FontId::Mono, 14, palette.text),
        }
    }

    pub fn shadows(self) -> ThemeShadows {
        ThemeShadows {
            sm: BoxShadow::sm(),
            md: BoxShadow::md(),
            lg: BoxShadow::lg(),
        }
    }
}

impl Default for Theme {
    fn default() -> Self {
        Self::dark()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_theme_uses_dark_orange_bundle_tokens() {
        let theme = Theme::default();
        let palette = theme.palette();

        assert_eq!(theme.window_bg, Color::rgba(9, 11, 12, 1.0));
        assert_eq!(palette.background, Color::rgba(22, 22, 22, 1.0));
        assert_eq!(palette.surface, Color::rgba(17, 20, 22, 0.5));
        assert_eq!(palette.surface_elevated, Color::rgba(23, 26, 29, 1.0));
        assert_eq!(palette.border, Color::rgba(42, 47, 52, 1.0));
        assert_eq!(palette.text, Color::rgba(244, 247, 248, 1.0));
        assert_eq!(palette.text_muted, Color::rgba(139, 148, 158, 1.0));
        assert_eq!(palette.accent, Color::rgba(255, 90, 0, 1.0));
    }

    #[test]
    fn theme_scales_are_stable_for_native_widgets() {
        let theme = Theme::default();

        assert_eq!(theme.radius().button(), Radius::uniform(8.0));
        assert_eq!(theme.radius().panel(), Radius::uniform(12.0));
        assert_eq!(theme.spacing().md, 12.0);
        assert_eq!(theme.typography().ui_md.px_size, 14);
        assert_eq!(theme.typography().mono_md.font, FontId::Mono);
        assert_eq!(theme.shadows().md.blur_radius, BoxShadow::md().blur_radius);
    }
}
