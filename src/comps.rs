//! Composition guides and the meta tier list from HSReplay's public API
//! (the same endpoint HDT uses), fetched off the UI thread and cached on disk.

use std::path::{Path, PathBuf};
use std::sync::mpsc::{channel, Receiver};
use std::time::{Duration, Instant, SystemTime};

use anyhow::{Context, Result};
use serde::Deserialize;

use crate::cards::Tribe;
use crate::{net, paths};

const URL: &str = "https://hsreplay.net/api/v1/battlegrounds/comp_guides/?language=en";
const MAX_AGE: Duration = Duration::from_secs(6 * 3600);

#[derive(Debug, Clone, PartialEq)]
pub struct CompGuide {
    pub id: i32,
    pub name: String,
    /// 1 = S, 2 = A, …
    pub tier: i32,
    /// Order within the tier.
    pub tier_rank: i32,
    /// 1 easy … 3 hard.
    pub difficulty: i32,
    /// `None` for comps that are not built around one tribe.
    pub tribe: Option<Tribe>,
    /// Card dbf ids.
    pub core_cards: Vec<i32>,
    pub addon_cards: Vec<i32>,
    /// Card id whose art stands for the comp.
    pub representative_card: String,
    pub summary: String,
    pub how_to_play: String,
    pub when_to_commit: String,
    pub common_enablers: String,
    /// Tier in the previous meta snapshot, for the trend arrow.
    pub previous_tier: Option<i32>,
}

impl CompGuide {
    pub fn tier_letter(&self) -> &'static str {
        match self.tier {
            1 => "S",
            2 => "A",
            3 => "B",
            4 => "C",
            _ => "D",
        }
    }

    pub fn difficulty_label(&self) -> &'static str {
        match self.difficulty {
            1 => "Easy",
            2 => "Medium",
            3 => "Hard",
            _ => "",
        }
    }

    /// Positive when the comp moved up a tier since the last snapshot.
    pub fn trend(&self) -> i32 {
        self.previous_tier.map(|p| (p - self.tier).signum()).unwrap_or(0)
    }
}

#[derive(Deserialize)]
struct RawGuide {
    id: i32,
    #[serde(default)]
    name: String,
    #[serde(default)]
    tier: i32,
    #[serde(default)]
    tier_rank: i32,
    #[serde(default)]
    difficulty: i32,
    #[serde(default)]
    primary_tribe: i32,
    #[serde(default)]
    core_cards: Vec<i32>,
    #[serde(default)]
    addon_cards: Vec<i32>,
    #[serde(default)]
    representative_card: Option<String>,
    #[serde(default)]
    summary: Option<String>,
    #[serde(default)]
    how_to_play: Option<String>,
    #[serde(default)]
    when_to_commit: Option<String>,
    #[serde(default)]
    common_enablers: Option<String>,
    #[serde(default)]
    previous_tier: Option<i32>,
    #[serde(default)]
    hidden: bool,
}

/// HSReplay uses the game's `CARDRACE` values.
fn tribe_from_race(race: i32) -> Option<Tribe> {
    Some(match race {
        11 => Tribe::Undead,
        14 => Tribe::Murloc,
        15 => Tribe::Demon,
        17 => Tribe::Mech,
        18 => Tribe::Elemental,
        20 => Tribe::Beast,
        23 => Tribe::Pirate,
        24 => Tribe::Dragon,
        43 => Tribe::Quilboar,
        92 => Tribe::Naga,
        126 => Tribe::Aberration,
        _ => return None,
    })
}

/// Guide text marks cards as `[[Name||dbfId]]`; keeps just the name.
pub fn strip_card_refs(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    let mut rest = text;
    while let Some(start) = rest.find("[[") {
        let Some(len) = rest[start..].find("]]") else { break };
        out.push_str(&rest[..start]);
        let inner = &rest[start + 2..start + len];
        out.push_str(inner.split("||").next().unwrap_or(inner));
        rest = &rest[start + len + 2..];
    }
    out.push_str(rest);
    out.trim().to_string()
}

pub fn parse(bytes: &[u8]) -> Result<Vec<CompGuide>> {
    let raw: Vec<RawGuide> = serde_json::from_slice(bytes).context("parsing comp guides")?;
    let text = |t: Option<String>| strip_card_refs(&t.unwrap_or_default());
    let mut guides: Vec<CompGuide> = raw
        .into_iter()
        .filter(|g| !g.hidden && g.tier > 0)
        .map(|g| CompGuide {
            id: g.id,
            name: g.name.trim().to_string(),
            tier: g.tier,
            tier_rank: g.tier_rank,
            difficulty: g.difficulty,
            tribe: tribe_from_race(g.primary_tribe),
            core_cards: g.core_cards,
            addon_cards: g.addon_cards,
            representative_card: g.representative_card.unwrap_or_default(),
            summary: text(g.summary),
            how_to_play: text(g.how_to_play),
            when_to_commit: text(g.when_to_commit),
            common_enablers: text(g.common_enablers),
            previous_tier: g.previous_tier.filter(|&t| t > 0),
        })
        .collect();
    guides.sort_by(|a, b| a.tier.cmp(&b.tier).then(a.tier_rank.cmp(&b.tier_rank)).then_with(|| a.name.cmp(&b.name)));
    Ok(guides)
}

fn cache_path() -> PathBuf {
    paths::cache_dir().join("comp_guides.json")
}

fn age(path: &Path) -> Option<Duration> {
    let modified = std::fs::metadata(path).and_then(|m| m.modified()).ok()?;
    SystemTime::now().duration_since(modified).ok()
}

/// The cached guides, downloaded again when missing or stale. A stale copy is
/// still used when the download fails.
pub fn load_or_fetch() -> Result<Vec<CompGuide>> {
    let path = cache_path();
    let age = age(&path);
    if age.is_none_or(|a| a > MAX_AGE) {
        log::info!("downloading comp guides");
        if let Err(e) = net::download(URL, &path) {
            if age.is_none() {
                return Err(e.context("could not download the comp guides"));
            }
            log::warn!("{e:#}");
        }
    }
    let bytes = std::fs::read(&path).with_context(|| format!("reading {}", path.display()))?;
    parse(&bytes)
}

pub enum CompsState {
    Loading,
    Ready(Vec<CompGuide>),
    Failed,
}

/// Guides as the UI sees them, loaded in the background.
pub struct Comps {
    pub state: CompsState,
    rx: Option<Receiver<Result<Vec<CompGuide>, String>>>,
    loaded_at: Option<Instant>,
}

impl Comps {
    /// Nothing loaded yet; `refresh` starts the download.
    pub fn new() -> Comps {
        Comps { state: CompsState::Loading, rx: None, loaded_at: None }
    }

    pub fn with_guides(guides: Vec<CompGuide>) -> Comps {
        Comps { state: CompsState::Ready(guides), rx: None, loaded_at: Some(Instant::now()) }
    }

    fn reload(&mut self) {
        if self.rx.is_some() {
            return;
        }
        let (tx, rx) = channel();
        std::thread::spawn(move || {
            let _ = tx.send(load_or_fetch().map_err(|e| format!("{e:#}")));
        });
        self.rx = Some(rx);
    }

    /// Collects a finished load. Returns true when the state changed.
    pub fn poll(&mut self) -> bool {
        let Some(rx) = &self.rx else { return false };
        let Ok(result) = rx.try_recv() else { return false };
        self.rx = None;
        match result {
            Ok(guides) => {
                log::info!("{} comp guides", guides.len());
                self.state = CompsState::Ready(guides);
                self.loaded_at = Some(Instant::now());
            }
            Err(e) => {
                log::warn!("{e}");
                // Keep guides we already have over an error.
                if !matches!(self.state, CompsState::Ready(_)) {
                    self.state = CompsState::Failed;
                }
            }
        }
        true
    }

    /// Loads the guides the first time, again after a failure, and when the
    /// tier list may have changed (the overlay can run for days).
    pub fn refresh(&mut self) {
        let stale = self.loaded_at.is_none_or(|t| t.elapsed() > MAX_AGE);
        if matches!(self.state, CompsState::Failed) || stale {
            if matches!(self.state, CompsState::Failed) {
                self.state = CompsState::Loading;
            }
            self.reload();
        }
    }

    pub fn guides(&self) -> &[CompGuide] {
        match &self.state {
            CompsState::Ready(g) => g,
            _ => &[],
        }
    }

    pub fn get(&self, id: i32) -> Option<&CompGuide> {
        self.guides().iter().find(|g| g.id == id)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn strips_card_references() {
        assert_eq!(strip_card_refs("Get [[Sky Admiral Rogers||122516]] + [[Proud Privateer||122520]]."), "Get Sky Admiral Rogers + Proud Privateer.");
        assert_eq!(strip_card_refs("no refs [[broken"), "no refs [[broken");
    }

    #[test]
    fn parses_sorts_and_hides() {
        let json = r#"[
            {"id":1,"name":"Late","tier":2,"tier_rank":0,"difficulty":3,"primary_tribe":17,"core_cards":[5],"representative_card":"X","previous_tier":1},
            {"id":2,"name":"Best","tier":1,"tier_rank":1,"difficulty":1,"primary_tribe":126,"summary":"Use [[Card||9]]","previous_tier":null},
            {"id":3,"name":"Hidden","tier":1,"tier_rank":0,"hidden":true},
            {"id":4,"name":"Mixed","tier":1,"tier_rank":0,"primary_tribe":0,"how_to_play":null,"previous_tier":2}
        ]"#;
        let g = parse(json.as_bytes()).unwrap();
        assert_eq!(g.iter().map(|g| g.id).collect::<Vec<_>>(), vec![4, 2, 1]);
        assert_eq!(g[0].tribe, None);
        assert_eq!(g[1].tribe, Some(Tribe::Aberration));
        assert_eq!(g[1].summary, "Use Card");
        assert_eq!((g[0].trend(), g[1].trend(), g[2].trend()), (1, 0, -1));
        assert_eq!(g[2].tier_letter(), "A");
    }
}
