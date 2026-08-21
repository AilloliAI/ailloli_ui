use crate::{FileIdentity, FileUri};

#[non_exhaustive]
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub enum WatchEventKind {
    Created,
    Modified,
    Removed,
    Renamed,
    Moved,
    Overflow,
}

#[non_exhaustive]
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct WatchEvent {
    kind: WatchEventKind,
    uri: FileUri,
    previous_uri: Option<FileUri>,
    sequence: u64,
    generation: u64,
    identity: Option<FileIdentity>,
}

impl WatchEvent {
    pub fn new(kind: WatchEventKind, uri: FileUri, sequence: u64, generation: u64) -> Self {
        Self {
            kind,
            uri,
            previous_uri: None,
            sequence,
            generation,
            identity: None,
        }
    }

    pub fn with_previous_uri(mut self, previous_uri: FileUri) -> Self {
        self.previous_uri = Some(previous_uri);
        self
    }

    pub fn with_identity(mut self, identity: FileIdentity) -> Self {
        self.identity = Some(identity);
        self
    }

    pub const fn kind(&self) -> WatchEventKind {
        self.kind
    }

    pub const fn uri(&self) -> &FileUri {
        &self.uri
    }

    pub const fn previous_uri(&self) -> Option<&FileUri> {
        self.previous_uri.as_ref()
    }

    pub const fn sequence(&self) -> u64 {
        self.sequence
    }

    pub const fn generation(&self) -> u64 {
        self.generation
    }

    pub const fn identity(&self) -> Option<&FileIdentity> {
        self.identity.as_ref()
    }
}
