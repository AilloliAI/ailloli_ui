use crate::FileUri;

#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub enum WatchEventKind {
    Created,
    Modified,
    Removed,
    Renamed,
}

#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct WatchEvent {
    pub kind: WatchEventKind,
    pub uri: FileUri,
    pub previous_uri: Option<FileUri>,
}
