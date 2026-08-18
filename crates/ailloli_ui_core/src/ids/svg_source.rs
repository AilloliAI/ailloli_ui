use std::hash::{Hash, Hasher};
use std::sync::Arc;

/// SVG bytes for GPU rasterization.
///
/// Equality and hashing use **pointer identity**, not byte content (for cache keys).
#[derive(Debug, Clone)]
pub enum SvgSource {
    /// Compile-time embedded SVG.
    Static(&'static [u8]),
    /// Heap-owned SVG bytes.
    Owned(Arc<[u8]>),
    /// SVG stored as UTF-8 string.
    Str(Arc<str>),
}

impl SvgSource {
    fn identity_key(&self) -> usize {
        match self {
            SvgSource::Static(bytes) => bytes.as_ptr().addr(),
            SvgSource::Owned(bytes) => Arc::as_ptr(bytes).addr(),
            SvgSource::Str(s) => Arc::as_ptr(s).addr(),
        }
    }

    /// Raw bytes for the SVG parser.
    pub fn as_bytes(&self) -> &[u8] {
        match self {
            SvgSource::Static(bytes) => bytes,
            SvgSource::Owned(bytes) => bytes,
            SvgSource::Str(s) => s.as_bytes(),
        }
    }
}

impl PartialEq for SvgSource {
    fn eq(&self, other: &Self) -> bool {
        self.identity_key() == other.identity_key()
    }
}

impl Eq for SvgSource {}

impl Hash for SvgSource {
    fn hash<H: Hasher>(&self, state: &mut H) {
        self.identity_key().hash(state);
    }
}
