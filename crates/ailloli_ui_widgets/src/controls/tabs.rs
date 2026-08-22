//! Immediate-mode drawing and hit geometry for a compact tab bar.

use ailloli_ui_core::{Color, FontId, IconId, Rect, TextStyle};
use ailloli_ui_runtime::{DrawCmd, DrawImage, DrawRRect, DrawRect, DrawText};
use ailloli_ui_text::{TextLayoutParams, TextSystem, WrapMode};

/// Default tab-bar height in logical pixels.
///
/// The drawing functions still use the height of the supplied [`Rect`]; this
/// constant is a caller convention rather than an enforced constraint.
///
/// # Examples
///
/// ```
/// use ailloli_ui_widgets::controls::tabs::TABS_BAR_H;
/// assert_eq!(TABS_BAR_H, 36.0);
/// ```
pub const TABS_BAR_H: f32 = 36.0;

/// Read-only data source for one immediate-mode tab.
///
/// IDs are cloned into [`TabsLayout::tab_rects`] and should be unique if the
/// caller uses them for hit routing. The renderer does not validate uniqueness.
///
/// # Examples
///
/// ```
/// use ailloli_ui_widgets::controls::tabs::TabsItem;
/// struct Tab;
/// impl TabsItem for Tab {
///     fn id(&self) -> &str { "docs" }
///     fn title(&self) -> &str { "Docs" }
///     fn selected(&self) -> bool { true }
/// }
/// assert!(Tab.selected());
/// ```
pub trait TabsItem {
    /// Returns the stable identity copied into output hit geometry.
    ///
    /// # Examples
    ///
    /// ```
    /// use ailloli_ui_widgets::controls::tabs::TabsItem;
    /// struct Tab;
    /// impl TabsItem for Tab {
    ///     fn id(&self) -> &str { "one" }
    ///     fn title(&self) -> &str { "One" }
    ///     fn selected(&self) -> bool { false }
    /// }
    /// assert_eq!(Tab.id(), "one");
    /// ```
    fn id(&self) -> &str;
    /// Returns the visible title; empty text is rendered as `"Conversation"`.
    ///
    /// # Examples
    ///
    /// ```
    /// use ailloli_ui_widgets::controls::tabs::TabsItem;
    /// struct Tab;
    /// impl TabsItem for Tab {
    ///     fn id(&self) -> &str { "one" }
    ///     fn title(&self) -> &str { "One" }
    ///     fn selected(&self) -> bool { false }
    /// }
    /// assert_eq!(Tab.title(), "One");
    /// ```
    fn title(&self) -> &str;
    /// Returns whether selected background and border colors are used.
    ///
    /// The renderer permits zero, one, or multiple selected items.
    ///
    /// # Examples
    ///
    /// ```
    /// use ailloli_ui_widgets::controls::tabs::TabsItem;
    /// struct Tab;
    /// impl TabsItem for Tab {
    ///     fn id(&self) -> &str { "one" }
    ///     fn title(&self) -> &str { "One" }
    ///     fn selected(&self) -> bool { true }
    /// }
    /// assert!(Tab.selected());
    /// ```
    fn selected(&self) -> bool;

    /// Returns an optional 14-logical-pixel icon before the title.
    ///
    /// The default is `None` and reserves no icon space.
    ///
    /// # Examples
    ///
    /// ```
    /// use ailloli_ui_widgets::controls::tabs::TabsItem;
    /// struct Tab;
    /// impl TabsItem for Tab {
    ///     fn id(&self) -> &str { "one" }
    ///     fn title(&self) -> &str { "One" }
    ///     fn selected(&self) -> bool { false }
    /// }
    /// assert!(Tab.leading_icon().is_none());
    /// ```
    fn leading_icon(&self) -> Option<&IconId> {
        None
    }

    /// Returns an optional icon tint; `None` uses [`TabsStyle::text_muted`].
    ///
    /// The value has no effect when [`Self::leading_icon`] returns `None`.
    ///
    /// # Examples
    ///
    /// ```
    /// use ailloli_ui_widgets::controls::tabs::TabsItem;
    /// struct Tab;
    /// impl TabsItem for Tab {
    ///     fn id(&self) -> &str { "one" }
    ///     fn title(&self) -> &str { "One" }
    ///     fn selected(&self) -> bool { false }
    /// }
    /// assert!(Tab.leading_icon_tint().is_none());
    /// ```
    fn leading_icon_tint(&self) -> Option<Color> {
        None
    }

    /// Returns the exact scope sentinel used to color the optional strip.
    ///
    /// Accepted colored sentinels are `"app_global"`, `"workspace_global"`,
    /// `"server"`, `"path"`, `"file"`, and `"task"`. Empty or any other value
    /// uses neutral gray. Disable the strip with [`TabsBarOptions::show_scope_strip`].
    ///
    /// # Examples
    ///
    /// ```
    /// use ailloli_ui_widgets::controls::tabs::TabsItem;
    /// struct Tab;
    /// impl TabsItem for Tab {
    ///     fn id(&self) -> &str { "one" }
    ///     fn title(&self) -> &str { "One" }
    ///     fn selected(&self) -> bool { false }
    ///     fn scope_kind(&self) -> &str { "workspace_global" }
    /// }
    /// assert_eq!(Tab.scope_kind(), "workspace_global");
    /// ```
    fn scope_kind(&self) -> &str {
        ""
    }

    /// Returns whether the shared unread/processing dot should be shown.
    ///
    /// The default is `false`.
    ///
    /// # Examples
    ///
    /// ```
    /// use ailloli_ui_widgets::controls::tabs::TabsItem;
    /// struct Tab;
    /// impl TabsItem for Tab {
    ///     fn id(&self) -> &str { "one" }
    ///     fn title(&self) -> &str { "One" }
    ///     fn selected(&self) -> bool { false }
    /// }
    /// assert!(!Tab.unread());
    /// ```
    fn unread(&self) -> bool {
        false
    }

    /// Returns whether the same status dot used for unread state is shown.
    ///
    /// `unread() || processing()` produces one dot, never two. The default is
    /// `false`; this helper does not animate processing state.
    ///
    /// # Examples
    ///
    /// ```
    /// use ailloli_ui_widgets::controls::tabs::TabsItem;
    /// struct Tab;
    /// impl TabsItem for Tab {
    ///     fn id(&self) -> &str { "one" }
    ///     fn title(&self) -> &str { "One" }
    ///     fn selected(&self) -> bool { false }
    ///     fn processing(&self) -> bool { true }
    /// }
    /// assert!(Tab.processing());
    /// ```
    fn processing(&self) -> bool {
        false
    }
}

#[derive(Debug, Clone, Copy)]
/// Visibility switches for optional tab-bar affordances.
///
/// All three options default to `true`.
///
/// # Examples
///
/// ```
/// use ailloli_ui_widgets::controls::TabsBarOptions;
/// let options = TabsBarOptions::default();
/// assert!(options.show_trailing_actions);
/// assert!(options.show_tab_close_affordance);
/// assert!(options.show_scope_strip);
/// ```
pub struct TabsBarOptions {
    /// Paint new-tab/history buttons and reserve their 62-pixel region.
    pub show_trailing_actions: bool,
    /// Paint an `x` label in each tab; hit geometry is returned regardless.
    pub show_tab_close_affordance: bool,
    /// Paint the semantic scope strip and reserve its title inset.
    pub show_scope_strip: bool,
}

impl Default for TabsBarOptions {
    fn default() -> Self {
        Self {
            show_trailing_actions: true,
            show_tab_close_affordance: true,
            show_scope_strip: true,
        }
    }
}

#[derive(Debug, Clone, Copy)]
/// Colors used by [`draw_tabs_bar`] and [`draw_tabs_bar_with_options`].
///
/// Geometry and typography are fixed by the drawing helper.
///
/// # Examples
///
/// ```
/// use ailloli_ui_widgets::controls::tabs::TabsStyle;
/// let style = TabsStyle::default();
/// assert_ne!(style.tab_bg, style.tab_bg_selected);
/// ```
pub struct TabsStyle {
    /// Whole-bar background.
    pub bar_bg: Color,
    /// Unselected tab background.
    pub tab_bg: Color,
    /// Selected tab background.
    pub tab_bg_selected: Color,
    /// Unselected one-pixel tab border color.
    pub tab_border: Color,
    /// Selected one-pixel tab border color.
    pub tab_border_selected: Color,
    /// Tab title and trailing action icon color.
    pub text_fg: Color,
    /// Default leading-icon tint and close-affordance color.
    pub text_muted: Color,
    /// Shared unread-or-processing dot color.
    pub unread_dot: Color,
}

impl Default for TabsStyle {
    fn default() -> Self {
        Self {
            bar_bg: Color::rgba(11, 16, 32, 1.0),
            tab_bg: Color::rgba(17, 24, 40, 1.0),
            tab_bg_selected: Color::rgba(31, 38, 55, 1.0),
            tab_border: Color::rgba(31, 38, 55, 1.0),
            tab_border_selected: Color::rgba(37, 99, 235, 1.0),
            text_fg: Color::rgba(243, 246, 251, 1.0),
            text_muted: Color::rgba(156, 163, 175, 1.0),
            unread_dot: Color::rgba(245, 158, 11, 1.0),
        }
    }
}

#[derive(Debug, Clone)]
/// Hit geometry returned alongside tab-bar draw commands.
///
/// Each tab tuple contains `(owned_id, whole_tab_rect, close_rect)` in visible
/// input order. Tabs that do not fit are absent.
///
/// # Examples
///
/// ```
/// use ailloli_ui_core::Rect;
/// use ailloli_ui_widgets::controls::tabs::{TabsControls, TabsLayout};
/// let layout = TabsLayout {
///     tab_rects: vec![("one".into(), Rect::new(0.0, 0.0, 120.0, 28.0), Rect::new(98.0, 0.0, 22.0, 28.0))],
///     controls: TabsControls {
///         new_tab_rect: Rect::new(0.0, 0.0, 0.0, 0.0),
///         history_rect: Rect::new(0.0, 0.0, 0.0, 0.0),
///         can_create_path_tab: false,
///     },
/// };
/// assert_eq!(layout.tab_rects[0].0, "one");
/// ```
pub struct TabsLayout {
    /// Visible tab identities and their tab/close hit rectangles.
    pub tab_rects: Vec<(String, Rect, Rect)>,
    /// Trailing-control geometry and caller-supplied capability flag.
    pub controls: TabsControls,
}

#[derive(Debug, Clone, Copy)]
/// Hit geometry and capability metadata for trailing tab actions.
///
/// When trailing actions are hidden, both rectangles are the zero rectangle.
/// `can_create_path_tab` is returned unchanged but does not alter painting.
///
/// # Examples
///
/// ```
/// use ailloli_ui_core::Rect;
/// use ailloli_ui_widgets::controls::tabs::TabsControls;
/// let controls = TabsControls {
///     new_tab_rect: Rect::new(8.0, 4.0, 28.0, 28.0),
///     history_rect: Rect::new(42.0, 4.0, 28.0, 28.0),
///     can_create_path_tab: true,
/// };
/// assert!(controls.can_create_path_tab);
/// ```
pub struct TabsControls {
    /// New-tab button rectangle in the same coordinate space as the input bar.
    pub new_tab_rect: Rect,
    /// History button rectangle in the same coordinate space as the input bar.
    pub history_rect: Rect,
    /// Caller-supplied permission for path-tab creation; not a disabled style.
    pub can_create_path_tab: bool,
}

/// Draws a tab bar with every optional affordance enabled.
///
/// This is equivalent to [`draw_tabs_bar_with_options`] with
/// [`TabsBarOptions::default`]. It returns commands plus hit geometry and does
/// not handle input. Only complete tabs with at least 120 logical pixels fit;
/// visible tabs target 220 pixels and preserve input order.
///
/// # Examples
///
/// ```
/// use ailloli_ui_core::Rect;
/// use ailloli_ui_text::TextSystem;
/// use ailloli_ui_widgets::controls::tabs::{draw_tabs_bar, TabsItem, TabsStyle};
/// struct Tab;
/// impl TabsItem for Tab {
///     fn id(&self) -> &str { "one" }
///     fn title(&self) -> &str { "One" }
///     fn selected(&self) -> bool { true }
/// }
/// let mut text = TextSystem::new();
/// let (commands, layout) = draw_tabs_bar(
///     Rect::new(0.0, 0.0, 400.0, 36.0), &[Tab], true, TabsStyle::default(), &mut text,
/// );
/// assert!(!commands.is_empty());
/// assert_eq!(layout.tab_rects.len(), 1);
/// ```
pub fn draw_tabs_bar<T: TabsItem>(
    rect: Rect,
    tabs: &[T],
    can_create_path_tab: bool,
    style: TabsStyle,
    text: &mut TextSystem,
) -> (Vec<DrawCmd>, TabsLayout) {
    draw_tabs_bar_with_options(
        rect,
        tabs,
        can_create_path_tab,
        style,
        text,
        TabsBarOptions::default(),
    )
}

/// Draws a tab bar and returns matching hit geometry.
///
/// The helper does not clip commands or process events. With trailing actions
/// visible it reserves two 28-pixel buttons plus a 6-pixel gap. Close hit rects
/// remain in `tab_rects` even when their `x` labels are hidden. An empty title
/// paints as `"Conversation"`; unread and processing share one status dot.
///
/// # Examples
///
/// ```
/// use ailloli_ui_core::Rect;
/// use ailloli_ui_text::TextSystem;
/// use ailloli_ui_widgets::controls::{draw_tabs_bar_with_options, TabsBarOptions};
/// use ailloli_ui_widgets::controls::tabs::{TabsItem, TabsStyle};
/// struct Tab;
/// impl TabsItem for Tab {
///     fn id(&self) -> &str { "one" }
///     fn title(&self) -> &str { "" }
///     fn selected(&self) -> bool { false }
/// }
/// let options = TabsBarOptions { show_trailing_actions: false, ..TabsBarOptions::default() };
/// let mut text = TextSystem::new();
/// let (_, layout) = draw_tabs_bar_with_options(
///     Rect::new(0.0, 0.0, 240.0, 36.0), &[Tab], false, TabsStyle::default(), &mut text, options,
/// );
/// assert_eq!(layout.tab_rects.len(), 1);
/// assert_eq!(layout.controls.new_tab_rect.w, 0.0);
/// ```
pub fn draw_tabs_bar_with_options<T: TabsItem>(
    rect: Rect,
    tabs: &[T],
    can_create_path_tab: bool,
    style: TabsStyle,
    text: &mut TextSystem,
    options: TabsBarOptions,
) -> (Vec<DrawCmd>, TabsLayout) {
    let mut out = Vec::new();

    out.push(DrawCmd::Rect(DrawRect {
        rect,
        color: style.bar_bg,
    }));

    let pad_x = 8.0;
    let pad_y = 4.0;
    let btn = 28.0;
    let gap = 6.0;
    let mut x = rect.x + pad_x;
    let y = rect.y + pad_y;
    let h = rect.h - pad_y * 2.0;

    let right_controls_w = if options.show_trailing_actions {
        btn * 2.0 + gap
    } else {
        0.0
    };
    let max_tabs_w = (rect.w - pad_x * 2.0 - right_controls_w).max(0.0);
    let x_end = x + max_tabs_w;

    let mut tab_rects = Vec::new();
    /// Appends one unwrapped text command at an already resolved baseline.
    fn push_text(
        out: &mut Vec<DrawCmd>,
        text: &mut TextSystem,
        pos: [f32; 2],
        color: Color,
        font: FontId,
        px_size: u16,
        s: &str,
    ) {
        out.push(DrawCmd::Text(DrawText {
            pos,
            color,
            decoration: ailloli_ui_core::TextDecoration::None,
            layout: text.layout_cached(TextLayoutParams {
                text: s,
                style: TextStyle::new(font, px_size, color),
                max_width: None,
                wrap_mode: WrapMode::NoWrap,
            }),
        }));
    }

    for tab in tabs {
        if x + 120.0 > x_end {
            break;
        }
        let w = 220.0_f32.min((x_end - x).max(120.0));
        let tab_r = Rect::new(x, y, w, h);
        tab_rects.push((
            tab.id().to_string(),
            tab_r,
            Rect::new(tab_r.x + tab_r.w - 22.0, tab_r.y, 22.0, tab_r.h),
        ));

        let bg = if tab.selected() {
            style.tab_bg_selected
        } else {
            style.tab_bg
        };
        let border = if tab.selected() {
            style.tab_border_selected
        } else {
            style.tab_border
        };

        out.push(DrawCmd::RRect(DrawRRect {
            rect: tab_r,
            radius: 6.0,
            color: bg,
        }));
        out.push(DrawCmd::Rect(DrawRect {
            rect: Rect::new(tab_r.x, tab_r.y, tab_r.w, 1.0),
            color: border,
        }));
        out.push(DrawCmd::Rect(DrawRect {
            rect: Rect::new(tab_r.x, tab_r.y + tab_r.h - 1.0, tab_r.w, 1.0),
            color: border,
        }));
        out.push(DrawCmd::Rect(DrawRect {
            rect: Rect::new(tab_r.x, tab_r.y, 1.0, tab_r.h),
            color: border,
        }));
        out.push(DrawCmd::Rect(DrawRect {
            rect: Rect::new(tab_r.x + tab_r.w - 1.0, tab_r.y, 1.0, tab_r.h),
            color: border,
        }));

        if options.show_scope_strip {
            out.push(DrawCmd::RRect(DrawRRect {
                rect: Rect::new(tab_r.x + 6.0, tab_r.y + 4.0, 6.0, tab_r.h - 8.0),
                radius: 3.0,
                color: scope_color(tab.scope_kind()),
            }));
        }

        let title = if tab.title().is_empty() {
            "Conversation"
        } else {
            tab.title()
        };
        let title_x = if options.show_scope_strip {
            tab_r.x + 18.0
        } else {
            tab_r.x + 8.0
        };
        let title_x = if let Some(icon) = tab.leading_icon() {
            let icon_size = 14.0;
            out.push(DrawCmd::Image(DrawImage {
                rect: Rect::new(
                    title_x,
                    tab_r.y + (tab_r.h - icon_size) * 0.5,
                    icon_size,
                    icon_size,
                ),
                icon: icon.clone(),
                tint: tab.leading_icon_tint().unwrap_or(style.text_muted),
                rotation_rad: 0.0,
            }));
            title_x + icon_size + 6.0
        } else {
            title_x
        };
        push_text(
            &mut out,
            text,
            [title_x, tab_r.y + 18.0],
            style.text_fg,
            FontId::Ui,
            12,
            title,
        );

        if tab.unread() || tab.processing() {
            out.push(DrawCmd::RRect(DrawRRect {
                rect: Rect::new(
                    tab_r.x + tab_r.w - 34.0,
                    tab_r.y + (tab_r.h - 8.0) / 2.0,
                    8.0,
                    8.0,
                ),
                radius: 4.0,
                color: style.unread_dot,
            }));
        }

        if options.show_tab_close_affordance {
            push_text(
                &mut out,
                text,
                [tab_r.x + tab_r.w - 16.0, tab_r.y + 18.0],
                style.text_muted,
                FontId::Ui,
                14,
                "x",
            );
        }

        x += w + gap;
    }

    let (history_rect, new_tab_rect) = if options.show_trailing_actions {
        let history_rect = Rect::new(rect.x + rect.w - pad_x - btn, y, btn, btn);
        let new_tab_rect = Rect::new(history_rect.x - gap - btn, y, btn, btn);
        (history_rect, new_tab_rect)
    } else {
        let empty = Rect::new(0.0, 0.0, 0.0, 0.0);
        (empty, empty)
    };

    let controls = TabsControls {
        new_tab_rect,
        history_rect,
        can_create_path_tab,
    };

    if options.show_trailing_actions {
        out.push(DrawCmd::RRect(DrawRRect {
            rect: new_tab_rect,
            radius: 8.0,
            color: style.tab_bg,
        }));
        out.push(DrawCmd::Image(DrawImage {
            rect: Rect::new(new_tab_rect.x + 6.0, new_tab_rect.y + 6.0, 16.0, 16.0),
            icon: IconId::Plus,
            tint: style.text_fg,
            rotation_rad: 0.0,
        }));

        out.push(DrawCmd::RRect(DrawRRect {
            rect: history_rect,
            radius: 8.0,
            color: style.tab_bg,
        }));
        out.push(DrawCmd::Image(DrawImage {
            rect: Rect::new(history_rect.x + 6.0, history_rect.y + 6.0, 16.0, 16.0),
            icon: IconId::History,
            tint: style.text_fg,
            rotation_rad: 0.0,
        }));
    }

    (
        out,
        TabsLayout {
            tab_rects,
            controls,
        },
    )
}

/// Maps the six documented scope sentinels to colors; all others use gray.
fn scope_color(kind: &str) -> Color {
    match kind {
        "app_global" => Color::rgba(139, 92, 246, 1.0),
        "workspace_global" => Color::rgba(37, 99, 235, 1.0),
        "server" => Color::rgba(249, 115, 38, 1.0),
        "path" => Color::rgba(16, 185, 129, 1.0),
        "file" => Color::rgba(245, 158, 11, 1.0),
        "task" => Color::rgba(236, 60, 153, 1.0),
        _ => Color::rgba(75, 85, 102, 1.0),
    }
}
