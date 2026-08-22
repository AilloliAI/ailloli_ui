//! PTY runtime contracts and backends for Ailloli UI terminals.
//!
//! This crate is intentionally separate from `ailloli_ui_terminal_core`: it owns
//! system I/O only. It does not parse ANSI, render widgets, or integrate with
//! application-specific UI. The default feature set provides models, batching,
//! a handle abstraction, and an in-memory mock without opening OS resources.
//! Feature `portable` adds the native `PortablePtyBackend` with detached worker
//! threads and unbounded queues documented on that feature-gated type.
//!
//! # Examples
//!
//! ```
//! use ailloli_ui_terminal_pty::{MockPtyBackend, PtyBackend, PtyEvent, PtySpawnConfig};
//! let backend = MockPtyBackend::default();
//! backend.push_event(PtyEvent::Output(b"ready".to_vec()));
//! let handle = backend.spawn(PtySpawnConfig::default()).unwrap();
//! assert_eq!(handle.drain_events(), vec![PtyEvent::Output(b"ready".to_vec())]);
//! ```

mod batch;
mod error;
mod handle;
mod mock;
mod model;

#[cfg(feature = "portable")]
mod portable;

pub use batch::{PtyBatchConfig, PtyOutputBatcher};
pub use error::PtyError;
pub use handle::{PtyBackend, PtyHandle};
pub use mock::MockPtyBackend;
pub use model::{PtyEvent, PtyExitStatus, PtySize, PtySpawnConfig};

#[cfg(feature = "portable")]
pub use portable::PortablePtyBackend;

#[cfg(test)]
/// Cross-module unit tests for mock, batching, dimensions, and native smoke behavior.
mod tests {
    use std::thread;
    use std::time::Duration;

    use super::*;

    #[test]
    fn mock_spawn_records_config_and_drains_events_in_order() {
        let backend = MockPtyBackend::default();
        backend.push_event(PtyEvent::Output(b"hello".to_vec()));
        backend.push_event(PtyEvent::Exit(PtyExitStatus::success(0)));

        let config = PtySpawnConfig::default();
        let handle = backend.spawn(config.clone()).expect("spawn");

        assert_eq!(backend.spawned_configs(), vec![config]);
        assert_eq!(
            handle.drain_events(),
            vec![
                PtyEvent::Output(b"hello".to_vec()),
                PtyEvent::Exit(PtyExitStatus::success(0)),
            ]
        );
        assert!(handle.drain_events().is_empty());
    }

    #[test]
    fn mock_spawn_error_is_deterministic() {
        let backend = MockPtyBackend::default().with_spawn_error(PtyError::Spawn("boom".into()));

        assert_eq!(
            backend.spawn(PtySpawnConfig::default()).expect_err("error"),
            PtyError::Spawn("boom".into())
        );
    }

    #[test]
    fn mock_write_and_resize_are_recorded() {
        let backend = MockPtyBackend::default();
        let handle = backend.spawn(PtySpawnConfig::default()).expect("spawn");
        let size = PtySize::new(40, 100, 800, 600);

        handle.write(b"ls\r").expect("write");
        handle.resize(size).expect("resize");

        assert_eq!(backend.writes(), vec![b"ls\r".to_vec()]);
        assert_eq!(backend.resizes(), vec![size]);
    }

    #[test]
    fn mock_shutdown_is_idempotent_and_write_after_shutdown_is_closed() {
        let backend = MockPtyBackend::default();
        let handle = backend.spawn(PtySpawnConfig::default()).expect("spawn");

        assert!(!handle.is_shutdown());
        handle.shutdown().expect("shutdown");
        handle.shutdown().expect("shutdown twice");
        assert!(handle.is_shutdown());
        assert_eq!(handle.write(b"nope").expect_err("closed"), PtyError::Closed);
    }

    #[test]
    fn batcher_flushes_by_size_newline_timeout_and_preserves_order() {
        let mut batcher = PtyOutputBatcher::with_config(PtyBatchConfig {
            max_bytes: 4,
            flush_timeout: Duration::from_millis(5),
        });

        assert!(batcher.push(b"ab").is_empty());
        assert_eq!(
            batcher.push(b"cd"),
            vec![PtyEvent::Output(b"abcd".to_vec())]
        );
        assert_eq!(
            batcher.push(b"e\n"),
            vec![PtyEvent::Output(b"e\n".to_vec())]
        );

        assert!(batcher.push(b"fg").is_empty());
        thread::sleep(Duration::from_millis(20));
        assert_eq!(batcher.tick(), Some(PtyEvent::Output(b"fg".to_vec())));
    }

    #[test]
    fn pty_size_clamps_rows_and_cols() {
        let size = PtySize::new(0, 0, 20, 30);
        assert_eq!(size.rows, 1);
        assert_eq!(size.cols, 1);
        assert_eq!(size.pixel_width, 20);
        assert_eq!(size.pixel_height, 30);
    }

    #[cfg(feature = "portable")]
    #[test]
    #[ignore]
    fn portable_backend_can_spawn_echo_resize_and_shutdown() {
        let backend = PortablePtyBackend::new();
        let mut config = PtySpawnConfig {
            size: PtySize::new(12, 80, 0, 0),
            ..PtySpawnConfig::default()
        };
        #[cfg(windows)]
        {
            config.program = Some("cmd".into());
            config.args = vec![
                "/C".into(),
                "echo ailloli_ui-pty && ping -n 5 127.0.0.1 >NUL".into(),
            ];
        }
        #[cfg(not(windows))]
        {
            config.program = Some("sh".into());
            config.args = vec!["-lc".into(), "printf ailloli_ui-pty; sleep 5".into()];
        }

        let handle = backend.spawn(config).expect("spawn");
        let deadline = std::time::Instant::now() + Duration::from_secs(2);
        let mut output = Vec::new();
        while std::time::Instant::now() < deadline {
            for event in handle.drain_events() {
                if let PtyEvent::Output(bytes) = event {
                    output.extend(bytes);
                }
            }
            if output
                .windows(b"ailloli_ui-pty".len())
                .any(|w| w == b"ailloli_ui-pty")
            {
                break;
            }
            thread::sleep(Duration::from_millis(20));
        }
        handle.resize(PtySize::new(24, 100, 0, 0)).expect("resize");
        handle.shutdown().expect("shutdown");
        assert!(
            output
                .windows(b"ailloli_ui-pty".len())
                .any(|w| w == b"ailloli_ui-pty"),
            "output was {:?}",
            String::from_utf8_lossy(&output)
        );
    }
}
