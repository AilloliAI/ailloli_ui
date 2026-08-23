//! App-level storage paths, diagnostics, and small persisted documents.
//!
//! The canonical storage layout follows XDG directories. A home-directory
//! symlink view can be created as an optional user-facing convenience, but it
//! is never the source of truth.
//!
//! # Examples
//!
//! ```
//! use ailloli_ui_app_storage::AppStorage;
//!
//! let storage = AppStorage::for_app("example-app")
//!     .resolve_with_env(|key| (key == "HOME").then(|| "/var/empty/example-user".into()))?;
//! assert_eq!(storage.config_dir().to_string_lossy(), "/var/empty/example-user/.config/example-app");
//! # Ok::<(), ailloli_ui_app_storage::AppStorageError>(())
//! ```

use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::collections::BTreeMap;
use std::fs::{self, File};
use std::io::Write;
use std::path::{Path, PathBuf};

/// Current [`AppPreferencesDocument`] schema version written by this crate.
///
/// # Examples
///
/// ```
/// use ailloli_ui_app_storage::{AppPreferencesDocument, APP_PREFERENCES_VERSION};
/// assert_eq!(AppPreferencesDocument::new().version, APP_PREFERENCES_VERSION);
/// ```
pub const APP_PREFERENCES_VERSION: u32 = 1;

/// Current [`WindowStateDocument`] schema version written by this crate.
///
/// # Examples
///
/// ```
/// use ailloli_ui_app_storage::{WindowStateDocument, WINDOW_STATE_VERSION};
/// assert_eq!(WindowStateDocument::empty().version, WINDOW_STATE_VERSION);
/// ```
pub const WINDOW_STATE_VERSION: u32 = 1;

/// Result type returned by storage operations in this crate.
///
/// # Examples
///
/// ```
/// use ailloli_ui_app_storage::{AppId, Result};
/// let id: Result<AppId> = AppId::new("example-app");
/// assert!(id.is_ok());
/// ```
pub type Result<T> = std::result::Result<T, AppStorageError>;

/// Failure produced while resolving paths, validating documents, or touching storage.
///
/// # Examples
///
/// ```
/// use ailloli_ui_app_storage::{AppId, AppStorageError};
/// assert!(matches!(AppId::new("Invalid"), Err(AppStorageError::InvalidAppId(_))));
/// ```
#[derive(Debug, thiserror::Error)]
pub enum AppStorageError {
    /// The application identifier violates [`AppId::new`] rules.
    #[error("invalid app id `{0}`")]
    InvalidAppId(String),
    /// A filesystem operation failed, optionally at the supplied path.
    #[error("io error at {path:?}: {source}")]
    Io {
        /// Path associated with the operation, or `None` when unavailable.
        path: Option<PathBuf>,
        /// Operating-system I/O error returned by the filesystem.
        #[source]
        source: std::io::Error,
    },
    /// JSON serialization or deserialization failed.
    #[error("json error: {0}")]
    Json(#[from] serde_json::Error),
    /// A persisted document or atomic-write destination is structurally unsupported.
    #[error("invalid storage document: {0}")]
    InvalidDocument(String),
    /// A requested compatibility symlink would replace or disagree with an existing path.
    #[error("home symlink view collision at {path:?}: {reason}")]
    SymlinkCollision {
        /// Existing link or root path that prevented creation.
        path: PathBuf,
        /// Human-readable collision detail.
        reason: String,
    },
    /// Directory symlinks are unavailable on the current platform or provider.
    #[error("home symlink view unsupported: {0}")]
    SymlinkUnsupported(String),
}

impl AppStorageError {
    /// Wraps one I/O failure while retaining its optional path context.
    fn io(path: impl Into<Option<PathBuf>>, source: std::io::Error) -> Self {
        Self::Io {
            path: path.into(),
            source,
        }
    }
}

/// Filename-safe application identifier used as the leaf of storage paths.
///
/// Valid constructor input is non-empty lowercase ASCII letters, digits, and
/// single hyphens. It cannot begin or end with a hyphen and cannot contain
/// consecutive hyphens. Derived deserialization does not call [`Self::new`],
/// so consumers of untrusted serialized `AppId` values must validate them.
///
/// # Examples
///
/// ```
/// use ailloli_ui_app_storage::AppId;
/// assert_eq!(AppId::new("demo-2")?.as_str(), "demo-2");
/// assert!(AppId::new("Demo_2").is_err());
/// # Ok::<(), ailloli_ui_app_storage::AppStorageError>(())
/// ```
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
pub struct AppId(String);

impl AppId {
    /// Validates and stores an application identifier.
    ///
    /// # Errors
    ///
    /// Returns [`AppStorageError::InvalidAppId`] for empty input, non-lowercase
    /// ASCII letters, punctuation other than internal single hyphens, leading
    /// or trailing hyphens, or consecutive hyphens.
    ///
    /// # Examples
    ///
    /// ```
    /// use ailloli_ui_app_storage::AppId;
    /// assert!(AppId::new("2d-editor").is_ok());
    /// assert!(AppId::new("-editor").is_err());
    /// ```
    pub fn new(value: impl Into<String>) -> Result<Self> {
        let value = value.into();
        if !is_valid_app_id(&value) {
            return Err(AppStorageError::InvalidAppId(value));
        }
        Ok(Self(value))
    }

    /// Borrows the stored identifier.
    ///
    /// # Examples
    ///
    /// ```
    /// use ailloli_ui_app_storage::AppId;
    /// assert_eq!(AppId::new("example")?.as_str(), "example");
    /// # Ok::<(), ailloli_ui_app_storage::AppStorageError>(())
    /// ```
    pub fn as_str(&self) -> &str {
        &self.0
    }

    /// Returns the uppercase prefix used for app-specific environment variables.
    ///
    /// ASCII alphanumeric characters are uppercased and every other character,
    /// including the normally valid hyphen, becomes `_`.
    ///
    /// # Examples
    ///
    /// ```
    /// use ailloli_ui_app_storage::AppId;
    /// assert_eq!(AppId::new("my-app")?.env_prefix(), "MY_APP");
    /// # Ok::<(), ailloli_ui_app_storage::AppStorageError>(())
    /// ```
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

/// Checks the complete portable application-identifier grammar.
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

/// Strategy used to resolve the four canonical storage directories.
///
/// # Examples
///
/// ```
/// use ailloli_ui_app_storage::AppStorageMode;
/// assert_eq!(AppStorageMode::default(), AppStorageMode::Xdg);
/// ```
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub enum AppStorageMode {
    /// Follow app-specific overrides, XDG homes, conventional home paths, then temp fallbacks.
    #[default]
    Xdg,
    /// Place `config`, `state`, `data`, and `cache` below one explicit root.
    SingleDir {
        /// Parent directory of the four category directories.
        root: PathBuf,
    },
}

/// Resolved canonical storage directories for one application.
///
/// No directory is created merely by constructing this value or resolving an
/// [`AppStorage`].
///
/// # Examples
///
/// ```
/// use ailloli_ui_app_storage::AppStorage;
/// let storage = AppStorage::single_dir("example", "/tmp/example-store").resolve_with_env(|_| None)?;
/// assert_eq!(storage.dirs().state_dir.to_string_lossy(), "/tmp/example-store/state");
/// # Ok::<(), ailloli_ui_app_storage::AppStorageError>(())
/// ```
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AppStorageDirs {
    /// Directory containing user preferences and configuration.
    pub config_dir: PathBuf,
    /// Directory containing restorable application/window state.
    pub state_dir: PathBuf,
    /// Directory containing durable application data.
    pub data_dir: PathBuf,
    /// Directory containing disposable cached data.
    pub cache_dir: PathBuf,
}

/// Optional human-facing directory whose entries link to canonical storage.
///
/// This view never becomes the source of truth and is disabled by default.
///
/// # Examples
///
/// ```
/// use ailloli_ui_app_storage::HomeSymlinkView;
/// let view = HomeSymlinkView { enabled: false, root: "/tmp/.example".into() };
/// assert!(!view.enabled);
/// ```
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HomeSymlinkView {
    /// Whether [`AppStorage::ensure_home_symlink_view`] may create the view.
    pub enabled: bool,
    /// Directory containing the `config`, `state`, `data`, and `cache` links.
    pub root: PathBuf,
}

/// Filesystem inspection result for the optional symlink view.
///
/// # Examples
///
/// ```
/// use ailloli_ui_app_storage::{HomeSymlinkViewState, HomeSymlinkViewStatus};
/// let status = HomeSymlinkViewStatus { enabled: false, root: "/tmp/.example".into(), entries: vec![] };
/// assert_eq!(status.state(), HomeSymlinkViewState::Disabled);
/// ```
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HomeSymlinkViewStatus {
    /// Configured enablement; disabled status takes precedence over entry states.
    pub enabled: bool,
    /// Inspected compatibility-view root.
    pub root: PathBuf,
    /// Per-category results in `config`, `state`, `data`, `cache` order.
    pub entries: Vec<HomeSymlinkEntryStatus>,
}

impl HomeSymlinkViewStatus {
    /// Reduces per-entry results to one view state.
    ///
    /// Precedence is disabled, collision, unsupported, all-ready, then missing.
    /// An enabled empty entry list is vacuously ready; statuses produced by
    /// [`AppStorage`] contain four entries.
    ///
    /// # Examples
    ///
    /// ```
    /// use ailloli_ui_app_storage::{HomeSymlinkViewState, HomeSymlinkViewStatus};
    /// let status = HomeSymlinkViewStatus { enabled: true, root: "/tmp/.example".into(), entries: vec![] };
    /// assert_eq!(status.state(), HomeSymlinkViewState::Ready);
    /// ```
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

/// Aggregate readiness of the optional symlink view.
///
/// # Examples
///
/// ```
/// use ailloli_ui_app_storage::HomeSymlinkViewState;
/// assert_ne!(HomeSymlinkViewState::Missing, HomeSymlinkViewState::Ready);
/// ```
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum HomeSymlinkViewState {
    /// Creation is disabled regardless of existing paths.
    Disabled,
    /// At least one expected link does not exist.
    Missing,
    /// Every expected link points to its canonical directory.
    Ready,
    /// An existing path would be overwritten or points elsewhere.
    Collision,
    /// A platform/provider cannot represent the requested directory links.
    Unsupported,
}

/// Inspection result for one named compatibility link.
///
/// # Examples
///
/// ```
/// use ailloli_ui_app_storage::{HomeSymlinkEntryState, HomeSymlinkEntryStatus};
/// let entry = HomeSymlinkEntryStatus { name: "state".into(), link: "/tmp/view/state".into(), target: "/tmp/data/state".into(), state: HomeSymlinkEntryState::Missing };
/// assert_eq!(entry.name, "state");
/// ```
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HomeSymlinkEntryStatus {
    /// Stable category name: `config`, `state`, `data`, or `cache`.
    pub name: String,
    /// Compatibility path inspected as a possible symlink.
    pub link: PathBuf,
    /// Canonical directory the link must reference exactly.
    pub target: PathBuf,
    /// Result of inspecting `link` against `target`.
    pub state: HomeSymlinkEntryState,
}

/// State of one compatibility link.
///
/// # Examples
///
/// ```
/// use ailloli_ui_app_storage::HomeSymlinkEntryState;
/// assert!(matches!(HomeSymlinkEntryState::Missing, HomeSymlinkEntryState::Missing));
/// ```
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum HomeSymlinkEntryState {
    /// No filesystem entry exists at the link path.
    Missing,
    /// The path is a symlink whose stored target exactly equals the canonical target.
    Ready,
    /// Creation succeeded; final statuses returned by the current implementation re-inspect as ready.
    Created,
    /// An existing path cannot be reused without destructive replacement.
    Collision {
        /// Human-readable mismatch or filesystem inspection failure.
        reason: String,
    },
    /// The platform/provider cannot represent this entry.
    Unsupported {
        /// Human-readable capability limitation.
        reason: String,
    },
}

/// App-specific environment path that took precedence during resolution.
///
/// Generic `XDG_*_HOME` and `HOME` inputs are intentionally not recorded here.
///
/// # Examples
///
/// ```
/// use ailloli_ui_app_storage::EnvOverride;
/// let value = EnvOverride { key: "MY_APP_STATE_DIR".into(), path: "/state".into() };
/// assert_eq!(value.key, "MY_APP_STATE_DIR");
/// ```
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct EnvOverride {
    /// Environment variable name that supplied the path.
    pub key: String,
    /// Non-empty path value used verbatim.
    pub path: PathBuf,
}

/// Snapshot of resolved paths, overrides, and compatibility-view readiness.
///
/// Calling [`AppStorage::diagnostics`] inspects symlink paths but creates or
/// modifies nothing.
///
/// # Examples
///
/// ```
/// use ailloli_ui_app_storage::AppStorage;
/// let storage = AppStorage::single_dir("example", "/tmp/example").resolve_with_env(|_| None)?;
/// assert_eq!(storage.diagnostics().app_id.as_str(), "example");
/// # Ok::<(), ailloli_ui_app_storage::AppStorageError>(())
/// ```
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AppStorageDiagnostics {
    /// Validated application identifier.
    pub app_id: AppId,
    /// Four canonical directories selected by resolution.
    pub dirs: AppStorageDirs,
    /// App-specific non-empty environment overrides that were consumed.
    pub env_overrides: Vec<EnvOverride>,
    /// Current read-only inspection of the optional compatibility view.
    pub home_symlink_view: HomeSymlinkViewStatus,
}

/// Configures storage resolution without accessing the filesystem.
///
/// # Examples
///
/// ```
/// use ailloli_ui_app_storage::AppStorage;
/// let builder = AppStorage::for_app("example").home_symlink_view(false);
/// let storage = builder.resolve_with_env(|_| None)?;
/// assert_eq!(storage.app_id().as_str(), "example");
/// # Ok::<(), ailloli_ui_app_storage::AppStorageError>(())
/// ```
#[derive(Debug, Clone)]
pub struct AppStorageBuilder {
    /// Unvalidated application identifier supplied by the caller.
    app_id: String,
    /// XDG or explicit single-directory resolution policy.
    mode: AppStorageMode,
    /// Whether resolution should prepare the optional home symlink view.
    home_symlink_view: bool,
    /// Optional caller-selected root for that compatibility view.
    home_symlink_root: Option<PathBuf>,
}

impl AppStorageBuilder {
    /// Enables or disables creation of the optional compatibility symlink view.
    ///
    /// The default is `false`. This setting alone performs no filesystem work.
    ///
    /// # Examples
    ///
    /// ```
    /// use ailloli_ui_app_storage::{AppStorage, HomeSymlinkViewState};
    /// let storage = AppStorage::for_app("example").home_symlink_view(false).resolve_with_env(|_| None)?;
    /// assert_eq!(storage.home_symlink_view_status().state(), HomeSymlinkViewState::Disabled);
    /// # Ok::<(), ailloli_ui_app_storage::AppStorageError>(())
    /// ```
    pub fn home_symlink_view(mut self, enabled: bool) -> Self {
        self.home_symlink_view = enabled;
        self
    }

    /// Overrides the compatibility-view root.
    ///
    /// Exact `~` and `~/...` prefixes expand from a non-empty `HOME`; without
    /// one, the path remains textual. This does not enable or create the view.
    ///
    /// # Examples
    ///
    /// ```
    /// use ailloli_ui_app_storage::AppStorage;
    /// let storage = AppStorage::for_app("example").home_symlink_root("~/view").resolve_with_env(|key| (key == "HOME").then(|| "/var/empty/example-user".into()))?;
    /// assert_eq!(storage.home_symlink_view_status().root.to_string_lossy(), "/var/empty/example-user/view");
    /// # Ok::<(), ailloli_ui_app_storage::AppStorageError>(())
    /// ```
    pub fn home_symlink_root(mut self, root: impl Into<PathBuf>) -> Self {
        self.home_symlink_root = Some(root.into());
        self
    }

    /// Switches to a single explicit root with four category subdirectories.
    ///
    /// Exact `~`/`~/...` expansion occurs during resolution. Repeated calls use
    /// the last root.
    ///
    /// # Examples
    ///
    /// ```
    /// use ailloli_ui_app_storage::AppStorage;
    /// let storage = AppStorage::for_app("example").single_dir("/store").resolve_with_env(|_| None)?;
    /// assert_eq!(storage.cache_dir().to_string_lossy(), "/store/cache");
    /// # Ok::<(), ailloli_ui_app_storage::AppStorageError>(())
    /// ```
    pub fn single_dir(mut self, root: impl Into<PathBuf>) -> Self {
        self.mode = AppStorageMode::SingleDir { root: root.into() };
        self
    }

    /// Resolves paths from the current process environment without creating them.
    ///
    /// Empty environment values are treated as absent.
    ///
    /// # Errors
    ///
    /// Returns [`AppStorageError::InvalidAppId`] if the builder's identifier is invalid.
    ///
    /// # Examples
    ///
    /// ```
    /// use ailloli_ui_app_storage::AppStorage;
    /// let storage = AppStorage::for_app("example").resolve()?;
    /// assert_eq!(storage.app_id().as_str(), "example");
    /// # Ok::<(), ailloli_ui_app_storage::AppStorageError>(())
    /// ```
    pub fn resolve(self) -> Result<AppStorage> {
        self.resolve_with_env(|key| std::env::var(key).ok())
    }

    /// Resolves paths using a caller-supplied environment lookup.
    ///
    /// In XDG mode each category uses, in order: `<APP>_<KIND>_DIR`, the
    /// corresponding `XDG_*_HOME` plus the app id, the conventional path below
    /// `HOME`, then a category-specific temporary-directory fallback. Only the
    /// first app-specific tier is recorded in diagnostics. Empty strings are
    /// absent. Resolution performs no filesystem writes.
    ///
    /// # Errors
    ///
    /// Returns [`AppStorageError::InvalidAppId`] if the builder's identifier is invalid.
    ///
    /// # Examples
    ///
    /// ```
    /// use ailloli_ui_app_storage::AppStorage;
    /// let storage = AppStorage::for_app("my-app").resolve_with_env(|key| match key {
    ///     "MY_APP_STATE_DIR" => Some("/override/state".into()),
    ///     "HOME" => Some("/var/empty/example-user".into()),
    ///     _ => None,
    /// })?;
    /// assert_eq!(storage.state_dir().to_string_lossy(), "/override/state");
    /// assert_eq!(storage.diagnostics().env_overrides.len(), 1);
    /// # Ok::<(), ailloli_ui_app_storage::AppStorageError>(())
    /// ```
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

/// Filters environment values and records app-specific paths that are consumed.
struct EnvResolver<F> {
    /// Environment lookup supplied by production code or a deterministic test.
    get: F,
    /// App-specific environment overrides consumed during resolution.
    overrides: Vec<EnvOverride>,
}

impl<F> EnvResolver<F>
where
    F: FnMut(&str) -> Option<String>,
{
    /// Wraps a lookup closure with an empty override log.
    fn new(get: F) -> Self {
        Self {
            get,
            overrides: Vec::new(),
        }
    }

    /// Returns one non-empty environment value.
    fn get(&mut self, key: &str) -> Option<String> {
        (self.get)(key).filter(|value| !value.is_empty())
    }

    /// Converts one non-empty environment value into a path verbatim.
    fn get_path(&mut self, key: &str) -> Option<PathBuf> {
        self.get(key).map(PathBuf::from)
    }

    /// Gets and records one app-specific path override.
    fn get_override_path(&mut self, key: &str) -> Option<PathBuf> {
        let path = self.get_path(key)?;
        self.overrides.push(EnvOverride {
            key: key.to_string(),
            path: path.clone(),
        });
        Some(path)
    }
}

/// Resolves all XDG category directories using the documented priority chain.
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

/// Expands only exact `~` and leading `~/` forms when a home path is available.
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

/// Resolved application storage paths and optional compatibility-view settings.
///
/// Constructing this value does not create directories. Write methods and
/// [`Self::ensure_home_symlink_view`] perform the filesystem mutations.
///
/// # Examples
///
/// ```
/// use ailloli_ui_app_storage::{AppStorage, AppStorageMode};
/// let storage = AppStorage::single_dir("example", "/tmp/store").resolve_with_env(|_| None)?;
/// assert!(matches!(storage.mode(), AppStorageMode::SingleDir { .. }));
/// # Ok::<(), ailloli_ui_app_storage::AppStorageError>(())
/// ```
#[derive(Debug, Clone)]
pub struct AppStorage {
    /// Validated filename-safe application identifier.
    app_id: AppId,
    /// Resolution policy that produced [`Self::dirs`].
    mode: AppStorageMode,
    /// Fully resolved configuration, state, data, and cache directories.
    dirs: AppStorageDirs,
    /// Optional home-view configuration; no symlink is created by resolution.
    home_symlink_view: HomeSymlinkView,
    /// App-specific environment values consumed while resolving paths.
    env_overrides: Vec<EnvOverride>,
}

impl AppStorage {
    /// Starts an XDG-mode builder with the symlink view disabled.
    ///
    /// The identifier is validated only by [`AppStorageBuilder::resolve`] or
    /// [`AppStorageBuilder::resolve_with_env`].
    ///
    /// # Examples
    ///
    /// ```
    /// use ailloli_ui_app_storage::{AppStorage, AppStorageMode};
    /// let storage = AppStorage::for_app("example").resolve_with_env(|_| None)?;
    /// assert_eq!(storage.mode(), &AppStorageMode::Xdg);
    /// # Ok::<(), ailloli_ui_app_storage::AppStorageError>(())
    /// ```
    pub fn for_app(app_id: impl Into<String>) -> AppStorageBuilder {
        AppStorageBuilder {
            app_id: app_id.into(),
            mode: AppStorageMode::Xdg,
            home_symlink_view: false,
            home_symlink_root: None,
        }
    }

    /// Starts a builder that places all category directories below `root`.
    ///
    /// This is shorthand for [`Self::for_app`] followed by
    /// [`AppStorageBuilder::single_dir`].
    ///
    /// # Examples
    ///
    /// ```
    /// use ailloli_ui_app_storage::AppStorage;
    /// let storage = AppStorage::single_dir("example", "/store").resolve_with_env(|_| None)?;
    /// assert_eq!(storage.data_dir().to_string_lossy(), "/store/data");
    /// # Ok::<(), ailloli_ui_app_storage::AppStorageError>(())
    /// ```
    pub fn single_dir(app_id: impl Into<String>, root: impl Into<PathBuf>) -> AppStorageBuilder {
        Self::for_app(app_id).single_dir(root)
    }

    /// Borrows the validated application identifier.
    ///
    /// # Examples
    ///
    /// ```
    /// use ailloli_ui_app_storage::AppStorage;
    /// let storage = AppStorage::single_dir("example", "/store").resolve_with_env(|_| None)?;
    /// assert_eq!(storage.app_id().as_str(), "example");
    /// # Ok::<(), ailloli_ui_app_storage::AppStorageError>(())
    /// ```
    pub fn app_id(&self) -> &AppId {
        &self.app_id
    }

    /// Borrows the selected resolution strategy and its root, if any.
    ///
    /// # Examples
    ///
    /// ```
    /// use ailloli_ui_app_storage::{AppStorage, AppStorageMode};
    /// let storage = AppStorage::single_dir("example", "/store").resolve_with_env(|_| None)?;
    /// assert!(matches!(storage.mode(), AppStorageMode::SingleDir { .. }));
    /// # Ok::<(), ailloli_ui_app_storage::AppStorageError>(())
    /// ```
    pub fn mode(&self) -> &AppStorageMode {
        &self.mode
    }

    /// Borrows all four resolved canonical directories.
    ///
    /// # Examples
    ///
    /// ```
    /// use ailloli_ui_app_storage::AppStorage;
    /// let storage = AppStorage::single_dir("example", "/store").resolve_with_env(|_| None)?;
    /// assert_eq!(storage.dirs().config_dir.to_string_lossy(), "/store/config");
    /// # Ok::<(), ailloli_ui_app_storage::AppStorageError>(())
    /// ```
    pub fn dirs(&self) -> &AppStorageDirs {
        &self.dirs
    }

    /// Returns the canonical configuration directory.
    ///
    /// # Examples
    ///
    /// ```
    /// use ailloli_ui_app_storage::AppStorage;
    /// let storage = AppStorage::single_dir("example", "/store").resolve_with_env(|_| None)?;
    /// assert_eq!(storage.config_dir().to_string_lossy(), "/store/config");
    /// # Ok::<(), ailloli_ui_app_storage::AppStorageError>(())
    /// ```
    pub fn config_dir(&self) -> &Path {
        &self.dirs.config_dir
    }

    /// Returns the canonical restorable-state directory.
    ///
    /// # Examples
    ///
    /// ```
    /// use ailloli_ui_app_storage::AppStorage;
    /// let storage = AppStorage::single_dir("example", "/store").resolve_with_env(|_| None)?;
    /// assert_eq!(storage.state_dir().to_string_lossy(), "/store/state");
    /// # Ok::<(), ailloli_ui_app_storage::AppStorageError>(())
    /// ```
    pub fn state_dir(&self) -> &Path {
        &self.dirs.state_dir
    }

    /// Returns the canonical durable-data directory.
    ///
    /// # Examples
    ///
    /// ```
    /// use ailloli_ui_app_storage::AppStorage;
    /// let storage = AppStorage::single_dir("example", "/store").resolve_with_env(|_| None)?;
    /// assert_eq!(storage.data_dir().to_string_lossy(), "/store/data");
    /// # Ok::<(), ailloli_ui_app_storage::AppStorageError>(())
    /// ```
    pub fn data_dir(&self) -> &Path {
        &self.dirs.data_dir
    }

    /// Returns the canonical disposable-cache directory.
    ///
    /// # Examples
    ///
    /// ```
    /// use ailloli_ui_app_storage::AppStorage;
    /// let storage = AppStorage::single_dir("example", "/store").resolve_with_env(|_| None)?;
    /// assert_eq!(storage.cache_dir().to_string_lossy(), "/store/cache");
    /// # Ok::<(), ailloli_ui_app_storage::AppStorageError>(())
    /// ```
    pub fn cache_dir(&self) -> &Path {
        &self.dirs.cache_dir
    }

    /// Returns `<config_dir>/preferences.json` without creating it.
    ///
    /// # Examples
    ///
    /// ```
    /// use ailloli_ui_app_storage::AppStorage;
    /// let storage = AppStorage::single_dir("example", "/store").resolve_with_env(|_| None)?;
    /// assert_eq!(storage.preferences_file().to_string_lossy(), "/store/config/preferences.json");
    /// # Ok::<(), ailloli_ui_app_storage::AppStorageError>(())
    /// ```
    pub fn preferences_file(&self) -> PathBuf {
        self.config_dir().join("preferences.json")
    }

    /// Returns `<state_dir>/windows.json` without creating it.
    ///
    /// # Examples
    ///
    /// ```
    /// use ailloli_ui_app_storage::AppStorage;
    /// let storage = AppStorage::single_dir("example", "/store").resolve_with_env(|_| None)?;
    /// assert_eq!(storage.window_state_file().to_string_lossy(), "/store/state/windows.json");
    /// # Ok::<(), ailloli_ui_app_storage::AppStorageError>(())
    /// ```
    pub fn window_state_file(&self) -> PathBuf {
        self.state_dir().join("windows.json")
    }

    /// Clones resolved metadata and inspects the current compatibility-view paths.
    ///
    /// This is read-only. The override list contains only app-specific
    /// `<APP>_<KIND>_DIR` values that actually won resolution.
    ///
    /// # Examples
    ///
    /// ```
    /// use ailloli_ui_app_storage::AppStorage;
    /// let storage = AppStorage::single_dir("example", "/store").resolve_with_env(|_| None)?;
    /// assert!(storage.diagnostics().env_overrides.is_empty());
    /// # Ok::<(), ailloli_ui_app_storage::AppStorageError>(())
    /// ```
    pub fn diagnostics(&self) -> AppStorageDiagnostics {
        AppStorageDiagnostics {
            app_id: self.app_id.clone(),
            dirs: self.dirs.clone(),
            env_overrides: self.env_overrides.clone(),
            home_symlink_view: self.home_symlink_view_status(),
        }
    }

    /// Inspects the four expected compatibility links without modifying them.
    ///
    /// Symlink targets are compared as stored paths; equivalent paths with
    /// different textual forms are reported as collisions. Inspection errors
    /// are represented as entry collisions rather than returned as `Err`.
    ///
    /// # Examples
    ///
    /// ```
    /// use ailloli_ui_app_storage::{AppStorage, HomeSymlinkViewState};
    /// let storage = AppStorage::single_dir("example", "/store").resolve_with_env(|_| None)?;
    /// assert_eq!(storage.home_symlink_view_status().state(), HomeSymlinkViewState::Disabled);
    /// # Ok::<(), ailloli_ui_app_storage::AppStorageError>(())
    /// ```
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

    /// Creates missing canonical directories and non-destructive compatibility links.
    ///
    /// Disabled views return their read-only status without writing. Enabled
    /// creation is sequential and not transactional: an error can leave earlier
    /// target directories or links in place. Existing correct links are reused;
    /// no existing non-link or differently targeted link is replaced.
    ///
    /// # Errors
    ///
    /// Returns [`AppStorageError::SymlinkCollision`] for an incompatible root or
    /// entry, [`AppStorageError::SymlinkUnsupported`] where directory links are
    /// unavailable, or [`AppStorageError::Io`] for directory/link operations.
    ///
    /// # Platform behavior
    ///
    /// Uses Unix symlinks on Unix and directory symlinks on Windows. Windows may
    /// require host policy or privileges for link creation. Other targets return
    /// [`AppStorageError::SymlinkUnsupported`].
    ///
    /// # Examples
    ///
    /// ```
    /// use ailloli_ui_app_storage::{AppStorage, HomeSymlinkViewState};
    /// let storage = AppStorage::single_dir("example", "/store").resolve_with_env(|_| None)?;
    /// assert_eq!(storage.ensure_home_symlink_view()?.state(), HomeSymlinkViewState::Disabled);
    /// # Ok::<(), ailloli_ui_app_storage::AppStorageError>(())
    /// ```
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

    /// Reads and validates the application preferences document if present.
    ///
    /// A missing file returns `Ok(None)`. An existing document must carry
    /// [`APP_PREFERENCES_VERSION`]; arbitrary preference keys and JSON values
    /// are preserved.
    ///
    /// # Errors
    ///
    /// Returns [`AppStorageError::Io`] for read failures,
    /// [`AppStorageError::Json`] for invalid JSON, or
    /// [`AppStorageError::InvalidDocument`] for an unsupported version.
    ///
    /// # Examples
    ///
    /// ```
    /// use ailloli_ui_app_storage::AppStorage;
    /// let root = std::env::temp_dir().join(format!("ailloli-doc-missing-prefs-{}", std::process::id()));
    /// let storage = AppStorage::single_dir("example", root).resolve_with_env(|_| None)?;
    /// assert_eq!(storage.read_preferences()?, None);
    /// # Ok::<(), ailloli_ui_app_storage::AppStorageError>(())
    /// ```
    pub fn read_preferences(&self) -> Result<Option<AppPreferencesDocument>> {
        read_optional_json(self.preferences_file()).and_then(|doc| match doc {
            Some(doc) => validate_preferences(doc).map(Some),
            None => Ok(None),
        })
    }

    /// Pretty-prints preferences through a same-directory temp file and rename.
    ///
    /// This method does not validate [`AppPreferencesDocument::version`]; a
    /// mismatched version can be written but a later [`Self::read_preferences`]
    /// will reject it.
    ///
    /// # Errors
    ///
    /// Returns [`AppStorageError::Json`] for serialization failure,
    /// [`AppStorageError::InvalidDocument`] for an unusable destination name,
    /// or [`AppStorageError::Io`] for filesystem failures.
    ///
    /// # Examples
    ///
    /// ```
    /// use ailloli_ui_app_storage::{AppPreferencesDocument, AppStorage};
    /// let root = std::env::temp_dir().join(format!("ailloli-doc-write-prefs-{}", std::process::id()));
    /// let storage = AppStorage::single_dir("example", &root).resolve_with_env(|_| None)?;
    /// storage.write_preferences(&AppPreferencesDocument::new())?;
    /// assert!(storage.preferences_file().is_file());
    /// std::fs::remove_dir_all(root)?;
    /// # Ok::<(), Box<dyn std::error::Error>>(())
    /// ```
    pub fn write_preferences(&self, preferences: &AppPreferencesDocument) -> Result<()> {
        write_json_atomic(self.preferences_file(), preferences)
    }

    /// Reads and validates the window-state document if present.
    ///
    /// A missing file returns `Ok(None)`. Validation currently checks only that
    /// [`WindowStateDocument::version`] equals [`WINDOW_STATE_VERSION`]; snapshot
    /// fields deserialized from JSON are otherwise preserved verbatim.
    ///
    /// # Errors
    ///
    /// Returns [`AppStorageError::Io`] for read failures,
    /// [`AppStorageError::Json`] for invalid JSON, or
    /// [`AppStorageError::InvalidDocument`] for an unsupported version.
    ///
    /// # Examples
    ///
    /// ```
    /// use ailloli_ui_app_storage::AppStorage;
    /// let root = std::env::temp_dir().join(format!("ailloli-doc-missing-windows-{}", std::process::id()));
    /// let storage = AppStorage::single_dir("example", root).resolve_with_env(|_| None)?;
    /// assert_eq!(storage.read_window_state()?, None);
    /// # Ok::<(), ailloli_ui_app_storage::AppStorageError>(())
    /// ```
    pub fn read_window_state(&self) -> Result<Option<WindowStateDocument>> {
        read_optional_json(self.window_state_file()).and_then(|doc| match doc {
            Some(doc) => validate_window_state(doc).map(Some),
            None => Ok(None),
        })
    }

    /// Pretty-prints window state through a same-directory temp file and rename.
    ///
    /// This method does not validate the document version or snapshot contents;
    /// version validation occurs when reading through [`Self::read_window_state`].
    ///
    /// # Errors
    ///
    /// Returns [`AppStorageError::Json`] for serialization failure,
    /// [`AppStorageError::InvalidDocument`] for an unusable destination name,
    /// or [`AppStorageError::Io`] for filesystem failures.
    ///
    /// # Examples
    ///
    /// ```
    /// use ailloli_ui_app_storage::{AppStorage, WindowStateDocument};
    /// let root = std::env::temp_dir().join(format!("ailloli-doc-write-windows-{}", std::process::id()));
    /// let storage = AppStorage::single_dir("example", &root).resolve_with_env(|_| None)?;
    /// storage.write_window_state(&WindowStateDocument::empty())?;
    /// assert!(storage.window_state_file().is_file());
    /// std::fs::remove_dir_all(root)?;
    /// # Ok::<(), Box<dyn std::error::Error>>(())
    /// ```
    pub fn write_window_state(&self, document: &WindowStateDocument) -> Result<()> {
        write_json_atomic(self.window_state_file(), document)
    }

    /// Builds the stable category/link/target triples in diagnostic order.
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

/// Inspects one path without following it and compares a symlink target verbatim.
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
/// Creates one Unix directory-target symlink without replacing an existing path.
///
/// # Errors
///
/// Returns [`AppStorageError::Io`] with `link` as path context when the native
/// symlink operation fails, including when the destination already exists.
fn create_dir_symlink(target: &Path, link: &Path) -> Result<()> {
    std::os::unix::fs::symlink(target, link)
        .map_err(|err| AppStorageError::io(Some(link.to_path_buf()), err))
}

#[cfg(windows)]
/// Creates one Windows directory symlink without replacing an existing path.
///
/// # Errors
///
/// Returns [`AppStorageError::Io`] with `link` as path context when the native
/// directory-symlink operation fails, including permission and collision errors.
fn create_dir_symlink(target: &Path, link: &Path) -> Result<()> {
    std::os::windows::fs::symlink_dir(target, link)
        .map_err(|err| AppStorageError::io(Some(link.to_path_buf()), err))
}

#[cfg(not(any(unix, windows)))]
/// Reports the lack of directory-symlink support on other targets.
///
/// # Errors
///
/// Always returns [`AppStorageError::SymlinkUnsupported`] because this target
/// has no implementation for directory symlinks.
fn create_dir_symlink(_target: &Path, _link: &Path) -> Result<()> {
    Err(AppStorageError::SymlinkUnsupported(
        "directory symlinks are not available on this platform".into(),
    ))
}

/// Versioned, extensible application-preferences payload.
///
/// Values are ordered by key for deterministic in-memory traversal and JSON
/// serialization. Empty values are valid. Deserialization preserves any
/// version; [`AppStorage::read_preferences`] performs compatibility validation.
///
/// # Examples
///
/// ```
/// use ailloli_ui_app_storage::AppPreferencesDocument;
/// use serde_json::json;
/// let mut preferences = AppPreferencesDocument::new();
/// preferences.values.insert("theme".into(), json!("dark"));
/// assert_eq!(preferences.values["theme"], "dark");
/// ```
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct AppPreferencesDocument {
    /// Persisted schema version; the current supported value is [`APP_PREFERENCES_VERSION`].
    pub version: u32,
    /// Application-defined JSON preferences; an empty map means no saved preferences.
    pub values: BTreeMap<String, Value>,
}

impl AppPreferencesDocument {
    /// Creates a current-version document with no preference values.
    ///
    /// # Examples
    ///
    /// ```
    /// use ailloli_ui_app_storage::{AppPreferencesDocument, APP_PREFERENCES_VERSION};
    /// let document = AppPreferencesDocument::new();
    /// assert_eq!(document.version, APP_PREFERENCES_VERSION);
    /// assert!(document.values.is_empty());
    /// ```
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

/// Versioned set of persisted top-level window snapshots.
///
/// An empty vector is valid. Duplicate or empty window identifiers are stored;
/// [`Self::snapshot_for`] returns the first exact match. Deserialization
/// preserves any version until [`AppStorage::read_window_state`] validates it.
///
/// # Examples
///
/// ```
/// use ailloli_ui_app_storage::{WindowSnapshot, WindowStateDocument};
/// let document = WindowStateDocument::new(vec![WindowSnapshot::new("main")]);
/// assert!(document.snapshot_for("main").is_some());
/// ```
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct WindowStateDocument {
    /// Persisted schema version; the current supported value is [`WINDOW_STATE_VERSION`].
    pub version: u32,
    /// Snapshots in caller-provided order; an empty list represents no saved windows.
    pub windows: Vec<WindowSnapshot>,
}

impl WindowStateDocument {
    /// Creates a current-version document from snapshots stored verbatim.
    ///
    /// # Examples
    ///
    /// ```
    /// use ailloli_ui_app_storage::{WindowSnapshot, WindowStateDocument};
    /// let document = WindowStateDocument::new(vec![WindowSnapshot::new("main")]);
    /// assert_eq!(document.windows.len(), 1);
    /// ```
    pub fn new(windows: Vec<WindowSnapshot>) -> Self {
        Self {
            version: WINDOW_STATE_VERSION,
            windows,
        }
    }

    /// Creates a current-version document with no snapshots.
    ///
    /// # Examples
    ///
    /// ```
    /// use ailloli_ui_app_storage::WindowStateDocument;
    /// assert!(WindowStateDocument::empty().windows.is_empty());
    /// ```
    pub fn empty() -> Self {
        Self::new(Vec::new())
    }

    /// Returns the first snapshot whose identifier exactly equals `window_id`.
    ///
    /// Returns `None` for no match, including when the document is empty. The
    /// lookup is linear in the number of windows.
    ///
    /// # Examples
    ///
    /// ```
    /// use ailloli_ui_app_storage::{WindowSnapshot, WindowStateDocument};
    /// let document = WindowStateDocument::new(vec![WindowSnapshot::new("main")]);
    /// assert_eq!(document.snapshot_for("missing"), None);
    /// ```
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

/// Restorable state for one logical top-level window.
///
/// The identifier may be empty and is not normalized. `None` size/position
/// means that geometry was not saved and the platform should choose it.
///
/// # Examples
///
/// ```
/// use ailloli_ui_app_storage::WindowSnapshot;
/// let snapshot = WindowSnapshot::new("main");
/// assert_eq!(snapshot.inner_size, None);
/// assert!(!snapshot.maximized && !snapshot.fullscreen);
/// ```
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct WindowSnapshot {
    /// Consumer-defined logical window identifier, matched exactly.
    pub window_id: String,
    /// Saved inner content size in logical pixels, or `None` to use platform defaults.
    pub inner_size: Option<LogicalWindowSize>,
    /// Whether the window should be restored maximized.
    pub maximized: bool,
    /// Whether the window should be restored fullscreen.
    pub fullscreen: bool,
    /// Saved logical desktop position, or `None` to let the platform place it.
    pub position: Option<LogicalWindowPosition>,
}

impl WindowSnapshot {
    /// Creates a normally positioned snapshot with no saved geometry.
    ///
    /// `window_id` is stored verbatim, including an empty string.
    ///
    /// # Examples
    ///
    /// ```
    /// use ailloli_ui_app_storage::WindowSnapshot;
    /// let snapshot = WindowSnapshot::new("main");
    /// assert_eq!(snapshot.window_id, "main");
    /// assert_eq!(snapshot.position, None);
    /// ```
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

/// Window inner size in logical pixels.
///
/// [`Self::new`] floors ordinary finite components, NaN, and negative infinity
/// at `1.0`; positive infinity remains infinite. Public fields and derived
/// deserialization can represent values that bypass that normalization.
///
/// # Examples
///
/// ```
/// use ailloli_ui_app_storage::LogicalWindowSize;
/// assert_eq!(LogicalWindowSize::new(0.0, 480.0), LogicalWindowSize { width: 1.0, height: 480.0 });
/// ```
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct LogicalWindowSize {
    /// Inner width in logical pixels.
    pub width: f64,
    /// Inner height in logical pixels.
    pub height: f64,
}

impl LogicalWindowSize {
    /// Creates a size with each component floored at one logical pixel.
    ///
    /// Floating-point `max` maps NaN and negative infinity to `1.0`, while
    /// positive infinity remains infinite.
    ///
    /// # Examples
    ///
    /// ```
    /// use ailloli_ui_app_storage::LogicalWindowSize;
    /// assert_eq!(LogicalWindowSize::new(-10.0, f64::NAN), LogicalWindowSize::new(1.0, 1.0));
    /// ```
    pub fn new(width: f64, height: f64) -> Self {
        Self {
            width: width.max(1.0),
            height: height.max(1.0),
        }
    }
}

/// Window position in logical desktop coordinates.
///
/// Negative, fractional, and non-finite coordinates are stored verbatim.
///
/// # Examples
///
/// ```
/// use ailloli_ui_app_storage::LogicalWindowPosition;
/// assert_eq!(LogicalWindowPosition::new(-20.5, 10.0).x, -20.5);
/// ```
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct LogicalWindowPosition {
    /// Horizontal logical desktop coordinate.
    pub x: f64,
    /// Vertical logical desktop coordinate.
    pub y: f64,
}

impl LogicalWindowPosition {
    /// Stores a logical position without validation or normalization.
    ///
    /// # Examples
    ///
    /// ```
    /// use ailloli_ui_app_storage::LogicalWindowPosition;
    /// assert_eq!(LogicalWindowPosition::new(10.0, 20.0), LogicalWindowPosition { x: 10.0, y: 20.0 });
    /// ```
    pub fn new(x: f64, y: f64) -> Self {
        Self { x, y }
    }
}

/// Accepts only the current preferences schema version, preserving all values.
///
/// # Errors
///
/// Returns [`AppStorageError::InvalidDocument`] when `document.version` differs
/// from [`APP_PREFERENCES_VERSION`].
fn validate_preferences(document: AppPreferencesDocument) -> Result<AppPreferencesDocument> {
    if document.version != APP_PREFERENCES_VERSION {
        return Err(AppStorageError::InvalidDocument(format!(
            "unsupported preferences version {}",
            document.version
        )));
    }
    Ok(document)
}

/// Accepts only the current window-state version without validating snapshots.
///
/// # Errors
///
/// Returns [`AppStorageError::InvalidDocument`] when `document.version` differs
/// from [`WINDOW_STATE_VERSION`].
fn validate_window_state(document: WindowStateDocument) -> Result<WindowStateDocument> {
    if document.version != WINDOW_STATE_VERSION {
        return Err(AppStorageError::InvalidDocument(format!(
            "unsupported window state version {}",
            document.version
        )));
    }
    Ok(document)
}

/// Deserializes an optional UTF-8 or binary JSON file into `T`.
///
/// A missing path returns `Ok(None)`; an existing JSON `null` is still
/// `Some(T)` when `T` accepts it. The whole file is read into memory.
///
/// # Errors
///
/// Returns [`AppStorageError::Io`] for filesystem failures other than not found,
/// or [`AppStorageError::Json`] when the bytes do not deserialize as `T`.
///
/// # Examples
///
/// ```
/// use ailloli_ui_app_storage::read_optional_json;
/// use serde_json::Value;
/// let path = std::env::temp_dir().join(format!("ailloli-doc-missing-json-{}", std::process::id()));
/// let value: Option<Value> = read_optional_json(path)?;
/// assert_eq!(value, None);
/// # Ok::<(), ailloli_ui_app_storage::AppStorageError>(())
/// ```
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

/// Pretty-serializes `value` and writes it through a same-directory temp file.
///
/// The complete JSON representation is buffered in memory before writing. See
/// [`atomic_write_bytes`] for rename, concurrency, and durability semantics.
///
/// # Errors
///
/// Returns [`AppStorageError::Json`] when serialization fails, or propagates
/// [`atomic_write_bytes`] destination and filesystem errors.
///
/// # Examples
///
/// ```
/// use ailloli_ui_app_storage::write_json_atomic;
/// use serde_json::json;
/// let path = std::env::temp_dir().join(format!("ailloli-doc-json-{}.json", std::process::id()));
/// write_json_atomic(&path, &json!({"ready": true}))?;
/// assert!(std::fs::read_to_string(&path)?.contains("ready"));
/// std::fs::remove_file(path)?;
/// # Ok::<(), Box<dyn std::error::Error>>(())
/// ```
pub fn write_json_atomic<T>(path: impl AsRef<Path>, value: &T) -> Result<()>
where
    T: Serialize,
{
    let bytes = serde_json::to_vec_pretty(value)?;
    atomic_write_bytes(path.as_ref(), &bytes)
}

/// Writes bytes to `<file-name>.tmp`, syncs them, then renames over `path`.
///
/// Missing parent directories are created. The temp name is deterministic, so
/// concurrent writers to the same destination race with one another. A failed
/// operation can leave the temp file behind. File contents are synced before
/// rename; opening and syncing the parent directory is best effort and its
/// errors are ignored. Destination replacement and rename atomicity follow the
/// host filesystem and platform; permissions of an older destination are not
/// preserved on the newly created file.
///
/// # Errors
///
/// Returns [`AppStorageError::InvalidDocument`] if the destination has no
/// UTF-8 file name, or [`AppStorageError::Io`] if parent creation, temp-file
/// creation/write/sync, or rename fails.
///
/// # Examples
///
/// ```
/// use ailloli_ui_app_storage::atomic_write_bytes;
/// let path = std::env::temp_dir().join(format!("ailloli-doc-bytes-{}.bin", std::process::id()));
/// atomic_write_bytes(&path, b"saved")?;
/// assert_eq!(std::fs::read(&path)?, b"saved");
/// std::fs::remove_file(path)?;
/// # Ok::<(), Box<dyn std::error::Error>>(())
/// ```
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

/// Derives the deterministic sibling temp path and requires a UTF-8 file name.
///
/// # Errors
///
/// Returns [`AppStorageError::InvalidDocument`] when `path` has no final file
/// name or that name is not valid UTF-8.
fn temp_path_for(path: &Path) -> Result<PathBuf> {
    let file_name = path
        .file_name()
        .and_then(|value| value.to_str())
        .ok_or_else(|| AppStorageError::InvalidDocument(path.display().to_string()))?;
    Ok(path.with_file_name(format!("{file_name}.tmp")))
}

#[cfg(test)]
mod tests {
    //! Covers identifier validation, path precedence, non-destructive symlinks,
    //! collision reporting, and persisted-document round trips.

    use super::*;
    use std::time::{SystemTime, UNIX_EPOCH};

    /// Creates a process/time-namespaced temporary directory for one test.
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
                "HOME" => Some("/tmp/ailloli_ui_home".into()),
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
