use std::sync::mpsc::{self, Receiver, Sender, TryRecvError};
use std::thread;

use ailloli_ui_fs::{FileEntry, FileError, FileProvider, FileUri};
use ailloli_ui_fs_local::LocalFileProvider;

use super::store::FileTreeNodeId;
use super::tree::{
    should_include_file_entry, sort_file_entries, truncate_entries, FileTreeOptions,
};

#[derive(Debug, Clone)]
pub enum FileExplorerIoRequest {
    LoadDirectory {
        node_id: FileTreeNodeId,
        uri: FileUri,
        selected: Option<FileUri>,
        options: FileTreeOptions,
    },
    Shutdown,
}

#[derive(Debug, Clone)]
pub struct FileExplorerIoResponse {
    pub node_id: FileTreeNodeId,
    pub uri: FileUri,
    pub result: Result<Vec<FileEntry>, FileError>,
    pub truncated: bool,
}

pub struct LocalFileExplorerIoWorker {
    tx: Sender<FileExplorerIoRequest>,
    rx: Receiver<FileExplorerIoResponse>,
}

impl Default for LocalFileExplorerIoWorker {
    fn default() -> Self {
        Self::new()
    }
}

impl LocalFileExplorerIoWorker {
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

    pub fn request(&self, request: FileExplorerIoRequest) -> Result<(), String> {
        self.tx.send(request).map_err(|err| err.to_string())
    }

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
mod tests {
    use std::fs;
    use std::path::PathBuf;
    use std::time::{Duration, SystemTime, UNIX_EPOCH};

    use ailloli_ui_fs::FileKind;

    use super::*;

    struct TempDir {
        path: PathBuf,
    }

    impl TempDir {
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
