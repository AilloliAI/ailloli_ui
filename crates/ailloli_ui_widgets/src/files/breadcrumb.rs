//! File URI breadcrumb builder and pure segment derivation helpers.

use std::path::{Path, PathBuf};
use std::rc::Rc;

use ailloli_ui_core::style::{AlignItems, FlexItemStyle, LayoutSizeHint, LayoutStyle};
use ailloli_ui_core::{FontId, TextStyle, Theme};
use ailloli_ui_fs::{FileError, FileUri};
use ailloli_ui_runtime::component::{IntoView, View};
use ailloli_ui_runtime::input::EventCtx;

use crate::controls::{Button, ButtonStyle};
use crate::layout::layout_ext::finish_view_sized;
use crate::layout::Row;
use crate::text::Text;

use super::tree::dedupe_file_uris;

/// Shared retained callback for one activated breadcrumb segment.
type SegmentHandler<A> = Rc<dyn Fn(&mut EventCtx<A>, FileBreadcrumbSegment)>;

/// One cumulative location displayed by [`FileBreadcrumb`].
///
/// # Examples
///
/// ```
/// use ailloli_ui_fs::FileUri;
/// use ailloli_ui_widgets::files::FileBreadcrumbSegment;
/// let segment = FileBreadcrumbSegment {
///     uri: FileUri::parse("file:///repo")?, label: "repo".into(), index: 0, last: true,
/// };
/// assert_eq!((segment.index, segment.last), (0, true));
/// # Ok::<(), ailloli_ui_fs::FileError>(())
/// ```
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct FileBreadcrumbSegment {
    /// Cumulative URI activated by this segment.
    pub uri: FileUri,
    /// Visible URI filename or caller-provided root label.
    pub label: String,
    /// Zero-based position in the returned segment vector.
    pub index: usize,
    /// Whether this is the final/active segment.
    pub last: bool,
}

/// Text and spacing styles for a [`FileBreadcrumb`].
///
/// All sizes and `gap` are logical pixels. `gap` is forwarded without clamping,
/// including negative or non-finite values.
///
/// # Examples
///
/// ```
/// use ailloli_ui_widgets::files::FileBreadcrumbStyle;
/// let style = FileBreadcrumbStyle::default();
/// assert_eq!(style.gap, 6.0);
/// assert_eq!(style.text.px_size, 12);
/// ```
#[derive(Clone, Debug, PartialEq)]
pub struct FileBreadcrumbStyle {
    /// Inactive segment text style.
    pub text: TextStyle,
    /// Final active segment text style.
    pub active_text: TextStyle,
    /// `>` separator text style.
    pub separator: TextStyle,
    /// Space between every segment/separator child in logical pixels.
    pub gap: f32,
}

/// Uses 12-pixel UI text, default-theme colors, and a six-pixel gap.
impl Default for FileBreadcrumbStyle {
    fn default() -> Self {
        let palette = Theme::default().palette();
        Self {
            text: TextStyle::new(FontId::Ui, 12, palette.text_muted),
            active_text: TextStyle::new(FontId::Ui, 12, palette.text),
            separator: TextStyle::new(FontId::Ui, 12, palette.text_muted.with_alpha(0.72)),
            gap: 6.0,
        }
    }
}

/// Horizontal cumulative-path breadcrumb for a file URI.
///
/// Without an activation handler segments are plain text. With one, each
/// segment becomes a text-only button; separators remain non-interactive. `A`
/// is the surrounding application's action type.
///
/// # Examples
///
/// ```
/// use ailloli_ui_fs::FileUri;
/// use ailloli_ui_widgets::files::FileBreadcrumb;
/// let breadcrumb = FileBreadcrumb::<()>::new(FileUri::parse("file:///repo/src/lib.rs")?)
///     .root_label("workspace");
/// let _ = breadcrumb;
/// # Ok::<(), ailloli_ui_fs::FileError>(())
/// ```
pub struct FileBreadcrumb<A = ()> {
    /// Standard logical-pixel size and position constraints.
    pub(crate) layout: LayoutStyle,
    /// Standard flex-parent participation settings.
    pub(crate) flex_item: FlexItemStyle,
    uri: FileUri,
    base: Option<FileUri>,
    root_label: Option<String>,
    style: FileBreadcrumbStyle,
    on_activate: Option<SegmentHandler<A>>,
}

crate::impl_layout_builders!(FileBreadcrumb);

impl<A: 'static> FileBreadcrumb<A> {
    /// Creates a breadcrumb for `uri` with no base, root label, or callback.
    ///
    /// # Examples
    ///
    /// ```
    /// use ailloli_ui_fs::FileUri;
    /// use ailloli_ui_widgets::files::FileBreadcrumb;
    /// let breadcrumb = FileBreadcrumb::<()>::new(FileUri::parse("file:///repo/main.rs")?);
    /// let _ = breadcrumb;
    /// # Ok::<(), ailloli_ui_fs::FileError>(())
    /// ```
    pub fn new(uri: FileUri) -> Self {
        Self {
            layout: LayoutStyle::default(),
            flex_item: FlexItemStyle::default(),
            uri,
            base: None,
            root_label: None,
            style: FileBreadcrumbStyle::default(),
            on_activate: None,
        }
    }

    /// Creates a breadcrumb from an absolute or working-directory-relative path.
    ///
    /// Existing paths are canonicalized; nonexistent paths retain their joined
    /// lexical form. Relative-path resolution falls back to `/` only if querying
    /// the current directory fails.
    ///
    /// # Errors
    ///
    /// Returns [`FileError`] when the resulting native path cannot be represented
    /// as a local file URI, for example because a component is not valid UTF-8.
    ///
    /// # Examples
    ///
    /// ```
    /// use ailloli_ui_widgets::files::FileBreadcrumb;
    /// let breadcrumb = FileBreadcrumb::<()>::local_path(".")?;
    /// let _ = breadcrumb;
    /// # Ok::<(), ailloli_ui_fs::FileError>(())
    /// ```
    pub fn local_path(path: impl Into<PathBuf>) -> Result<Self, FileError> {
        Ok(Self::new(FileUri::local(make_absolute(path.into()))?))
    }

    /// Sets the first URI shown when it matches the target location/prefix.
    ///
    /// The base is used only when scheme and authority match and the target path
    /// starts with the base path. Otherwise the absolute URI is segmented.
    ///
    /// # Examples
    ///
    /// ```
    /// use ailloli_ui_fs::FileUri;
    /// use ailloli_ui_widgets::files::FileBreadcrumb;
    /// let target = FileUri::parse("file:///repo/src/lib.rs")?;
    /// let breadcrumb = FileBreadcrumb::<()>::new(target)
    ///     .base_uri(FileUri::parse("file:///repo")?);
    /// let _ = breadcrumb;
    /// # Ok::<(), ailloli_ui_fs::FileError>(())
    /// ```
    pub fn base_uri(mut self, base: FileUri) -> Self {
        self.base = Some(base);
        self
    }

    /// Resolves a local path and sets it as the breadcrumb base URI.
    ///
    /// Resolution and failure semantics match [`Self::local_path`].
    ///
    /// # Errors
    ///
    /// Returns [`FileError`] if the native base cannot be converted to a URI.
    ///
    /// # Examples
    ///
    /// ```
    /// use ailloli_ui_widgets::files::FileBreadcrumb;
    /// let breadcrumb = FileBreadcrumb::<()>::local_path(".")?.base_path(".")?;
    /// let _ = breadcrumb;
    /// # Ok::<(), ailloli_ui_fs::FileError>(())
    /// ```
    pub fn base_path(mut self, path: impl Into<PathBuf>) -> Result<Self, FileError> {
        self.base = Some(FileUri::local(make_absolute(path.into()))?);
        Ok(self)
    }

    /// Overrides the first visible segment label.
    ///
    /// The label is stored verbatim, including an empty string. It does not
    /// change the segment URI or labels after index zero.
    ///
    /// # Examples
    ///
    /// ```
    /// use ailloli_ui_fs::FileUri;
    /// use ailloli_ui_widgets::files::FileBreadcrumb;
    /// let breadcrumb = FileBreadcrumb::<()>::new(FileUri::parse("file:///repo/src")?)
    ///     .root_label("workspace");
    /// let _ = breadcrumb;
    /// # Ok::<(), ailloli_ui_fs::FileError>(())
    /// ```
    pub fn root_label(mut self, label: impl Into<String>) -> Self {
        self.root_label = Some(label.into());
        self
    }

    /// Replaces text styles and logical-pixel child spacing.
    ///
    /// # Examples
    ///
    /// ```
    /// use ailloli_ui_fs::FileUri;
    /// use ailloli_ui_widgets::files::{FileBreadcrumb, FileBreadcrumbStyle};
    /// let breadcrumb = FileBreadcrumb::<()>::new(FileUri::parse("file:///repo")?)
    ///     .breadcrumb_style(FileBreadcrumbStyle::default());
    /// let _ = breadcrumb;
    /// # Ok::<(), ailloli_ui_fs::FileError>(())
    /// ```
    pub fn breadcrumb_style(mut self, style: FileBreadcrumbStyle) -> Self {
        self.style = style;
        self
    }

    /// Maps an activated segment into an application action.
    ///
    /// Installing this callback also changes every segment from static text to
    /// a text-only button.
    ///
    /// # Examples
    ///
    /// ```
    /// use ailloli_ui_fs::FileUri;
    /// use ailloli_ui_widgets::files::{FileBreadcrumb, FileBreadcrumbSegment};
    /// enum Action { Open(FileBreadcrumbSegment) }
    /// let breadcrumb = FileBreadcrumb::<Action>::new(FileUri::parse("file:///repo")?)
    ///     .on_activate(Action::Open);
    /// let _ = breadcrumb;
    /// # Ok::<(), ailloli_ui_fs::FileError>(())
    /// ```
    pub fn on_activate(mut self, f: impl Fn(FileBreadcrumbSegment) -> A + 'static) -> Self {
        self.on_activate = Some(Rc::new(move |ctx, segment| ctx.dispatch(f(segment))));
        self
    }

    /// Handles activation with direct access to the event context.
    ///
    /// Use this form to dispatch zero or multiple actions or request additional
    /// runtime work.
    ///
    /// # Examples
    ///
    /// ```
    /// use ailloli_ui_fs::FileUri;
    /// use ailloli_ui_widgets::files::FileBreadcrumb;
    /// let breadcrumb = FileBreadcrumb::<()>::new(FileUri::parse("file:///repo")?)
    ///     .on_activate_ctx(|_ctx, _segment| {});
    /// let _ = breadcrumb;
    /// # Ok::<(), ailloli_ui_fs::FileError>(())
    /// ```
    pub fn on_activate_ctx(
        mut self,
        f: impl Fn(&mut EventCtx<A>, FileBreadcrumbSegment) + 'static,
    ) -> Self {
        self.on_activate = Some(Rc::new(f));
        self
    }
}

/// Converts segments into a nowrap, vertically centered row of text/buttons.
impl<A: 'static> IntoView<A> for FileBreadcrumb<A> {
    fn into_view(self) -> View<A> {
        let mut row = Row::new()
            .gap(self.style.gap)
            .align_items(AlignItems::Center);
        row.layout = self.layout;

        let segments =
            breadcrumb_segments(&self.uri, self.base.as_ref(), self.root_label.as_deref());
        for (idx, segment) in segments.iter().cloned().enumerate() {
            if idx > 0 {
                row = row.child(Text::new(">").style(self.style.separator).nowrap());
            }

            let text_style = if segment.last {
                self.style.active_text
            } else {
                self.style.text
            };
            if let Some(handler) = &self.on_activate {
                let handler = handler.clone();
                let segment_for_click = segment.clone();
                row = row.child(
                    Button::new()
                        .button_style(ButtonStyle::text_only(text_style.color))
                        .child(Text::new(segment.label).style(text_style).nowrap())
                        .on_click_ctx(move |ctx| handler(ctx, segment_for_click.clone())),
                );
            } else {
                row = row.child(Text::new(segment.label).style(text_style).nowrap());
            }
        }

        finish_view_sized(
            row.into_view(),
            self.flex_item,
            LayoutSizeHint::from_layout(self.layout),
        )
    }
}

/// Builds cumulative breadcrumb segments for a URI and optional base.
///
/// A base applies only when scheme/authority match and the target path has the
/// base's byte prefix. Duplicate URIs are removed. A root-only URI may yield an
/// empty vector because it has no non-empty path component.
///
/// # Examples
///
/// ```
/// use ailloli_ui_fs::FileUri;
/// use ailloli_ui_widgets::files::breadcrumb_segments;
/// let base = FileUri::parse("file:///repo")?;
/// let target = FileUri::parse("file:///repo/src/lib.rs")?;
/// let labels: Vec<_> = breadcrumb_segments(&target, Some(&base), Some("workspace"))
///     .into_iter().map(|segment| segment.label).collect();
/// assert_eq!(labels, ["workspace", "src", "lib.rs"]);
/// # Ok::<(), ailloli_ui_fs::FileError>(())
/// ```
pub fn breadcrumb_segments(
    uri: &FileUri,
    base: Option<&FileUri>,
    root_label: Option<&str>,
) -> Vec<FileBreadcrumbSegment> {
    let Some(base) =
        base.filter(|base| same_location(base, uri) && uri.path().starts_with(base.path()))
    else {
        return segments_from_absolute(uri, root_label);
    };

    let mut uris = vec![base.clone()];
    let mut current_path = base.path().trim_end_matches('/').to_string();
    let relative = uri
        .path()
        .trim_start_matches(base.path().trim_end_matches('/'))
        .trim_start_matches('/');
    for part in relative.split('/').filter(|part| !part.is_empty()) {
        current_path = if current_path == "/" {
            format!("/{part}")
        } else {
            format!("{current_path}/{part}")
        };
        if let Ok(next) = FileUri::new(
            uri.scheme().to_string(),
            uri.authority().map(str::to_string),
            current_path.clone(),
        ) {
            uris.push(next);
        }
    }
    make_segments(dedupe_file_uris(uris), root_label)
}

/// Expands every non-empty absolute path component into a cumulative URI.
fn segments_from_absolute(uri: &FileUri, root_label: Option<&str>) -> Vec<FileBreadcrumbSegment> {
    let mut current = String::new();
    let mut uris = Vec::new();
    for part in uri.path().split('/').filter(|part| !part.is_empty()) {
        current.push('/');
        current.push_str(part);
        if let Ok(next) = FileUri::new(
            uri.scheme().to_string(),
            uri.authority().map(str::to_string),
            current.clone(),
        ) {
            uris.push(next);
        }
    }
    make_segments(uris, root_label)
}

/// Assigns labels, contiguous indices, and exactly one final marker when nonempty.
fn make_segments(uris: Vec<FileUri>, root_label: Option<&str>) -> Vec<FileBreadcrumbSegment> {
    let last_index = uris.len().saturating_sub(1);
    uris.into_iter()
        .enumerate()
        .map(|(index, uri)| {
            let label = if index == 0 {
                root_label
                    .map(str::to_string)
                    .or_else(|| uri.file_name().map(str::to_string))
                    .unwrap_or_else(|| uri.path().to_string())
            } else {
                uri.file_name()
                    .map(str::to_string)
                    .unwrap_or_else(|| uri.path().to_string())
            };
            FileBreadcrumbSegment {
                uri,
                label,
                index,
                last: index == last_index,
            }
        })
        .collect()
}

/// Compares scheme and optional authority without considering path.
fn same_location(a: &FileUri, b: &FileUri) -> bool {
    a.scheme() == b.scheme() && a.authority() == b.authority()
}

/// Makes a path absolute and canonicalizes it when the target exists.
fn make_absolute(path: PathBuf) -> PathBuf {
    let path = if path.is_absolute() {
        path
    } else {
        std::env::current_dir()
            .unwrap_or_else(|_| Path::new("/").to_path_buf())
            .join(path)
    };
    path.canonicalize().unwrap_or(path)
}

#[cfg(test)]
/// Verifies base trimming, custom root labels, order, and final-segment marking.
mod tests {
    use super::*;

    #[test]
    fn breadcrumb_segments_trim_base_path_and_use_root_label() {
        let base = uri("/repo");
        let file = uri("/repo/sample_app/src/view/panes/left.rs");

        let labels = breadcrumb_segments(&file, Some(&base), Some("ailloli_ui"))
            .into_iter()
            .map(|segment| (segment.label, segment.last))
            .collect::<Vec<_>>();

        assert_eq!(
            labels,
            [
                ("ailloli_ui".to_string(), false),
                ("sample_app".to_string(), false),
                ("src".to_string(), false),
                ("view".to_string(), false),
                ("panes".to_string(), false),
                ("left.rs".to_string(), true),
            ]
        );
    }

    /// Creates a local file URI fixture from an absolute path.
    fn uri(path: &str) -> FileUri {
        FileUri::parse(format!("file://{path}")).expect("file uri")
    }
}
