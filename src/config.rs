use std::path::{Path, PathBuf};

use anyhow::{Context, Result};
use serde::Deserialize;

/// `~/.config/hsbg-overlay/config.toml`; every key is optional.
#[derive(Debug, Clone, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct Config {
    /// Hearthstone install folder (the one containing `Logs/`). Found
    /// automatically from the running game or common Wine prefixes.
    pub hearthstone_dir: Option<PathBuf>,
    /// UI scale on top of the automatic scaling to the game window height.
    pub scale: f32,
    /// Combat simulations per fight.
    pub simulations: u32,
    /// Node.js executable used for the simulator.
    pub node: String,
    /// Folder holding `sim.mjs` and its `node_modules`.
    pub sidecar_dir: Option<PathBuf>,
    /// Hide the overlay while another window has focus.
    pub hide_when_unfocused: bool,
    pub show_simulation: bool,
    pub show_tiers: bool,
    pub show_session: bool,
    pub show_opponent_boards: bool,
    /// "Comps" button: HSReplay's meta tier list and comp guides.
    pub show_comps: bool,
    /// Tier list starts with only the lobby's tribes (toggle in its header).
    pub hide_unavailable_tribes: bool,
    /// Read the lobby tribes from the game's memory (known from the hero
    /// pick on). Without it they are inferred from the shop.
    pub read_game_memory: bool,
    pub leaderboard: LeaderboardGeometry,
}

/// Where the game draws its leaderboard portraits, as fractions of the game
/// window height (the game scales its UI with height and centres it).
#[derive(Debug, Clone, Copy, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct LeaderboardGeometry {
    /// Portrait column centre, relative to the window's horizontal centre.
    pub center_x: f32,
    /// Top of the first portrait.
    pub top: f32,
    /// Distance between consecutive portraits.
    pub spacing: f32,
    pub width: f32,
    pub height: f32,
}

impl Default for LeaderboardGeometry {
    fn default() -> Self {
        LeaderboardGeometry { center_x: -0.697, top: 0.178, spacing: 0.098, width: 0.09, height: 0.088 }
    }
}

impl Default for Config {
    fn default() -> Self {
        Config {
            hearthstone_dir: None,
            scale: 1.0,
            simulations: 8000,
            node: "node".into(),
            sidecar_dir: None,
            hide_when_unfocused: true,
            show_simulation: true,
            show_tiers: true,
            show_session: true,
            show_opponent_boards: true,
            show_comps: true,
            hide_unavailable_tribes: true,
            read_game_memory: true,
            leaderboard: LeaderboardGeometry::default(),
        }
    }
}

impl Config {
    pub fn load(path: &Path) -> Result<Config> {
        match std::fs::read_to_string(path) {
            Ok(text) => toml::from_str(&text).with_context(|| format!("invalid config {}", path.display())),
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(Config::default()),
            Err(e) => Err(e).with_context(|| format!("reading {}", path.display())),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn partial_config_keeps_defaults() {
        let c: Config = toml::from_str("scale = 1.25\n[leaderboard]\ntop = 0.2\n").unwrap();
        assert_eq!(c.scale, 1.25);
        assert_eq!(c.leaderboard.top, 0.2);
        assert_eq!(c.leaderboard.spacing, LeaderboardGeometry::default().spacing);
        assert_eq!(c.simulations, 8000);
    }
}
