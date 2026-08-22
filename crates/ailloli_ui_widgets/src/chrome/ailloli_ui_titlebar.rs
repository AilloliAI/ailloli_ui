//! Default title bar for `WindowChrome::AilloliUi` (OS decorations disabled).

use ailloli_ui_core::style::{
    AlignItems, Background, BoxStyle, JustifyContent, Length, Radius, StateStyle,
};
use ailloli_ui_core::EdgeInsets;
use ailloli_ui_core::{AppIcon, Color, FontId, IconId, TextStyle, Theme};
use ailloli_ui_runtime::component::{IntoView, View};

use crate::controls::button::ButtonStyle;
use crate::controls::Button;
use crate::layout::layout_ext::LayoutExt;
use crate::layout::{Container, Row};
use crate::primitives::Icon;
use crate::text::Text;

/// Builds the shared transparent minimize/maximize button style from `theme`.
fn titlebar_chrome_control_style(theme: &Theme) -> ButtonStyle {
    let radius = Radius::uniform(4.0);
    ButtonStyle {
        container: StateStyle {
            normal: BoxStyle::new()
                .background(Background::color(Color::TRANSPARENT))
                .radius(radius),
            hovered: Some(
                BoxStyle::new()
                    .background(Background::color(theme.titlebar_control_hover))
                    .radius(radius),
            ),
            pressed: Some(
                BoxStyle::new()
                    .background(Background::color(theme.titlebar_control_pressed))
                    .radius(radius),
            ),
            focused: None,
            disabled: None,
        },
        text: StateStyle {
            normal: TextStyle::new(FontId::Ui, 12, theme.icon_fg),
            hovered: None,
            pressed: None,
            focused: None,
            disabled: None,
        },
        height: 26.0,
        horizontal_padding: 4.0,
        vertical_padding: 2.0,
        align_items: AlignItems::Center,
        justify_content: JustifyContent::Center,
        baseline_shift: 0.0,
    }
}

/// Builds the close-button style with destructive hover and pressed colors.
fn titlebar_close_style(theme: &Theme) -> ButtonStyle {
    let radius = Radius::uniform(4.0);
    ButtonStyle {
        container: StateStyle {
            normal: BoxStyle::new()
                .background(Background::color(Color::TRANSPARENT))
                .radius(radius),
            hovered: Some(
                BoxStyle::new()
                    .background(Background::color(theme.close_bg_hover))
                    .radius(radius),
            ),
            pressed: Some(
                BoxStyle::new()
                    .background(Background::color(theme.close_bg_pressed))
                    .radius(radius),
            ),
            focused: None,
            disabled: None,
        },
        text: StateStyle {
            normal: TextStyle::new(FontId::Ui, 12, theme.icon_fg),
            hovered: None,
            pressed: None,
            focused: None,
            disabled: None,
        },
        height: 26.0,
        horizontal_padding: 4.0,
        vertical_padding: 2.0,
        align_items: AlignItems::Center,
        justify_content: JustifyContent::Center,
        baseline_shift: 0.0,
    }
}

/// Standard Ailloli UI title bar: `theme.titlebar_bg`, label, min / max / close icons.
///
/// `logical_window_id` must match the `Window::new(...)` id so min/max target the correct window.
/// The returned view has a fixed height of 36 logical pixels and no application icon.
///
/// # Examples
///
/// ```
/// use ailloli_ui_runtime::component::View;
/// use ailloli_ui_widgets::chrome::ailloli_ui_default_titlebar;
/// let titlebar: View<()> = ailloli_ui_default_titlebar("main", "Ailloli");
/// let _ = titlebar;
/// ```
pub fn ailloli_ui_default_titlebar<A: 'static>(
    logical_window_id: impl Into<String>,
    title: impl Into<String>,
) -> View<A> {
    ailloli_ui_default_titlebar_with_icon(logical_window_id, title, None)
}

/// Standard Ailloli UI title bar with an optional static application icon.
///
/// `None` produces the same view as [`ailloli_ui_default_titlebar`]. The icon
/// preserves its source colors and is rendered at 20 logical pixels.
///
/// # Examples
///
/// ```
/// use ailloli_ui_core::AppIcon;
/// use ailloli_ui_runtime::component::View;
/// use ailloli_ui_widgets::chrome::ailloli_ui_default_titlebar_with_icon;
/// let icon = AppIcon::from_static_svg(b"<svg/>", "icon.svg");
/// let titlebar: View<()> =
///     ailloli_ui_default_titlebar_with_icon("main", "Ailloli", Some(icon));
/// let _ = titlebar;
/// ```
pub fn ailloli_ui_default_titlebar_with_icon<A: 'static>(
    logical_window_id: impl Into<String>,
    title: impl Into<String>,
    app_icon: Option<AppIcon>,
) -> View<A> {
    let win_id = logical_window_id.into();
    let title = title.into();
    let theme = Theme::dark();
    let chrome_style = titlebar_chrome_control_style(&theme);
    let close_style = titlebar_close_style(&theme);
    let icon = theme.icon_fg;

    let win_min = win_id.clone();
    let win_max = win_id;

    Container::new()
        .fill_width()
        .height(Length::px(36.0))
        .background(theme.titlebar_bg)
        .child({
            let mut row = Row::new()
                .fill_width()
                .fill_height()
                .align_items(AlignItems::Center)
                .gap(6.0);
            if let Some(app_icon) = app_icon {
                row = row.child(application_icon(app_icon, 20.0));
            }
            row = row
                .child(
                    Text::new(title)
                        .style(TextStyle::new(FontId::Ui, 13, theme.icon_fg))
                        .nowrap()
                        .flex_grow(),
                )
                .child(
                    Button::new()
                        .button_style(chrome_style.clone())
                        .on_click_ctx(move |ctx| ctx.request_minimize_window(&win_min))
                        .child(Icon::new(IconId::Minimize).tint(icon).size(14.0)),
                )
                .child(
                    Button::new()
                        .button_style(chrome_style)
                        .on_click_ctx(move |ctx| ctx.request_toggle_maximize_window(&win_max))
                        .child(Icon::new(IconId::Maximize).tint(icon).size(14.0)),
                )
                .child(
                    Button::new()
                        .button_style(close_style)
                        .on_click_ctx(move |ctx| ctx.request_close())
                        .child(Icon::new(IconId::Close).tint(icon).size(14.0)),
                );
            row.layout_mut().padding = EdgeInsets::new(12.0, 4.0, 12.0, 4.0);
            row
        })
        .into_view()
}

/// Static, color-preserving application icon suitable for custom chrome.
///
/// `size` is used directly as the square logical-pixel extent; callers should
/// pass a finite non-negative value accepted by the normal layout pipeline.
///
/// # Examples
///
/// ```
/// use ailloli_ui_core::AppIcon;
/// use ailloli_ui_runtime::component::View;
/// use ailloli_ui_widgets::chrome::application_icon;
/// let icon = AppIcon::from_static_svg(b"<svg/>", "icon.svg");
/// let view: View<()> = application_icon(icon, 20.0);
/// let _ = view;
/// ```
pub fn application_icon<A: 'static>(icon: AppIcon, size: f32) -> View<A> {
    Icon::new(IconId::Svg(icon.source().clone()))
        .size(size)
        .tint(Color::WHITE)
        .interactive_tint(false)
        .into_view()
}
