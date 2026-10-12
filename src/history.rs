//! Finished games, persisted as JSON for the "latest games" panel.

use std::path::PathBuf;
use std::time::{SystemTime, UNIX_EPOCH};

use serde::{Deserialize, Serialize};

use crate::paths;

const KEEP: usize = 300;
/// Games further apart than this belong to different sessions.
const SESSION_GAP_SECS: u64 = 3 * 3600;

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct GameRecord {
    pub id: String,
    pub hero: String,
    pub place: i32,
    pub turn: i32,
    pub ended_at: u64,
    #[serde(default)]
    pub tribes: Vec<String>,
}

pub struct History {
    path: PathBuf,
    games: Vec<GameRecord>,
}

pub fn now_secs() -> u64 {
    SystemTime::now().duration_since(UNIX_EPOCH).map(|d| d.as_secs()).unwrap_or(0)
}

impl History {
    pub fn load() -> History {
        let path = paths::data_dir().join("games.json");
        let games = std::fs::read(&path)
            .ok()
            .and_then(|b| serde_json::from_slice(&b).map_err(|e| log::warn!("{}: {e}", path.display())).ok())
            .unwrap_or_default();
        History { path, games }
    }

    /// A history that is never written to disk.
    pub fn in_memory(games: Vec<GameRecord>) -> History {
        History { path: PathBuf::new(), games }
    }

    /// Adds a finished game unless it is already known (the log is replayed
    /// from the start whenever the overlay launches mid-session).
    pub fn add(&mut self, record: GameRecord) -> bool {
        if self.games.iter().any(|g| g.id == record.id) {
            return false;
        }
        self.games.push(record);
        if self.games.len() > KEEP {
            self.games.drain(..self.games.len() - KEEP);
        }
        if self.path.as_os_str().is_empty() {
            return true;
        }
        if let Some(dir) = self.path.parent() {
            let _ = std::fs::create_dir_all(dir);
        }
        match serde_json::to_vec_pretty(&self.games) {
            Ok(json) => {
                if let Err(e) = std::fs::write(&self.path, json) {
                    log::warn!("saving {}: {e}", self.path.display());
                }
            }
            Err(e) => log::warn!("{e}"),
        }
        true
    }

    /// Most recent first.
    pub fn latest(&self, n: usize) -> Vec<&GameRecord> {
        self.games.iter().rev().take(n).collect()
    }

    /// Games of the current session (consecutive games less than a few hours
    /// apart, the last of them recent).
    pub fn session(&self, now: u64) -> Vec<&GameRecord> {
        let mut out = Vec::new();
        let mut next = now;
        for g in self.games.iter().rev() {
            if next.saturating_sub(g.ended_at) > SESSION_GAP_SECS {
                break;
            }
            out.push(g);
            next = g.ended_at;
        }
        out
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn rec(id: &str, place: i32, at: u64) -> GameRecord {
        GameRecord { id: id.into(), hero: "H".into(), place, turn: 10, ended_at: at, tribes: vec![] }
    }

    #[test]
    fn session_stops_at_long_gap() {
        let h = History::in_memory(vec![rec("a", 1, 1_000), rec("b", 2, 20_000), rec("c", 3, 21_000)]);
        let s = h.session(22_000);
        assert_eq!(s.iter().map(|g| g.id.as_str()).collect::<Vec<_>>(), vec!["c", "b"]);
        assert!(h.session(100_000).is_empty());
    }
}
