//! Follows the newest `Power.log` under Hearthstone's `Logs` directory.
//!
//! Current clients write one folder per launch
//! (`Logs/Hearthstone_2026_10_12_02_07_17/Power.log`); older ones wrote
//! `Logs/Power.log` directly. Both layouts are handled, and a newer session
//! folder is picked up automatically when the game is restarted.

use std::fs::File;
use std::io::{Read, Seek, SeekFrom};
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

const RESCAN_INTERVAL: Duration = Duration::from_secs(2);

pub struct Poll {
    /// A different file was opened; game state must be discarded.
    pub reset: bool,
    /// Lines that were already in the file when the tracker started.
    pub catchup: bool,
    pub lines: Vec<String>,
}

pub struct LogTailer {
    logs_root: PathBuf,
    path: Option<PathBuf>,
    file: Option<File>,
    pos: u64,
    pending: Vec<u8>,
    last_scan: Option<Instant>,
    first_open: bool,
}

impl LogTailer {
    pub fn new(logs_root: PathBuf) -> Self {
        Self {
            logs_root,
            path: None,
            file: None,
            pos: 0,
            pending: Vec::new(),
            last_scan: None,
            first_open: true,
        }
    }

    pub fn current_path(&self) -> Option<&Path> {
        self.path.as_deref()
    }

    pub fn poll(&mut self) -> Poll {
        let mut reset = false;
        let mut catchup = false;
        if self.last_scan.is_none_or(|t| t.elapsed() >= RESCAN_INTERVAL) {
            self.last_scan = Some(Instant::now());
            if let Some(newest) = newest_power_log(&self.logs_root) {
                if self.path.as_ref() != Some(&newest) {
                    match File::open(&newest) {
                        Ok(f) => {
                            log::info!("following {}", newest.display());
                            self.file = Some(f);
                            self.path = Some(newest);
                            self.pos = 0;
                            self.pending.clear();
                            reset = true;
                            catchup = self.first_open;
                            self.first_open = false;
                        }
                        Err(e) => log::warn!("cannot open {}: {e}", newest.display()),
                    }
                }
            } else {
                // Nothing to follow yet; anything opened later is live.
                self.first_open = false;
            }
        }

        let mut lines = Vec::new();
        let Some(file) = self.file.as_mut() else {
            return Poll { reset, catchup, lines };
        };
        let len = file.metadata().map(|m| m.len()).unwrap_or(0);
        if len < self.pos {
            // Truncated in place (log rotation without a new folder).
            self.pos = 0;
            self.pending.clear();
            reset = true;
        }
        if len == self.pos {
            return Poll { reset, catchup, lines };
        }
        let mut buf = Vec::with_capacity((len - self.pos) as usize);
        if file.seek(SeekFrom::Start(self.pos)).is_ok() && file.read_to_end(&mut buf).is_ok() {
            self.pos += buf.len() as u64;
            self.pending.extend_from_slice(&buf);
            let cut = self.pending.iter().rposition(|&b| b == b'\n').map(|p| p + 1).unwrap_or(0);
            let complete: Vec<u8> = self.pending.drain(..cut).collect();
            lines = String::from_utf8_lossy(&complete)
                .lines()
                .map(|l| l.trim_end_matches('\r').to_string())
                .collect();
        }
        Poll { reset, catchup, lines }
    }
}

/// The `Power.log` of the most recent session folder, or the legacy flat one.
pub fn newest_power_log(logs_root: &Path) -> Option<PathBuf> {
    let mut sessions: Vec<PathBuf> = std::fs::read_dir(logs_root)
        .ok()?
        .filter_map(|e| e.ok())
        .map(|e| e.path())
        .filter(|p| {
            p.is_dir()
                && p.file_name()
                    .and_then(|n| n.to_str())
                    .is_some_and(|n| n.starts_with("Hearthstone_"))
        })
        .collect();
    // Folder names embed a zero-padded timestamp, so lexical order is
    // chronological.
    sessions.sort();
    for dir in sessions.iter().rev() {
        let p = dir.join("Power.log");
        if p.is_file() {
            return Some(p);
        }
        // The newest session has no Power.log until a game starts; older
        // sessions' logs are stale then, so do not fall back to them.
        if dir == sessions.last().unwrap() {
            return None;
        }
    }
    let flat = logs_root.join("Power.log");
    flat.is_file().then_some(flat)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;

    fn tmpdir(name: &str) -> PathBuf {
        let d = std::env::temp_dir().join(format!("hsbg-tail-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&d);
        std::fs::create_dir_all(&d).unwrap();
        d
    }

    #[test]
    fn picks_newest_session_and_tails_lines() {
        let root = tmpdir("newest");
        let old = root.join("Hearthstone_2026_01_01_10_00_00");
        let new = root.join("Hearthstone_2026_01_02_10_00_00");
        std::fs::create_dir_all(&old).unwrap();
        std::fs::create_dir_all(&new).unwrap();
        std::fs::write(old.join("Power.log"), "old\n").unwrap();
        std::fs::write(new.join("Power.log"), "a\nb").unwrap();

        let mut t = LogTailer::new(root.clone());
        let p = t.poll();
        assert!(p.reset && p.catchup);
        assert_eq!(p.lines, vec!["a"]);

        let mut f = std::fs::OpenOptions::new().append(true).open(new.join("Power.log")).unwrap();
        f.write_all(b"c\nd\n").unwrap();
        let p = t.poll();
        assert!(!p.reset);
        assert_eq!(p.lines, vec!["bc", "d"]);
        std::fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn newest_session_without_power_log_yields_nothing() {
        let root = tmpdir("empty");
        let old = root.join("Hearthstone_2026_01_01_10_00_00");
        std::fs::create_dir_all(&old).unwrap();
        std::fs::write(old.join("Power.log"), "x\n").unwrap();
        std::fs::create_dir_all(root.join("Hearthstone_2026_01_02_10_00_00")).unwrap();
        assert_eq!(newest_power_log(&root), None);
        std::fs::remove_dir_all(root).unwrap();
    }
}
