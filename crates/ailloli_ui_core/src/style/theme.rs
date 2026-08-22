//! Built-in dark/orange semantic colors, spacing, typography, radius, and shadows.

use crate::FontId;

use super::{BoxShadow, Color, Radius, TextStyle};

/// Semantic color tokens for built-in widgets.
///
/// # Examples
///
/// ```
/// use ailloli_ui_core::style::Theme;
/// let palette = Theme::dark().palette();
/// assert_ne!(palette.text, palette.background);
/// ```
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct ThemePalette {
    /// Base canvas behind ordinary content.
    pub background: Color,
    /// Translucent or low-elevation control/panel surface.
    pub surface: Color,
    /// Opaque elevated overlay or panel surface.
    pub surface_elevated: Color,
    /// Standard separator and outline color.
    pub border: Color,
    /// Primary readable text color.
    pub text: Color,
    /// Secondary or de-emphasized text color.
    pub text_muted: Color,
    /// Primary brand/action color.
    pub accent: Color,
    /// Destructive action and error color.
    pub danger: Color,
    /// Successful state color.
    pub success: Color,
    /// Warning state color.
    pub warning: Color,
    /// Informational state color.
    pub info: Color,
    /// Keyboard-focus indicator color.
    pub focus: Color,
}

/// Standard radii used by native widgets.
///
/// # Examples
///
/// ```
/// use ailloli_ui_core::style::Theme;
/// assert_eq!(Theme::dark().radius().md, 8.0);
/// ```
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct ThemeRadius {
    /// Small radius in logical pixels.
    pub sm: f32,
    /// Medium radius in logical pixels.
    pub md: f32,
    /// Large radius in logical pixels.
    pub lg: f32,
    /// Extra-large radius in logical pixels.
    pub xl: f32,
}

impl ThemeRadius {
    /// Returns the uniform medium radius used by buttons.
    ///
    /// # Examples
    ///
    /// ```
    /// use ailloli_ui_core::style::{Radius, Theme};
    /// assert_eq!(Theme::dark().radius().button(), Radius::uniform(8.0));
    /// ```
    pub const fn button(self) -> Radius {
        Radius::uniform(self.md)
    }

    /// Returns the uniform medium radius used by input controls.
    ///
    /// # Examples
    ///
    /// ```
    /// use ailloli_ui_core::style::{Radius, Theme};
    /// assert_eq!(Theme::dark().radius().input(), Radius::uniform(8.0));
    /// ```
    pub const fn input(self) -> Radius {
        Radius::uniform(self.md)
    }

    /// Returns the uniform large radius used by panels.
    ///
    /// # Examples
    ///
    /// ```
    /// use ailloli_ui_core::style::{Radius, Theme};
    /// assert_eq!(Theme::dark().radius().panel(), Radius::uniform(12.0));
    /// ```
    pub const fn panel(self) -> Radius {
        Radius::uniform(self.lg)
    }
}

/// Standard spacing scale in logical pixels.
///
/// # Examples
///
/// ```
/// use ailloli_ui_core::style::Theme;
/// assert_eq!(Theme::dark().spacing().xl, 24.0);
/// ```
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct ThemeSpacing {
    /// Extra-small spacing in logical pixels.
    pub xs: f32,
    /// Small spacing in logical pixels.
    pub sm: f32,
    /// Medium spacing in logical pixels.
    pub md: f32,
    /// Large spacing in logical pixels.
    pub lg: f32,
    /// Extra-large spacing in logical pixels.
    pub xl: f32,
}

/// Standard text styles used by native widgets and showcases.
///
/// # Examples
///
/// ```
/// use ailloli_ui_core::style::Theme;
/// assert_eq!(Theme::dark().typography().ui_md.px_size, 14);
/// ```
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct ThemeTypography {
    /// Small UI sans-serif text.
    pub ui_sm: TextStyle,
    /// Default UI sans-serif text.
    pub ui_md: TextStyle,
    /// Large UI sans-serif text.
    pub ui_lg: TextStyle,
    /// Default monospace text for code and terminals.
    pub mono_md: TextStyle,
}

/// Standard shadow presets for native boxes.
///
/// # Examples
///
/// ```
/// use ailloli_ui_core::style::Theme;
/// assert_eq!(Theme::dark().shadows().md.blur_radius, 12.0);
/// ```
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct ThemeShadows {
    /// Small elevation shadow.
    pub sm: BoxShadow,
    /// Medium elevation shadow.
    pub md: BoxShadow,
    /// Large elevation shadow.
    pub lg: BoxShadow,
}

/// Shared semantic widget states for theme-driven controls.
///
/// Values cover normal, hover, press, focus, disabled, selected, and invalid
/// control states.
///
/// # Examples
///
/// ```
/// use ailloli_ui_core::style::ThemeState;
/// assert_ne!(ThemeState::Normal, ThemeState::Invalid);
/// ```
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ThemeState {
    /// Idle enabled state.
    Normal,
    /// Pointer is over the control.
    Hovered,
    /// An activation gesture is held.
    Pressed,
    /// Control owns keyboard focus.
    Focused,
    /// Control does not accept interaction.
    Disabled,
    /// Control or option is selected.
    Selected,
    /// Current value failed validation.
    Invalid,
}

/// Semantic color tokens for built-in chrome and controls.
///
/// # Examples
///
/// ```
/// use ailloli_ui_core::style::Theme;
/// let theme = Theme::dark();
/// assert_ne!(theme.button_bg, theme.window_bg);
/// ```
#[derive(Debug, Clone, Copy)]
pub struct Theme {
    /// Client window background.
    pub window_bg: Color,
    /// Title bar background (client chrome).
    pub titlebar_bg: Color,

    /// Normal action-button fill.
    pub button_bg: Color,
    /// Action-button fill while hovered.
    pub button_bg_hover: Color,
    /// Action-button fill while pressed.
    pub button_bg_pressed: Color,

    /// Destructive close-control fill while hovered.
    pub close_bg_hover: Color,
    /// Destructive close-control fill while pressed.
    pub close_bg_pressed: Color,

    /// Hover fill for minimize/maximize title-bar controls.
    pub titlebar_control_hover: Color,
    /// Pressed fill for minimize/maximize title-bar controls.
    pub titlebar_control_pressed: Color,

    /// Default icon foreground on chrome.
    pub icon_fg: Color,
}

impl Theme {
    /// Returns the built-in dark theme with orange action accents.
    ///
    /// # Examples
    ///
    /// ```
    /// use ailloli_ui_core::style::Theme;
    /// assert_eq!(Theme::dark().button_bg.as_rgba8(), (255, 90, 0, 255));
    /// ```
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

    /// Returns the built-in semantic color palette.
    ///
    /// The current palette is a fixed bundle and is not recomputed from
    /// mutable public chrome fields on `Theme`.
    ///
    /// # Examples
    ///
    /// ```
    /// use ailloli_ui_core::style::Theme;
    /// assert_eq!(Theme::dark().palette().accent.as_rgba8(), (255, 90, 0, 255));
    /// ```
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

    /// Returns the fixed `4/8/12/16` logical-pixel radius scale.
    ///
    /// # Examples
    ///
    /// ```
    /// use ailloli_ui_core::style::Theme;
    /// assert_eq!((Theme::dark().radius().sm, Theme::dark().radius().xl), (4.0, 16.0));
    /// ```
    pub const fn radius(self) -> ThemeRadius {
        ThemeRadius {
            sm: 4.0,
            md: 8.0,
            lg: 12.0,
            xl: 16.0,
        }
    }

    /// Returns the fixed `4/8/12/16/24` logical-pixel spacing scale.
    ///
    /// # Examples
    ///
    /// ```
    /// use ailloli_ui_core::style::Theme;
    /// assert_eq!(Theme::dark().spacing().md, 12.0);
    /// ```
    pub const fn spacing(self) -> ThemeSpacing {
        ThemeSpacing {
            xs: 4.0,
            sm: 8.0,
            md: 12.0,
            lg: 16.0,
            xl: 24.0,
        }
    }

    /// Builds standard text styles using the built-in palette text color.
    ///
    /// Sizes are 12, 14, and 18 logical pixels for UI text and 14 for
    /// monospace text.
    ///
    /// # Examples
    ///
    /// ```
    /// use ailloli_ui_core::FontId;
    /// use ailloli_ui_core::style::Theme;
    /// assert_eq!(Theme::dark().typography().mono_md.font, FontId::Mono);
    /// ```
    pub fn typography(self) -> ThemeTypography {
        let palette = self.palette();
        ThemeTypography {
            ui_sm: TextStyle::new(FontId::Ui, 12, palette.text),
            ui_md: TextStyle::new(FontId::Ui, 14, palette.text),
            ui_lg: TextStyle::new(FontId::Ui, 18, palette.text),
            mono_md: TextStyle::new(FontId::Mono, 14, palette.text),
        }
    }

    /// Returns small, medium, and large [`BoxShadow`] presets.
    ///
    /// # Examples
    ///
    /// ```
    /// use ailloli_ui_core::style::Theme;
    /// assert_eq!(Theme::dark().shadows().lg.blur_radius, 24.0);
    /// ```
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
    //! Locks the built-in palette and native-widget scale values.

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
