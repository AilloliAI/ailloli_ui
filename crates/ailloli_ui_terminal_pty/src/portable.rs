use std::collections::VecDeque;
use std::io::{Read, Write};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc;
use std::sync::{Arc, Mutex};
use std::thread;

use portable_pty::{ChildKiller, CommandBuilder, MasterPty};

use crate::batch::{PtyBatchConfig, PtyOutputBatcher};
use crate::handle::{PtyBackend, PtySession};
use crate::{PtyError, PtyEvent, PtyExitStatus, PtyHandle, PtySize, PtySpawnConfig};

pub struct PortablePtyBackend {
    batch_config: PtyBatchConfig,
}

impl PortablePtyBackend {
    pub fn new() -> Self {
        Self {
            batch_config: PtyBatchConfig::default(),
        }
    }

    pub fn with_batch_config(batch_config: PtyBatchConfig) -> Self {
        Self { batch_config }
    }
}

impl Default for PortablePtyBackend {
    fn default() -> Self {
        Self::new()
    }
}

impl PtyBackend for PortablePtyBackend {
    fn spawn(&self, config: PtySpawnConfig) -> Result<PtyHandle, PtyError> {
        let pty_system = portable_pty::native_pty_system();
        let pair = pty_system
            .openpty(to_portable_size(config.size))
            .map_err(|err| PtyError::Spawn(err.to_string()))?;
        let mut command = command_builder(&config);
        command.env("TERM", &config.term);
        for (key, value) in &config.env {
            command.env(key, value);
        }
        if let Some(cwd) = &config.cwd {
            command.cwd(cwd.as_os_str());
        }

        let child = pair
            .slave
            .spawn_command(command)
            .map_err(|err| PtyError::Spawn(err.to_string()))?;
        let killer = child.clone_killer();
        let reader = pair
            .master
            .try_clone_reader()
            .map_err(|err| PtyError::Io(err.to_string()))?;
        let writer = pair
            .master
            .take_writer()
            .map_err(|err| PtyError::Io(err.to_string()))?;
        let master = Arc::new(Mutex::new(pair.master));
        let events = Arc::new(Mutex::new(VecDeque::new()));
        let shutdown = Arc::new(AtomicBool::new(false));
        let (raw_tx, raw_rx) = mpsc::channel::<Vec<u8>>();

        spawn_reader_thread(reader, raw_tx, events.clone(), shutdown.clone());
        spawn_batcher_thread(raw_rx, events.clone(), self.batch_config);
        spawn_wait_thread(child, events.clone(), shutdown.clone());

        Ok(PtyHandle::new(Arc::new(PortablePtySession {
            master,
            writer: Mutex::new(writer),
            killer: Mutex::new(killer),
            events,
            shutdown,
        })))
    }
}

struct PortablePtySession {
    master: Arc<Mutex<Box<dyn MasterPty + Send>>>,
    writer: Mutex<Box<dyn Write + Send>>,
    killer: Mutex<Box<dyn ChildKiller + Send + Sync>>,
    events: Arc<Mutex<VecDeque<PtyEvent>>>,
    shutdown: Arc<AtomicBool>,
}

impl PtySession for PortablePtySession {
    fn write(&self, bytes: &[u8]) -> Result<(), PtyError> {
        if self.shutdown.load(Ordering::SeqCst) {
            return Err(PtyError::Closed);
        }
        let mut writer = self
            .writer
            .lock()
            .map_err(|_| PtyError::Write("writer lock poisoned".into()))?;
        writer
            .write_all(bytes)
            .map_err(|err| PtyError::Write(err.to_string()))?;
        writer
            .flush()
            .map_err(|err| PtyError::Write(err.to_string()))
    }

    fn resize(&self, size: PtySize) -> Result<(), PtyError> {
        if self.shutdown.load(Ordering::SeqCst) {
            return Err(PtyError::Closed);
        }
        self.master
            .lock()
            .map_err(|_| PtyError::Resize("master lock poisoned".into()))?
            .resize(to_portable_size(size))
            .map_err(|err| PtyError::Resize(err.to_string()))
    }

    fn drain_events(&self) -> Vec<PtyEvent> {
        self.events.lock().expect("pty events").drain(..).collect()
    }

    fn shutdown(&self) -> Result<(), PtyError> {
        if self.shutdown.swap(true, Ordering::SeqCst) {
            return Ok(());
        }
        self.killer
            .lock()
            .map_err(|_| PtyError::Shutdown("killer lock poisoned".into()))?
            .kill()
            .map_err(|err| PtyError::Shutdown(err.to_string()))
    }

    fn is_shutdown(&self) -> bool {
        self.shutdown.load(Ordering::SeqCst)
    }
}

fn command_builder(config: &PtySpawnConfig) -> CommandBuilder {
    if let Some(program) = &config.program {
        let mut command = CommandBuilder::new(program.as_os_str());
        command.args(config.args.iter().map(String::as_str));
        command
    } else {
        CommandBuilder::new_default_prog()
    }
}

fn to_portable_size(size: PtySize) -> portable_pty::PtySize {
    let size = PtySize::new(size.rows, size.cols, size.pixel_width, size.pixel_height);
    portable_pty::PtySize {
        rows: size.rows,
        cols: size.cols,
        pixel_width: size.pixel_width,
        pixel_height: size.pixel_height,
    }
}

fn spawn_reader_thread(
    mut reader: Box<dyn Read + Send>,
    raw_tx: mpsc::Sender<Vec<u8>>,
    events: Arc<Mutex<VecDeque<PtyEvent>>>,
    shutdown: Arc<AtomicBool>,
) {
    thread::spawn(move || {
        let mut buf = [0_u8; 4096];
        loop {
            match reader.read(&mut buf) {
                Ok(0) => break,
                Ok(n) => {
                    if raw_tx.send(buf[..n].to_vec()).is_err() {
                        break;
                    }
                }
                Err(err) => {
                    if !shutdown.load(Ordering::SeqCst) {
                        events
                            .lock()
                            .expect("pty events")
                            .push_back(PtyEvent::Error(err.to_string()));
                    }
                    break;
                }
            }
        }
    });
}

fn spawn_batcher_thread(
    raw_rx: mpsc::Receiver<Vec<u8>>,
    events: Arc<Mutex<VecDeque<PtyEvent>>>,
    config: PtyBatchConfig,
) {
    thread::spawn(move || {
        let mut batcher = PtyOutputBatcher::with_config(config);
        loop {
            match raw_rx.recv_timeout(config.flush_timeout) {
                Ok(bytes) => {
                    for event in batcher.push(&bytes) {
                        events.lock().expect("pty events").push_back(event);
                    }
                }
                Err(mpsc::RecvTimeoutError::Timeout) => {
                    if let Some(event) = batcher.tick() {
                        events.lock().expect("pty events").push_back(event);
                    }
                }
                Err(mpsc::RecvTimeoutError::Disconnected) => {
                    if let Some(event) = batcher.flush() {
                        events.lock().expect("pty events").push_back(event);
                    }
                    break;
                }
            }
        }
    });
}

fn spawn_wait_thread(
    mut child: Box<dyn portable_pty::Child + Send + Sync>,
    events: Arc<Mutex<VecDeque<PtyEvent>>>,
    shutdown: Arc<AtomicBool>,
) {
    thread::spawn(move || {
        match child.wait() {
            Ok(status) => {
                events
                    .lock()
                    .expect("pty events")
                    .push_back(PtyEvent::Exit(PtyExitStatus {
                        success: status.success(),
                        exit_code: Some(status.exit_code()),
                        signal: status.signal().map(str::to_string),
                    }))
            }
            Err(err) => {
                if !shutdown.load(Ordering::SeqCst) {
                    events
                        .lock()
                        .expect("pty events")
                        .push_back(PtyEvent::Error(err.to_string()));
                }
            }
        }
        shutdown.store(true, Ordering::SeqCst);
    });
}
