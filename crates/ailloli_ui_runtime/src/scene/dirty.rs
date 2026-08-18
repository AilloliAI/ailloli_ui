#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
pub struct DirtyFlags {
    pub paint: bool,
}

impl DirtyFlags {
    pub const fn clean() -> Self {
        Self { paint: false }
    }

    pub const fn paint() -> Self {
        Self { paint: true }
    }
}
