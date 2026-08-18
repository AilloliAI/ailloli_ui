#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
pub struct DirtyFlags {
    pub layout: bool,
    pub paint: bool,
    pub input: bool,
}

impl DirtyFlags {
    pub const fn clean() -> Self {
        Self {
            layout: false,
            paint: false,
            input: false,
        }
    }

    pub const fn layout() -> Self {
        Self {
            layout: true,
            paint: true,
            input: false,
        }
    }

    pub const fn paint() -> Self {
        Self {
            layout: false,
            paint: true,
            input: false,
        }
    }

    pub const fn input() -> Self {
        Self {
            layout: false,
            paint: false,
            input: true,
        }
    }
}
