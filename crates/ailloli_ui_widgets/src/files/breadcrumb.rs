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

type SegmentHandler<A> = Rc<dyn Fn(&mut EventCtx<A>, FileBreadcrumbSegment)>;

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct FileBreadcrumbSegment {
    pub uri: FileUri,
    pub label: String,
    pub index: usize,
    pub last: bool,
}

#[derive(Clone, Debug, PartialEq)]
pub struct FileBreadcrumbStyle {
    pub text: TextStyle,
    pub active_text: TextStyle,
    pub separator: TextStyle,
    pub gap: f32,
}

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

pub struct FileBreadcrumb<A = ()> {
    pub(crate) layout: LayoutStyle,
    pub(crate) flex_item: FlexItemStyle,
    uri: FileUri,
    base: Option<FileUri>,
    root_label: Option<String>,
    style: FileBreadcrumbStyle,
    on_activate: Option<SegmentHandler<A>>,
}

crate::impl_layout_builders!(FileBreadcrumb);

impl<A: 'static> FileBreadcrumb<A> {
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

    pub fn local_path(path: impl Into<PathBuf>) -> Result<Self, FileError> {
        Ok(Self::new(FileUri::local(make_absolute(path.into()))?))
    }

    pub fn base_uri(mut self, base: FileUri) -> Self {
        self.base = Some(base);
        self
    }

    pub fn base_path(mut self, path: impl Into<PathBuf>) -> Result<Self, FileError> {
        self.base = Some(FileUri::local(make_absolute(path.into()))?);
        Ok(self)
    }

    pub fn root_label(mut self, label: impl Into<String>) -> Self {
        self.root_label = Some(label.into());
        self
    }

    pub fn breadcrumb_style(mut self, style: FileBreadcrumbStyle) -> Self {
        self.style = style;
        self
    }

    pub fn on_activate(mut self, f: impl Fn(FileBreadcrumbSegment) -> A + 'static) -> Self {
        self.on_activate = Some(Rc::new(move |ctx, segment| ctx.dispatch(f(segment))));
        self
    }

    pub fn on_activate_ctx(
        mut self,
        f: impl Fn(&mut EventCtx<A>, FileBreadcrumbSegment) + 'static,
    ) -> Self {
        self.on_activate = Some(Rc::new(f));
        self
    }
}

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

fn same_location(a: &FileUri, b: &FileUri) -> bool {
    a.scheme() == b.scheme() && a.authority() == b.authority()
}

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

    fn uri(path: &str) -> FileUri {
        FileUri::parse(format!("file://{path}")).expect("file uri")
    }
}
