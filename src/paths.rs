//! Locating Hearthstone (under Wine/Proton) and the tracker's own folders.

use std::path::{Path, PathBuf};

use anyhow::{bail, Context, Result};

const APP: &str = "hsbg-overlay";

pub fn cache_dir() -> PathBuf {
    dirs::cache_dir().unwrap_or_else(std::env::temp_dir).join(APP)
}

pub fn data_dir() -> PathBuf {
    dirs::data_dir().unwrap_or_else(std::env::temp_dir).join(APP)
}

pub fn config_path() -> PathBuf {
    dirs::config_dir().unwrap_or_else(std::env::temp_dir).join(APP).join("config.toml")
}

fn looks_like_install(dir: &Path) -> bool {
    dir.join("Hearthstone.exe").is_file() || dir.join("Logs").is_dir()
}

/// The working directory of a running `Hearthstone.exe` is its install folder.
fn from_running_process() -> Option<PathBuf> {
    let pid = crate::memory::hearthstone_pid()?;
    let cwd = std::fs::read_link(format!("/proc/{pid}/cwd")).ok()?;
    looks_like_install(&cwd).then_some(cwd)
}

fn children(dir: &Path) -> Vec<PathBuf> {
    std::fs::read_dir(dir)
        .map(|it| it.flatten().map(|e| e.path()).collect())
        .unwrap_or_default()
}

/// Wine prefixes in the usual places (Lutris, Steam/Proton, Bottles, plain Wine).
fn wine_prefixes() -> Vec<PathBuf> {
    let Some(home) = dirs::home_dir() else { return vec![] };
    let mut prefixes = vec![home.join(".wine")];
    prefixes.extend(children(&home.join("Games")));
    prefixes.extend(children(&home.join(".local/share/bottles/bottles")));
    for steam in [home.join(".local/share/Steam"), home.join(".steam/steam")] {
        prefixes.extend(children(&steam.join("steamapps/compatdata")).into_iter().map(|p| p.join("pfx")));
    }
    if let Ok(prefix) = std::env::var("WINEPREFIX") {
        prefixes.insert(0, PathBuf::from(prefix));
    }
    prefixes
}

pub fn find_hearthstone_dir(configured: Option<&Path>) -> Result<PathBuf> {
    if let Some(dir) = configured {
        if looks_like_install(dir) {
            return Ok(dir.to_path_buf());
        }
        bail!("hearthstone_dir {} does not look like a Hearthstone install", dir.display());
    }
    if let Some(dir) = from_running_process() {
        return Ok(dir);
    }
    for prefix in wine_prefixes() {
        for pf in ["Program Files (x86)", "Program Files"] {
            let dir = prefix.join("drive_c").join(pf).join("Hearthstone");
            if looks_like_install(&dir) {
                return Ok(dir);
            }
        }
    }
    bail!("Hearthstone install not found; set hearthstone_dir in {}", config_path().display())
}

const POWER_SECTION: &str = "[Power]\nLogLevel=1\nFilePrinting=True\nConsolePrinting=False\nScreenPrinting=False\nVerbose=True\n";

/// Makes sure `log.config` enables the verbose Power log. Returns the file
/// path when it had to be written (the game must be restarted to pick it up).
pub fn ensure_log_config(hs_dir: &Path) -> Result<Option<PathBuf>> {
    let prefix = hs_dir
        .ancestors()
        .find(|p| p.file_name().is_some_and(|n| n == "drive_c"))
        .context("Hearthstone is not inside a Wine prefix (no drive_c)")?;
    let mut configs: Vec<PathBuf> = children(&prefix.join("users"))
        .into_iter()
        .flat_map(|u| {
            [
                u.join("AppData/Local/Blizzard/Hearthstone/log.config"),
                u.join("Local Settings/Application Data/Blizzard/Hearthstone/log.config"),
            ]
        })
        .collect();
    configs.retain(|p| p.parent().is_some_and(|d| d.is_dir()));
    let Some(path) = configs.first() else {
        bail!("Blizzard/Hearthstone settings folder not found under {}", prefix.join("users").display());
    };
    let current = std::fs::read_to_string(path).unwrap_or_default();
    let (updated, changed) = with_power_section(&current);
    if !changed {
        return Ok(None);
    }
    std::fs::write(path, updated).with_context(|| format!("writing {}", path.display()))?;
    Ok(Some(path.clone()))
}

const CLIENT_CONFIG_SECTION: &str = "[Log]\nFileSizeLimit.Int = -1\n";

/// Ensures `client.config` in the Hearthstone root folder disables the default
/// 10MB log file size limit (`FileSizeLimit.Int = -1`). Without this, Hearthstone
/// stops logging to `Power.log` mid-session once 10000KB is reached.
pub fn ensure_client_config(hs_dir: &Path) -> Result<Option<PathBuf>> {
    let path = hs_dir.join("client.config");
    let current = std::fs::read_to_string(&path).unwrap_or_default();
    let (updated, changed) = with_file_size_limit(&current);
    if !changed {
        return Ok(None);
    }
    std::fs::write(&path, updated).with_context(|| format!("writing {}", path.display()))?;
    Ok(Some(path))
}

pub fn with_file_size_limit(config: &str) -> (String, bool) {
    let mut out = String::new();
    let mut in_log = false;
    let mut has_log_section = false;
    let mut has_limit = false;

    for line in config.lines() {
        let trimmed = line.trim();
        if trimmed.starts_with('[') {
            if trimmed.eq_ignore_ascii_case("[Log]") {
                in_log = true;
                has_log_section = true;
                out.push_str("[Log]\n");
                continue;
            } else if trimmed.eq_ignore_ascii_case("[Logging]") {
                // Migrate legacy / incorrect [Logging] to [Log]
                in_log = true;
                has_log_section = true;
                out.push_str("[Log]\n");
                continue;
            } else {
                in_log = false;
            }
        } else if in_log {
            if let Some((k, v)) = trimmed.split_once('=') {
                if k.trim().eq_ignore_ascii_case("FileSizeLimit.Int") {
                    has_limit = true;
                    if v.trim() == "-1" {
                        out.push_str("FileSizeLimit.Int = -1\n");
                        continue;
                    }
                    out.push_str("FileSizeLimit.Int = -1\n");
                    continue;
                }
            }
        }
        out.push_str(line);
        out.push('\n');
    }

    if has_limit {
        let changed = out != config;
        (out, changed)
    } else if has_log_section {
        // Append FileSizeLimit.Int to the existing [Log] section
        let mut final_out = String::new();
        let mut in_target = false;
        let mut inserted = false;
        for line in out.lines() {
            let trimmed = line.trim();
            if trimmed.starts_with('[') {
                if in_target && !inserted {
                    final_out.push_str("FileSizeLimit.Int = -1\n");
                    inserted = true;
                }
                in_target = trimmed.eq_ignore_ascii_case("[Log]");
            }
            final_out.push_str(line);
            final_out.push('\n');
        }
        if !inserted {
            final_out.push_str("FileSizeLimit.Int = -1\n");
        }
        (final_out, true)
    } else {
        if !out.is_empty() && !out.ends_with('\n') {
            out.push('\n');
        }
        out.push_str(CLIENT_CONFIG_SECTION);
        (out, true)
    }
}

/// Replaces (or appends) the `[Power]` section; other sections are kept.
fn with_power_section(config: &str) -> (String, bool) {
    let mut out = String::new();
    let mut in_power = false;
    let mut existing = String::new();
    for line in config.lines() {
        let trimmed = line.trim();
        if trimmed.starts_with('[') {
            in_power = trimmed.eq_ignore_ascii_case("[Power]");
            if in_power {
                existing.push_str(line);
                existing.push('\n');
                continue;
            }
        }
        if in_power {
            existing.push_str(line);
            existing.push('\n');
        } else {
            out.push_str(line);
            out.push('\n');
        }
    }
    let ok = |key: &str, val: &str| {
        existing.lines().any(|l| {
            l.split_once('=')
                .is_some_and(|(k, v)| k.trim().eq_ignore_ascii_case(key) && v.trim().eq_ignore_ascii_case(val))
        })
    };
    if ok("LogLevel", "1") && ok("FilePrinting", "True") && ok("Verbose", "True") {
        return (config.to_string(), false);
    }
    out.push_str(POWER_SECTION);
    (out, true)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn keeps_good_power_section() {
        let cfg = "[Power]\nLogLevel=1\nFilePrinting=True\nConsolePrinting=False\nScreenPrinting=False\nVerbose=True\n";
        assert_eq!(with_power_section(cfg), (cfg.to_string(), false));
    }

    #[test]
    fn rewrites_non_verbose_power_and_keeps_others() {
        let cfg = "[Arena]\nLogLevel=1\n[Power]\nLogLevel=1\nVerbose=False\n[LoadingScreen]\nLogLevel=1\n";
        let (out, changed) = with_power_section(cfg);
        assert!(changed);
        assert!(out.starts_with("[Arena]\nLogLevel=1\n[LoadingScreen]\nLogLevel=1\n"));
        assert!(out.ends_with(POWER_SECTION));
    }

    #[test]
    fn adds_file_size_limit_when_empty() {
        let (out, changed) = with_file_size_limit("");
        assert!(changed);
        assert_eq!(out, CLIENT_CONFIG_SECTION);
    }

    #[test]
    fn keeps_existing_file_size_limit() {
        let cfg = "[Log]\nFileSizeLimit.Int = -1\n";
        let (out, changed) = with_file_size_limit(cfg);
        assert!(!changed);
        assert_eq!(out, cfg);
    }

    #[test]
    fn updates_different_file_size_limit() {
        let cfg = "[Log]\nFileSizeLimit.Int = 10000\n";
        let (out, changed) = with_file_size_limit(cfg);
        assert!(changed);
        assert_eq!(out, "[Log]\nFileSizeLimit.Int = -1\n");
    }

    #[test]
    fn migrates_logging_to_log() {
        let cfg = "[Logging]\nFileSizeLimit.Int = -1\n";
        let (out, changed) = with_file_size_limit(cfg);
        assert!(changed);
        assert_eq!(out, "[Log]\nFileSizeLimit.Int = -1\n");
    }
}
