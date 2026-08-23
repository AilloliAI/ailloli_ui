//! Single background worker for non-blocking local directory listings.

use std::sync::mpsc::{self, Receiver, Sender, TryRecvError};
use std::thread;

use ailloli_ui_fs::{FileEntry, FileError, FileProvider, FileUri};
use ailloli_ui_fs_local::LocalFileProvider;

use super::store::FileTreeNodeId;
use super::tree::{
    should_include_file_entry, sort_file_entries, truncate_entries, FileTreeOptions,
};

/// Command sent to the local filesystem worker.
///
/// Requests are processed serially in send order by one background thread.
///
/// # Examples
///
/// ```
/// use ailloli_ui_widgets::files::FileExplorerIoRequest;
/// assert!(matches!(FileExplorerIoRequest::Shutdown, FileExplorerIoRequest::Shutdown));
/// ```
#[derive(Debug, Clone)]
pub enum FileExplorerIoRequest {
    /// Reads and normalizes one directory.
    LoadDirectory {
        /// Store-local node to correlate with the response.
        node_id: FileTreeNodeId,
        /// Directory URI passed to the local provider.
        uri: FileUri,
        /// Optional selected URI preserved through hidden/exclusion filters.
        selected: Option<FileUri>,
        /// Filtering, sorting, and truncation policy snapshot.
        options: FileTreeOptions,
    },
    /// Stops the worker after all earlier queued requests.
    Shutdown,
}

/// Completed local directory request returned by the worker.
///
/// `truncated` is meaningful only for a successful result; provider errors set
/// it to `false`. Responses retain the request's node and URI for stale-result
/// detection by the owner.
///
/// # Examples
///
/// ```
/// use ailloli_ui_fs::{FileEntry, FileError, FileUri};
/// use ailloli_ui_widgets::files::{FileExplorerIoResponse, FileTreeNodeId};
/// let response = FileExplorerIoResponse {
///     node_id: FileTreeNodeId(3),
///     uri: FileUri::parse("file:///tmp")?,
///     result: Ok::<Vec<FileEntry>, FileError>(Vec::new()),
///     truncated: false,
/// };
/// assert_eq!(response.node_id, FileTreeNodeId(3));
/// # Ok::<(), ailloli_ui_fs::FileError>(())
/// ```
#[derive(Debug, Clone)]
pub struct FileExplorerIoResponse {
    /// Store-local node copied from the request.
    pub node_id: FileTreeNodeId,
    /// Directory URI copied from the request.
    pub uri: FileUri,
    /// Normalized entries or the provider failure.
    pub result: Result<Vec<FileEntry>, FileError>,
    /// Whether the configured per-directory cap removed entries.
    pub truncated: bool,
}

/// Owned request/response channels and worker thread for local listings.
///
/// Construction starts one thread and one [`LocalFileProvider`]. Dropping the
/// handle requests shutdown but does not join the thread; the thread also exits
/// when all request senders disconnect.
///
/// # Examples
///
/// ```
/// use ailloli_ui_widgets::files::LocalFileExplorerIoWorker;
/// let worker = LocalFileExplorerIoWorker::new();
/// assert!(worker.try_recv_all().is_empty());
/// ```
pub struct LocalFileExplorerIoWorker {
    /// Request producer consumed by the dedicated filesystem thread.
    tx: Sender<FileExplorerIoRequest>,
    /// Response consumer drained by the UI-side polling path.
    rx: Receiver<FileExplorerIoResponse>,
}

/// Starts a worker using [`Self::new`].
impl Default for LocalFileExplorerIoWorker {
    fn default() -> Self {
        Self::new()
    }
}

impl LocalFileExplorerIoWorker {
    /// Starts a background local-provider request loop.
    ///
    /// The channels are unbounded. This function returns immediately after
    /// spawning and does not perform filesystem I/O on the caller's thread.
    ///
    /// # Examples
    ///
    /// ```
    /// use ailloli_ui_widgets::files::LocalFileExplorerIoWorker;
    /// let worker = LocalFileExplorerIoWorker::new();
    /// assert!(worker.try_recv_all().is_empty());
    /// ```
    pub fn new() -> Self {
        let (tx, request_rx) = mpsc::channel();
        let (response_tx, rx) = mpsc::channel();
        thread::spawn(move || {
            let provider = LocalFileProvider::new();
            while let Ok(request) = request_rx.recv() {
                match request {
                    FileExplorerIoRequest::LoadDirectory {
                        node_id,
                        uri,
                        selected,
                        options,
                    } => {
                        let result = provider
                            .read_dir(&uri)
                            .map(|entries| normalize_entries(entries, selected.as_ref(), options));
                        let (result, truncated) = match result {
                            Ok((entries, truncated)) => (Ok(entries), truncated),
                            Err(err) => (Err(err), false),
                        };
                        let _ = response_tx.send(FileExplorerIoResponse {
                            node_id,
                            uri,
                            result,
                            truncated,
                        });
                    }
                    FileExplorerIoRequest::Shutdown => break,
                }
            }
        });
        Self { tx, rx }
    }

    /// Queues a request without waiting for filesystem completion.
    ///
    /// # Errors
    ///
    /// Returns the channel error as a string if the worker has disconnected.
    /// A successful send does not imply that a directory read will succeed.
    ///
    /// # Examples
    ///
    /// ```
    /// use ailloli_ui_widgets::files::{FileExplorerIoRequest, LocalFileExplorerIoWorker};
    /// let worker = LocalFileExplorerIoWorker::new();
    /// worker.request(FileExplorerIoRequest::Shutdown)?;
    /// # Ok::<(), String>(())
    /// ```
    pub fn request(&self, request: FileExplorerIoRequest) -> Result<(), String> {
        self.tx.send(request).map_err(|err| err.to_string())
    }

    /// Drains every response currently available without blocking.
    ///
    /// Responses preserve completion order. An empty vector means no result is
    /// ready or the response channel disconnected; it is not an error signal.
    ///
    /// # Examples
    ///
    /// ```
    /// use ailloli_ui_widgets::files::LocalFileExplorerIoWorker;
    /// let worker = LocalFileExplorerIoWorker::default();
    /// let ready = worker.try_recv_all();
    /// assert!(ready.is_empty());
    /// ```
    pub fn try_recv_all(&self) -> Vec<FileExplorerIoResponse> {
        let mut out = Vec::new();
        loop {
            match self.rx.try_recv() {
                Ok(response) => out.push(response),
                Err(TryRecvError::Empty) => break,
                Err(TryRecvError::Disconnected) => break,
            }
        }
        out
    }
}

impl Drop for LocalFileExplorerIoWorker {
    fn drop(&mut self) {
        let _ = self.tx.send(FileExplorerIoRequest::Shutdown);
    }
}

/// Applies inclusion, deterministic directory-first sorting, and size capping.
fn normalize_entries(
    entries: Vec<FileEntry>,
    selected: Option<&FileUri>,
    options: FileTreeOptions,
) -> (Vec<FileEntry>, bool) {
    let mut entries = entries
        .into_iter()
        .filter(|entry| should_include_file_entry(entry, selected, options))
        .collect::<Vec<_>>();
    sort_file_entries(&mut entries);
    let truncated = truncate_entries(&mut entries, options);
    (entries, truncated)
}

#[cfg(test)]
/// Exercises the real background thread against a bounded temporary directory.
mod tests {
    use std::fs;
    use std::path::PathBuf;
    use std::time::{Duration, SystemTime, UNIX_EPOCH};

    use ailloli_ui_fs::FileKind;

    use super::*;

    /// Per-test directory removed on drop.
    struct TempDir {
        path: PathBuf,
    }

    impl TempDir {
        /// Creates a process/time-namespaced directory under the OS temp root.
        fn new(name: &str) -> Self {
            let nanos = SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .expect("time")
                .as_nanos();
            let path = std::env::temp_dir().join(format!(
                "ailloli_ui_widgets_local_file_worker_{name}_{}_{}",
                std::process::id(),
                nanos
            ));
            fs::create_dir_all(&path).expect("temp dir");
            Self { path }
        }
    }

    impl Drop for TempDir {
        fn drop(&mut self) {
            let _ = fs::remove_dir_all(&self.path);
        }
    }

    #[test]
    fn worker_loads_directory_async() {
        let temp = TempDir::new("load");
        fs::create_dir_all(temp.path.join("src")).expect("src");
        fs::write(temp.path.join("main.rs"), b"main").expect("main");
        let uri = FileUri::local(&temp.path).expect("uri");
        let worker = LocalFileExplorerIoWorker::new();

        worker
            .request(FileExplorerIoRequest::LoadDirectory {
                node_id: FileTreeNodeId(7),
                uri: uri.clone(),
                selected: None,
                options: FileTreeOptions::default(),
            })
            .expect("request");

        let deadline = std::time::Instant::now() + Duration::from_secs(2);
        let response = loop {
            if let Some(response) = worker.try_recv_all().into_iter().next() {
                break response;
            }
            assert!(std::time::Instant::now() < deadline, "worker timed out");
            std::thread::sleep(Duration::from_millis(10));
        };

        assert_eq!(response.node_id, FileTreeNodeId(7));
        let entries = response.result.expect("entries");
        assert!(entries
            .iter()
            .any(|entry| entry.name == "src" && entry.metadata.kind == FileKind::Directory));
        assert!(entries.iter().any(|entry| entry.name == "main.rs"));
    }
}
