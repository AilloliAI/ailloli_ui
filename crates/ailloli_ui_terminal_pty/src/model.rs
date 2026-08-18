use std::path::PathBuf;

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct PtySize {
    pub rows: u16,
    pub cols: u16,
    pub pixel_width: u16,
    pub pixel_height: u16,
}

impl PtySize {
    pub const fn new(rows: u16, cols: u16, pixel_width: u16, pixel_height: u16) -> Self {
        Self {
            rows: if rows == 0 { 1 } else { rows },
            cols: if cols == 0 { 1 } else { cols },
            pixel_width,
            pixel_height,
        }
    }
}

impl Default for PtySize {
    fn default() -> Self {
        Self::new(24, 80, 0, 0)
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PtySpawnConfig {
    pub program: Option<PathBuf>,
    pub args: Vec<String>,
    pub cwd: Option<PathBuf>,
    pub env: Vec<(String, String)>,
    pub term: String,
    pub size: PtySize,
}

impl Default for PtySpawnConfig {
    fn default() -> Self {
        Self {
            program: None,
            args: Vec::new(),
            cwd: None,
            env: Vec::new(),
            term: "xterm-256color".to_string(),
            size: PtySize::default(),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PtyExitStatus {
    pub success: bool,
    pub exit_code: Option<u32>,
    pub signal: Option<String>,
}

impl PtyExitStatus {
    pub fn success(exit_code: u32) -> Self {
        Self {
            success: true,
            exit_code: Some(exit_code),
            signal: None,
        }
    }

    pub fn failure(exit_code: Option<u32>, signal: Option<String>) -> Self {
        Self {
            success: false,
            exit_code,
            signal,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum PtyEvent {
    Output(Vec<u8>),
    Exit(PtyExitStatus),
    Error(String),
}
