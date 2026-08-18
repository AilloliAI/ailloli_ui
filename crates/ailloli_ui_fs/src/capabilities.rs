#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct FileCapabilities {
    pub read: bool,
    pub write: bool,
    pub create_dir: bool,
    pub rename: bool,
    pub remove: bool,
    pub copy_entry: bool,
    pub move_entry: bool,
    pub remove_recursive: bool,
    pub watch: bool,
}

impl FileCapabilities {
    pub const READ_ONLY: Self = Self {
        read: true,
        write: false,
        create_dir: false,
        rename: false,
        remove: false,
        copy_entry: false,
        move_entry: false,
        remove_recursive: false,
        watch: false,
    };

    pub const READ_WRITE: Self = Self {
        read: true,
        write: true,
        create_dir: true,
        rename: true,
        remove: true,
        copy_entry: true,
        move_entry: true,
        remove_recursive: true,
        watch: false,
    };
}

impl Default for FileCapabilities {
    fn default() -> Self {
        Self::READ_ONLY
    }
}
