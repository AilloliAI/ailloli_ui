//! App-level storage paths, diagnostics, and small persisted documents.
//!
//! The canonical storage layout follows XDG directories. A home-directory
//! symlink view can be created as an optional user-facing convenience, but it
//! is never the source of truth.

use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::collections::BTreeMap;
use std::fs::{self, File};
use std::io::Write;
use std::path::{Path, PathBuf};

pub const APP_PREFERENCES_VERSION: u32 = 1;
pub const WINDOW_STATE_VERSION: u32 = 1;

pub type Result<T> = std::result::Result<T, AppStorageError>;

#[derive(Debug, thiserror::Error)]
pub enum AppStorageError {
    #[error("invalid app id `{0}`")]
    InvalidAppId(String),
    #[error("io error at {path:?}: {source}")]
    Io {
        path: Option<PathBuf>,
        #[source]
        source: std::io::Error,
    },
    #[error("json error: {0}")]
    Json(#[from] serde_json::Error),
    #[error("invalid storage document: {0}")]
    InvalidDocument(String),
    #[error("home symlink view collision at {path:?}: {reason}")]
    SymlinkCollision { path: PathBuf, reason: String },
    #[error("home symlink view unsupported: {0}")]
    SymlinkUnsupported(String),
}

impl AppStorageError {
    fn io(path: impl Into<Option<PathBuf>>, source: std::io::Error) -> Self {
        Self::Io {
            path: path.into(),
            source,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
pub struct AppId(String);

impl AppId {
    pub fn new(value: impl Into<String>) -> Result<Self> {
        let value = value.into();
        if !is_valid_app_id(&value) {
            return Err(AppStorageError::InvalidAppId(value));
        }
        Ok(Self(value))
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }

    pub fn env_prefix(&self) -> String {
        self.0
            .chars()
            .map(|ch| {
                if ch.is_ascii_alphanumeric() {
                    ch.to_ascii_uppercase()
                } else {
                    '_'
                }
            })
            .collect()
    }
}

fn is_valid_app_id(value: &str) -> bool {
    if value.is_empty() || value.starts_with('.') || value.starts_with('-') {
        return false;
    }
    if value.ends_with('-') || value.contains("--") {
        return false;
    }
    value
        .bytes()
        .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'-')
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub enum AppStorageMode {
    #[default]
    Xdg,
    SingleDir {
        root: PathBuf,
    },
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AppStorageDirs {
    pub config_dir: PathBuf,
    pub state_dir: PathBuf,
    pub data_dir: PathBuf,
    pub cache_dir: PathBuf,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HomeSymlinkView {
    pub enabled: bool,
    pub root: PathBuf,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HomeSymlinkViewStatus {
    pub enabled: bool,
    pub root: PathBuf,
    pub entries: Vec<HomeSymlinkEntryStatus>,
}

impl HomeSymlinkViewStatus {
    pub fn state(&self) -> HomeSymlinkViewState {
        if !self.enabled {
            return HomeSymlinkViewState::Disabled;
        }
        if self
            .entries
            .iter()
            .any(|entry| matches!(entry.state, HomeSymlinkEntryState::Collision { .. }))
        {
            return HomeSymlinkViewState::Collision;
        }
        if self
            .entries
            .iter()
            .any(|entry| matches!(entry.state, HomeSymlinkEntryState::Unsupported { .. }))
        {
            return HomeSymlinkViewState::Unsupported;
        }
        if self
            .entries
            .iter()
            .all(|entry| matches!(entry.state, HomeSymlinkEntryState::Ready))
        {
            HomeSymlinkViewState::Ready
        } else {
            HomeSymlinkViewState::Missing
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum HomeSymlinkViewState {
    Disabled,
    Missing,
    Ready,
    Collision,
    Unsupported,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HomeSymlinkEntryStatus {
    pub name: String,
    pub link: PathBuf,
    pub target: PathBuf,
    pub state: HomeSymlinkEntryState,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum HomeSymlinkEntryState {
    Missing,
    Ready,
    Created,
    Collision { reason: String },
    Unsupported { reason: String },
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct EnvOverride {
    pub key: String,
    pub path: PathBuf,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AppStorageDiagnostics {
    pub app_id: AppId,
    pub dirs: AppStorageDirs,
    pub env_overrides: Vec<EnvOverride>,
    pub home_symlink_view: HomeSymlinkViewStatus,
}

#[derive(Debug, Clone)]
pub struct AppStorageBuilder {
    app_id: String,
    mode: AppStorageMode,
    home_symlink_view: bool,
    home_symlink_root: Option<PathBuf>,
}

impl AppStorageBuilder {
    pub fn home_symlink_view(mut self, enabled: bool) -> Self {
        self.home_symlink_view = enabled;
        self
    }

    pub fn home_symlink_root(mut self, root: impl Into<PathBuf>) -> Self {
        self.home_symlink_root = Some(root.into());
        self
    }

    pub fn single_dir(mut self, root: impl Into<PathBuf>) -> Self {
        self.mode = AppStorageMode::SingleDir { root: root.into() };
        self
    }

    pub fn resolve(self) -> Result<AppStorage> {
        self.resolve_with_env(|key| std::env::var(key).ok())
    }

    pub fn resolve_with_env(
        mut self,
        get: impl FnMut(&str) -> Option<String>,
    ) -> Result<AppStorage> {
        let app_id = AppId::new(std::mem::take(&mut self.app_id))?;
        let mut resolver = EnvResolver::new(get);
        let home = resolver.get_path("HOME");
        let dirs = match &self.mode {
            AppStorageMode::Xdg => resolve_xdg_dirs(&app_id, &mut resolver, home.as_deref()),
            AppStorageMode::SingleDir { root } => {
                let root = expand_home(root.clone(), home.as_deref());
                AppStorageDirs {
                    config_dir: root.join("config"),
                    state_dir: root.join("state"),
                    data_dir: root.join("data"),
                    cache_dir: root.join("cache"),
                }
            }
        };
        let home_symlink_root = self
            .home_symlink_root
            .map(|root| expand_home(root, home.as_deref()))
            .or_else(|| {
                home.as_ref()
                    .map(|home| home.join(format!(".{}", app_id.as_str())))
            })
            .unwrap_or_else(|| std::env::temp_dir().join(format!(".{}", app_id.as_str())));
        let home_symlink_view = HomeSymlinkView {
            enabled: self.home_symlink_view,
            root: home_symlink_root,
        };
        let storage = AppStorage {
            app_id: app_id.clone(),
            mode: self.mode,
            dirs: dirs.clone(),
            home_symlink_view,
            env_overrides: resolver.overrides,
        };
        Ok(storage)
    }
}

struct EnvResolver<F> {
    get: F,
    overrides: Vec<EnvOverride>,
}

impl<F> EnvResolver<F>
where
    F: FnMut(&str) -> Option<String>,
{
    fn new(get: F) -> Self {
        Self {
            get,
            overrides: Vec::new(),
        }
    }

    fn get(&mut self, key: &str) -> Option<String> {
        (self.get)(key).filter(|value| !value.is_empty())
    }

    fn get_path(&mut self, key: &str) -> Option<PathBuf> {
        self.get(key).map(PathBuf::from)
    }

    fn get_override_path(&mut self, key: &str) -> Option<PathBuf> {
        let path = self.get_path(key)?;
        self.overrides.push(EnvOverride {
            key: key.to_string(),
            path: path.clone(),
        });
        Some(path)
    }
}

fn resolve_xdg_dirs<F>(
    app_id: &AppId,
    resolver: &mut EnvResolver<F>,
    home: Option<&Path>,
) -> AppStorageDirs
where
    F: FnMut(&str) -> Option<String>,
{
    let prefix = app_id.env_prefix();
    let app = app_id.as_str();
    let config_dir = resolver
        .get_override_path(&format!("{prefix}_CONFIG_DIR"))
        .or_else(|| {
            resolver
                .get_path("XDG_CONFIG_HOME")
                .map(|path| path.join(app))
        })
        .or_else(|| home.map(|home| home.join(".config").join(app)))
        .unwrap_or_else(|| std::env::temp_dir().join(format!("{app}-config")));
    let state_dir = resolver
        .get_override_path(&format!("{prefix}_STATE_DIR"))
        .or_else(|| {
            resolver
                .get_path("XDG_STATE_HOME")
                .map(|path| path.join(app))
        })
        .or_else(|| home.map(|home| home.join(".local/state").join(app)))
        .unwrap_or_else(|| std::env::temp_dir().join(format!("{app}-state")));
    let data_dir = resolver
        .get_override_path(&format!("{prefix}_DATA_DIR"))
        .or_else(|| {
            resolver
                .get_path("XDG_DATA_HOME")
                .map(|path| path.join(app))
        })
        .or_else(|| home.map(|home| home.join(".local/share").join(app)))
        .unwrap_or_else(|| std::env::temp_dir().join(format!("{app}-data")));
    let cache_dir = resolver
        .get_override_path(&format!("{prefix}_CACHE_DIR"))
        .or_else(|| {
            resolver
                .get_path("XDG_CACHE_HOME")
                .map(|path| path.join(app))
        })
        .or_else(|| home.map(|home| home.join(".cache").join(app)))
        .unwrap_or_else(|| std::env::temp_dir().join(format!("{app}-cache")));
    AppStorageDirs {
        config_dir,
        state_dir,
        data_dir,
        cache_dir,
    }
}

fn expand_home(path: PathBuf, home: Option<&Path>) -> PathBuf {
    let value = path.to_string_lossy();
    if value == "~" {
        return home.map(Path::to_path_buf).unwrap_or(path);
    }
    if let Some(rest) = value.strip_prefix("~/") {
        return home
            .map(|home| home.join(rest))
            .unwrap_or_else(|| PathBuf::from(value.as_ref()));
    }
    path
}

#[derive(Debug, Clone)]
pub struct AppStorage {
    app_id: AppId,
    mode: AppStorageMode,
    dirs: AppStorageDirs,
    home_symlink_view: HomeSymlinkView,
    env_overrides: Vec<EnvOverride>,
}

impl AppStorage {
    pub fn for_app(app_id: impl Into<String>) -> AppStorageBuilder {
        AppStorageBuilder {
            app_id: app_id.into(),
            mode: AppStorageMode::Xdg,
            home_symlink_view: false,
            home_symlink_root: None,
        }
    }

    pub fn single_dir(app_id: impl Into<String>, root: impl Into<PathBuf>) -> AppStorageBuilder {
        Self::for_app(app_id).single_dir(root)
    }

    pub fn app_id(&self) -> &AppId {
        &self.app_id
    }

    pub fn mode(&self) -> &AppStorageMode {
        &self.mode
    }

    pub fn dirs(&self) -> &AppStorageDirs {
        &self.dirs
    }

    pub fn config_dir(&self) -> &Path {
        &self.dirs.config_dir
    }

    pub fn state_dir(&self) -> &Path {
        &self.dirs.state_dir
    }

    pub fn data_dir(&self) -> &Path {
        &self.dirs.data_dir
    }

    pub fn cache_dir(&self) -> &Path {
        &self.dirs.cache_dir
    }

    pub fn preferences_file(&self) -> PathBuf {
        self.config_dir().join("preferences.json")
    }

    pub fn window_state_file(&self) -> PathBuf {
        self.state_dir().join("windows.json")
    }

    pub fn diagnostics(&self) -> AppStorageDiagnostics {
        AppStorageDiagnostics {
            app_id: self.app_id.clone(),
            dirs: self.dirs.clone(),
            env_overrides: self.env_overrides.clone(),
            home_symlink_view: self.home_symlink_view_status(),
        }
    }

    pub fn home_symlink_view_status(&self) -> HomeSymlinkViewStatus {
        let entries = self
            .symlink_entries()
            .into_iter()
            .map(|(name, link, target)| HomeSymlinkEntryStatus {
                name,
                state: inspect_symlink_entry(&link, &target),
                link,
                target,
            })
            .collect();
        HomeSymlinkViewStatus {
            enabled: self.home_symlink_view.enabled,
            root: self.home_symlink_view.root.clone(),
            entries,
        }
    }

    pub fn ensure_home_symlink_view(&self) -> Result<HomeSymlinkViewStatus> {
        if !self.home_symlink_view.enabled {
            return Ok(self.home_symlink_view_status());
        }
        match fs::symlink_metadata(&self.home_symlink_view.root) {
            Ok(metadata) if !metadata.is_dir() => {
                return Err(AppStorageError::SymlinkCollision {
                    path: self.home_symlink_view.root.clone(),
                    reason: "root exists and is not a directory".into(),
                });
            }
            Ok(_) => {}
            Err(err) if err.kind() == std::io::ErrorKind::NotFound => {
                fs::create_dir_all(&self.home_symlink_view.root).map_err(|err| {
                    AppStorageError::io(Some(self.home_symlink_view.root.clone()), err)
                })?;
            }
            Err(err) => {
                return Err(AppStorageError::io(
                    Some(self.home_symlink_view.root.clone()),
                    err,
                ));
            }
        }
        for (_, link, target) in self.symlink_entries() {
            fs::create_dir_all(&target)
                .map_err(|err| AppStorageError::io(Some(target.clone()), err))?;
            match inspect_symlink_entry(&link, &target) {
                HomeSymlinkEntryState::Ready => {}
                HomeSymlinkEntryState::Missing => create_dir_symlink(&target, &link)?,
                HomeSymlinkEntryState::Collision { reason } => {
                    return Err(AppStorageError::SymlinkCollision { path: link, reason });
                }
                HomeSymlinkEntryState::Unsupported { reason } => {
                    return Err(AppStorageError::SymlinkUnsupported(reason));
                }
                HomeSymlinkEntryState::Created => {}
            }
        }
        Ok(self.home_symlink_view_status())
    }

    pub fn read_preferences(&self) -> Result<Option<AppPreferencesDocument>> {
        read_optional_json(self.preferences_file()).and_then(|doc| match doc {
            Some(doc) => validate_preferences(doc).map(Some),
            None => Ok(None),
        })
    }

    pub fn write_preferences(&self, preferences: &AppPreferencesDocument) -> Result<()> {
        write_json_atomic(self.preferences_file(), preferences)
    }

    pub fn read_window_state(&self) -> Result<Option<WindowStateDocument>> {
        read_optional_json(self.window_state_file()).and_then(|doc| match doc {
            Some(doc) => validate_window_state(doc).map(Some),
            None => Ok(None),
        })
    }

    pub fn write_window_state(&self, document: &WindowStateDocument) -> Result<()> {
        write_json_atomic(self.window_state_file(), document)
    }

    fn symlink_entries(&self) -> Vec<(String, PathBuf, PathBuf)> {
        vec![
            (
                "config".into(),
                self.home_symlink_view.root.join("config"),
                self.dirs.config_dir.clone(),
            ),
            (
                "state".into(),
                self.home_symlink_view.root.join("state"),
                self.dirs.state_dir.clone(),
            ),
            (
                "data".into(),
                self.home_symlink_view.root.join("data"),
                self.dirs.data_dir.clone(),
            ),
            (
                "cache".into(),
                self.home_symlink_view.root.join("cache"),
                self.dirs.cache_dir.clone(),
            ),
        ]
    }
}

fn inspect_symlink_entry(link: &Path, target: &Path) -> HomeSymlinkEntryState {
    match fs::symlink_metadata(link) {
        Ok(metadata) if metadata.file_type().is_symlink() => match fs::read_link(link) {
            Ok(existing) if existing == target => HomeSymlinkEntryState::Ready,
            Ok(existing) => HomeSymlinkEntryState::Collision {
                reason: format!(
                    "points to `{}`, expected `{}`",
                    existing.display(),
                    target.display()
                ),
            },
            Err(err) => HomeSymlinkEntryState::Collision {
                reason: err.to_string(),
            },
        },
        Ok(_) => HomeSymlinkEntryState::Collision {
            reason: "path exists and is not a symlink".into(),
        },
        Err(err) if err.kind() == std::io::ErrorKind::NotFound => HomeSymlinkEntryState::Missing,
        Err(err) => HomeSymlinkEntryState::Collision {
            reason: err.to_string(),
        },
    }
}

#[cfg(unix)]
fn create_dir_symlink(target: &Path, link: &Path) -> Result<()> {
    std::os::unix::fs::symlink(target, link)
        .map_err(|err| AppStorageError::io(Some(link.to_path_buf()), err))
}

#[cfg(windows)]
fn create_dir_symlink(target: &Path, link: &Path) -> Result<()> {
    std::os::windows::fs::symlink_dir(target, link)
        .map_err(|err| AppStorageError::io(Some(link.to_path_buf()), err))
}

#[cfg(not(any(unix, windows)))]
fn create_dir_symlink(_target: &Path, _link: &Path) -> Result<()> {
    Err(AppStorageError::SymlinkUnsupported(
        "directory symlinks are not available on this platform".into(),
    ))
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct AppPreferencesDocument {
    pub version: u32,
    pub values: BTreeMap<String, Value>,
}

impl AppPreferencesDocument {
    pub fn new() -> Self {
        Self {
            version: APP_PREFERENCES_VERSION,
            values: BTreeMap::new(),
        }
    }
}

impl Default for AppPreferencesDocument {
    fn default() -> Self {
        Self::new()
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct WindowStateDocument {
    pub version: u32,
    pub windows: Vec<WindowSnapshot>,
}

impl WindowStateDocument {
    pub fn new(windows: Vec<WindowSnapshot>) -> Self {
        Self {
            version: WINDOW_STATE_VERSION,
            windows,
        }
    }

    pub fn empty() -> Self {
        Self::new(Vec::new())
    }

    pub fn snapshot_for(&self, window_id: &str) -> Option<&WindowSnapshot> {
        self.windows
            .iter()
            .find(|snapshot| snapshot.window_id == window_id)
    }
}

impl Default for WindowStateDocument {
    fn default() -> Self {
        Self::empty()
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct WindowSnapshot {
    pub window_id: String,
    pub inner_size: Option<LogicalWindowSize>,
    pub maximized: bool,
    pub fullscreen: bool,
    pub position: Option<LogicalWindowPosition>,
}

impl WindowSnapshot {
    pub fn new(window_id: impl Into<String>) -> Self {
        Self {
            window_id: window_id.into(),
            inner_size: None,
            maximized: false,
            fullscreen: false,
            position: None,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct LogicalWindowSize {
    pub width: f64,
    pub height: f64,
}

impl LogicalWindowSize {
    pub fn new(width: f64, height: f64) -> Self {
        Self {
            width: width.max(1.0),
            height: height.max(1.0),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct LogicalWindowPosition {
    pub x: f64,
    pub y: f64,
}

impl LogicalWindowPosition {
    pub fn new(x: f64, y: f64) -> Self {
        Self { x, y }
    }
}

fn validate_preferences(document: AppPreferencesDocument) -> Result<AppPreferencesDocument> {
    if document.version != APP_PREFERENCES_VERSION {
        return Err(AppStorageError::InvalidDocument(format!(
            "unsupported preferences version {}",
            document.version
        )));
    }
    Ok(document)
}

fn validate_window_state(document: WindowStateDocument) -> Result<WindowStateDocument> {
    if document.version != WINDOW_STATE_VERSION {
        return Err(AppStorageError::InvalidDocument(format!(
            "unsupported window state version {}",
            document.version
        )));
    }
    Ok(document)
}

pub fn read_optional_json<T>(path: impl AsRef<Path>) -> Result<Option<T>>
where
    T: for<'de> Deserialize<'de>,
{
    let path = path.as_ref();
    let bytes = match fs::read(path) {
        Ok(bytes) => bytes,
        Err(err) if err.kind() == std::io::ErrorKind::NotFound => return Ok(None),
        Err(err) => return Err(AppStorageError::io(Some(path.to_path_buf()), err)),
    };
    Ok(Some(serde_json::from_slice(&bytes)?))
}

pub fn write_json_atomic<T>(path: impl AsRef<Path>, value: &T) -> Result<()>
where
    T: Serialize,
{
    let bytes = serde_json::to_vec_pretty(value)?;
    atomic_write_bytes(path.as_ref(), &bytes)
}

pub fn atomic_write_bytes(path: &Path, bytes: &[u8]) -> Result<()> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)
            .map_err(|err| AppStorageError::io(Some(parent.to_path_buf()), err))?;
    }
    let tmp_path = temp_path_for(path)?;
    {
        let mut file = File::create(&tmp_path)
            .map_err(|err| AppStorageError::io(Some(tmp_path.clone()), err))?;
        file.write_all(bytes)
            .map_err(|err| AppStorageError::io(Some(tmp_path.clone()), err))?;
        file.sync_all()
            .map_err(|err| AppStorageError::io(Some(tmp_path.clone()), err))?;
    }
    fs::rename(&tmp_path, path)
        .map_err(|err| AppStorageError::io(Some(path.to_path_buf()), err))?;
    if let Some(parent) = path.parent() {
        if let Ok(dir) = File::open(parent) {
            let _ = dir.sync_all();
        }
    }
    Ok(())
}

fn temp_path_for(path: &Path) -> Result<PathBuf> {
    let file_name = path
        .file_name()
        .and_then(|value| value.to_str())
        .ok_or_else(|| AppStorageError::InvalidDocument(path.display().to_string()))?;
    Ok(path.with_file_name(format!("{file_name}.tmp")))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::{SystemTime, UNIX_EPOCH};

    fn temp_dir(name: &str) -> PathBuf {
        let nanos = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("clock")
            .as_nanos();
        let dir = std::env::temp_dir().join(format!(
            "ailloli_ui_app_storage_{name}_{}_{}",
            std::process::id(),
            nanos
        ));
        fs::create_dir_all(&dir).expect("create temp dir");
        dir
    }

    #[test]
    fn app_id_validation_is_filename_safe() {
        assert_eq!(AppId::new("my-app").unwrap().env_prefix(), "MY_APP");
        assert!(AppId::new(".my-app").is_err());
        assert!(AppId::new("MyApp").is_err());
        assert!(AppId::new("my/app").is_err());
        assert!(AppId::new("my--app").is_err());
    }

    #[test]
    fn xdg_dirs_use_expected_fallbacks() {
        let home = temp_dir("home");
        let storage = AppStorage::for_app("my-app")
            .resolve_with_env(|key| {
                if key == "HOME" {
                    Some(home.display().to_string())
                } else {
                    None
                }
            })
            .unwrap();
        assert_eq!(storage.config_dir(), home.join(".config/my-app"));
        assert_eq!(storage.state_dir(), home.join(".local/state/my-app"));
        assert_eq!(storage.data_dir(), home.join(".local/share/my-app"));
        assert_eq!(storage.cache_dir(), home.join(".cache/my-app"));
    }

    #[test]
    fn env_overrides_take_priority() {
        let storage = AppStorage::for_app("my-app")
            .resolve_with_env(|key| match key {
                "MY_APP_STATE_DIR" => Some("/override/state".into()),
                "XDG_STATE_HOME" => Some("/xdg/state".into()),
                "HOME" => Some("/tmp/ailloli-ui-home".into()),
                _ => None,
            })
            .unwrap();
        assert_eq!(storage.state_dir(), Path::new("/override/state"));
        assert_eq!(
            storage.diagnostics().env_overrides[0].key,
            "MY_APP_STATE_DIR"
        );
    }

    #[test]
    fn single_dir_mode_uses_explicit_root() {
        let root = temp_dir("single");
        let storage = AppStorage::single_dir("my-app", root.join("store"))
            .resolve_with_env(|_| None)
            .unwrap();
        assert_eq!(storage.config_dir(), root.join("store/config"));
        assert_eq!(storage.state_dir(), root.join("store/state"));
    }

    #[cfg(any(unix, windows))]
    #[test]
    fn symlink_view_creates_non_destructive_links() {
        let home = temp_dir("symlink_home");
        let storage = AppStorage::for_app("my-app")
            .home_symlink_view(true)
            .resolve_with_env(|key| {
                if key == "HOME" {
                    Some(home.display().to_string())
                } else {
                    None
                }
            })
            .unwrap();
        let status = storage.ensure_home_symlink_view().unwrap();
        assert_eq!(status.state(), HomeSymlinkViewState::Ready);
        assert_eq!(
            fs::read_link(home.join(".my-app/state")).unwrap(),
            home.join(".local/state/my-app")
        );
    }

    #[test]
    fn symlink_view_reports_collisions() {
        let home = temp_dir("symlink_collision");
        fs::create_dir_all(home.join(".my-app")).unwrap();
        fs::write(home.join(".my-app/config"), "not a symlink").unwrap();
        let storage = AppStorage::for_app("my-app")
            .home_symlink_view(true)
            .resolve_with_env(|key| {
                if key == "HOME" {
                    Some(home.display().to_string())
                } else {
                    None
                }
            })
            .unwrap();
        assert!(matches!(
            storage.ensure_home_symlink_view(),
            Err(AppStorageError::SymlinkCollision { .. })
        ));
    }

    #[test]
    fn preferences_and_window_state_roundtrip() {
        let home = temp_dir("docs");
        let storage = AppStorage::for_app("my-app")
            .resolve_with_env(|key| {
                if key == "HOME" {
                    Some(home.display().to_string())
                } else {
                    None
                }
            })
            .unwrap();
        let mut prefs = AppPreferencesDocument::new();
        prefs
            .values
            .insert("theme".into(), Value::String("dark".into()));
        storage.write_preferences(&prefs).unwrap();
        assert_eq!(storage.read_preferences().unwrap(), Some(prefs));

        let mut snapshot = WindowSnapshot::new("main");
        snapshot.inner_size = Some(LogicalWindowSize::new(800.0, 600.0));
        snapshot.maximized = true;
        let windows = WindowStateDocument::new(vec![snapshot.clone()]);
        storage.write_window_state(&windows).unwrap();
        let read = storage.read_window_state().unwrap().unwrap();
        assert_eq!(read.snapshot_for("main"), Some(&snapshot));
    }
}
