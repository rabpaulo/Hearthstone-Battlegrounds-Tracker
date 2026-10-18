use std::path::{Path, PathBuf};

use anyhow::{Context, Result};
use serde::Deserialize;

use crate::ui::theme::ThemeName;

/// `~/.config/hsbg-overlay/config.toml`; every key is optional.
#[derive(Debug, Clone, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct Config {
    /// Hearthstone install folder (the one containing `Logs/`). Found
    /// automatically from the running game or common Wine prefixes.
    pub hearthstone_dir: Option<PathBuf>,
    /// UI scale on top of the automatic scaling to the game window height.
    pub scale: f32,
    /// Panel colour theme; the settings panel switches it live.
    pub theme: ThemeName,
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
    /// "Search" button: find any Battlegrounds card by name.
    pub show_search: bool,
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
        LeaderboardGeometry { center_x: -0.62, top: 0.157, spacing: 0.0865, width: 0.085, height: 0.083 }
    }
}

impl Default for Config {
    fn default() -> Self {
        Config {
            hearthstone_dir: None,
            scale: 1.0,
            theme: ThemeName::default(),
            simulations: 8000,
            node: "node".into(),
            sidecar_dir: None,
            hide_when_unfocused: true,
            show_simulation: true,
            show_tiers: true,
            show_session: true,
            show_opponent_boards: true,
            show_comps: true,
            show_search: true,
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

/// Writes the theme picked in the settings panel, keeping the rest of the
/// file (comments, keys and sections).
pub fn save_theme(path: &Path, name: ThemeName) -> Result<()> {
    let target = std::fs::canonicalize(path).unwrap_or_else(|_| path.to_path_buf());
    let current = match std::fs::read_to_string(&target) {
        Ok(text) => text,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => String::new(),
        Err(e) => return Err(e).with_context(|| format!("reading {}", target.display())),
    };
    let updated = set_top_level_key(&current, "theme", name.config_name());
    if let Some(dir) = target.parent() {
        std::fs::create_dir_all(dir).with_context(|| format!("creating {}", dir.display()))?;
    }
    // Write, then rename: a crash mid-write cannot leave a truncated config.
    let tmp = target.with_extension("toml.tmp");
    std::fs::write(&tmp, updated).with_context(|| format!("writing {}", tmp.display()))?;
    std::fs::rename(&tmp, &target).with_context(|| format!("renaming {}", tmp.display()))?;
    Ok(())
}

/// Sets a top-level `key = "value"` line, leaving comments and the other
/// lines as they are. A missing key is inserted before the first `[section]`,
/// or appended when the file has none.
fn set_top_level_key(config: &str, key: &str, value: &str) -> String {
    let line = format!("{key} = \"{value}\"");
    let mut out = String::with_capacity(config.len() + line.len() + 1);
    let mut in_section = false;
    let mut done = false;
    for l in config.lines() {
        let trimmed = l.trim();
        if !in_section && trimmed.starts_with('[') {
            if !done {
                out.push_str(&line);
                out.push('\n');
                done = true;
            }
            in_section = true;
        } else if !in_section && trimmed.split_once('=').is_some_and(|(k, _)| k.trim() == key) {
            if !done {
                out.push_str(&line);
                out.push('\n');
                done = true;
            }
            continue;
        }
        out.push_str(l);
        out.push('\n');
    }
    if !done {
        out.push_str(&line);
        out.push('\n');
    }
    out
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
        assert_eq!(c.theme, ThemeName::Dark);
    }

    #[test]
    fn theme_round_trips_through_the_config() {
        let c: Config = toml::from_str("theme = \"high-contrast\"\n").unwrap();
        assert_eq!(c.theme, ThemeName::HighContrast);
        let c2: Config = toml::from_str("theme = \"high_contrast\"\n").unwrap();
        assert_eq!(c2.theme, ThemeName::HighContrast);
        let c3: Config = toml::from_str("theme = \"Dark\"\n").unwrap();
        assert_eq!(c3.theme, ThemeName::Dark);
        assert!(toml::from_str::<Config>("theme = \"neon\"").is_err());
        for name in ThemeName::ALL {
            let text = format!("theme = \"{}\"\n", name.config_name());
            assert_eq!(toml::from_str::<Config>(&text).unwrap().theme, name);
        }
    }

    #[test]
    fn sets_the_theme_key_without_touching_the_rest() {
        let cfg = "# my config\ntheme = \"light\"\nscale = 2.0\n\n[leaderboard]\ntop = 0.2\n";
        assert_eq!(
            set_top_level_key(cfg, "theme", "dark"),
            "# my config\ntheme = \"dark\"\nscale = 2.0\n\n[leaderboard]\ntop = 0.2\n"
        );
        // Duplicate keys at top level are cleaned up.
        let dup = "theme = \"dark\"\ntheme = \"light\"\nscale = 1.0\n";
        assert_eq!(set_top_level_key(dup, "theme", "high-contrast"), "theme = \"high-contrast\"\nscale = 1.0\n");
        // Missing keys go before the first table, or at the end.
        assert_eq!(
            set_top_level_key("scale = 2.0\n[leaderboard]\ntop = 0.2\n", "theme", "light"),
            "scale = 2.0\ntheme = \"light\"\n[leaderboard]\ntop = 0.2\n"
        );
        assert_eq!(set_top_level_key("scale = 2.0\n", "theme", "light"), "scale = 2.0\ntheme = \"light\"\n");
        // A key of the same name inside a table is not the one being set.
        assert_eq!(
            set_top_level_key("[leaderboard]\ntheme = \"light\"\n", "theme", "dark"),
            "theme = \"dark\"\n[leaderboard]\ntheme = \"light\"\n"
        );
    }

    #[test]
    fn save_theme_keeps_comments_and_other_keys() {
        let dir = std::env::temp_dir().join(format!("hsbg-config-theme-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("config.toml");
        std::fs::write(&path, "# keep me\ntheme = \"dark\"\n# and me\n[leaderboard]\ntop = 0.2\n").unwrap();
        save_theme(&path, ThemeName::Light).unwrap();
        assert_eq!(
            std::fs::read_to_string(&path).unwrap(),
            "# keep me\ntheme = \"light\"\n# and me\n[leaderboard]\ntop = 0.2\n"
        );
        let c = Config::load(&path).unwrap();
        assert_eq!((c.theme, c.leaderboard.top), (ThemeName::Light, 0.2));
        // A config (and its folder) that do not exist yet are created.
        let fresh = dir.join("sub/new.toml");
        save_theme(&fresh, ThemeName::HighContrast).unwrap();
        assert_eq!(std::fs::read_to_string(&fresh).unwrap(), "theme = \"high-contrast\"\n");
        // Symlinks are preserved rather than overwritten by rename.
        #[cfg(unix)]
        {
            let real = dir.join("real_config.toml");
            std::fs::write(&real, "theme = \"dark\"\n").unwrap();
            let link = dir.join("symlink_config.toml");
            std::os::unix::fs::symlink(&real, &link).unwrap();
            save_theme(&link, ThemeName::Light).unwrap();
            assert!(link.is_symlink(), "symlink must be preserved");
            assert_eq!(std::fs::read_to_string(&real).unwrap(), "theme = \"light\"\n");
        }
        let _ = std::fs::remove_dir_all(&dir);
    }
}
