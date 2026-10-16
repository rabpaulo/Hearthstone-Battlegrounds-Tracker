//! Heroes that enable comps, from Firestone's public comp stats. A hero is an
//! enabler when its players end the game in a comp far more often than
//! everyone else does (Flurgl and Murlocs, Patches and Pirates…).

use std::collections::HashMap;
use std::path::PathBuf;
use std::time::Duration;

use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};

use crate::cards::Tribe;
use crate::{net, paths};

const STATS_URL: &str = "https://static.zerotoheroes.com/api/bgs/comp-stats/last-patch/overview-from-hourly.gz.json";
const STRATEGIES_URL: &str =
    "https://static.zerotoheroes.com/hearthstone/data/battlegrounds-strategies/bgs-comps-strategies.gz.json";
const MAX_AGE: Duration = Duration::from_secs(6 * 3600);
/// Fewer games than this in a comp says nothing about a hero.
const MIN_GAMES: u32 = 50;
/// How much more often than average a hero must play a comp.
const MIN_LIFT: f32 = 1.5;
const MAX_HEROES: usize = 5;
/// Card overlap a guide needs with an archetype to be called the same comp:
/// two shared core cards, or one plus some add-ons.
const MIN_MATCH: usize = 5;

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct HeroEnabler {
    pub hero: String,
    pub games: u32,
    /// Games in the comp over what the hero's overall games predict.
    pub lift: f32,
    /// The hero's average placement when it ends in the comp.
    pub placement: f32,
}

/// One of Firestone's comp archetypes, or every archetype of a tribe merged.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Archetype {
    /// Firestone's id, e.g. `murloc_handbuff`; `<tribe>_tribe` when merged.
    pub id: String,
    pub tribe_wide: bool,
    pub core: Vec<String>,
    /// Core, add-on and other cards of the comp.
    pub cards: Vec<String>,
    pub heroes: Vec<HeroEnabler>,
}

impl Archetype {
    pub fn tribe(&self) -> Option<Tribe> {
        tribe_of(&self.id)
    }
}

/// From an archetype id's prefix (Firestone also spells it "abberation").
fn tribe_of(id: &str) -> Option<Tribe> {
    Some(match id.split('_').next()? {
        "beast" => Tribe::Beast,
        "demon" => Tribe::Demon,
        "dragon" => Tribe::Dragon,
        "elemental" => Tribe::Elemental,
        "mech" => Tribe::Mech,
        "murloc" => Tribe::Murloc,
        "naga" => Tribe::Naga,
        "pirate" => Tribe::Pirate,
        "quilboar" => Tribe::Quilboar,
        "undead" => Tribe::Undead,
        "aberration" | "abberation" => Tribe::Aberration,
            _ => return None,
    })
}

/// The archetype whose cards best match a comp guide's, else every comp of
/// the guide's tribe merged. Card ids, not dbf ids.
pub fn for_comp<'a>(archetypes: &'a [Archetype], tribe: Option<Tribe>, core: &[&str], addons: &[&str]) -> Option<&'a Archetype> {
    let same_tribe = || archetypes.iter().filter(move |a| a.tribe() == tribe);
    let score = |a: &Archetype| {
        let shared = |ids: &[&str], of: &[String]| ids.iter().filter(|id| of.iter().any(|c| c == *id)).count();
        2 * shared(core, &a.core) + shared(core, &a.cards) + shared(addons, &a.cards)
    };
    let best = same_tribe().filter(|a| !a.tribe_wide).map(|a| (score(a), a)).filter(|(s, _)| *s >= MIN_MATCH).max_by_key(|(s, _)| *s);
    best.map(|(_, a)| a).or_else(|| tribe.and_then(|_| same_tribe().find(|a| a.tribe_wide)))
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct RawStats {
    comp_stats: Vec<RawComp>,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct RawComp {
    archetype: String,
    #[serde(default)]
    hero_stats: Vec<RawHero>,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct RawHero {
    hero_card_id: String,
    data_points: u32,
    average_placement: f32,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct RawStrategy {
    comp_id: String,
    #[serde(default)]
    cards: Vec<RawStrategyCard>,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct RawStrategyCard {
    card_id: String,
    #[serde(default)]
    status: String,
}

/// Games of one hero in one comp.
struct Cell {
    games: u32,
    placement_sum: f32,
}

fn tally<'a>(comps: &[&'a RawComp]) -> (HashMap<&'a str, Cell>, u32) {
    let mut cells: HashMap<&str, Cell> = HashMap::new();
    for h in comps.iter().flat_map(|c| &c.hero_stats) {
        let cell = cells.entry(h.hero_card_id.as_str()).or_insert(Cell { games: 0, placement_sum: 0.0 });
        cell.games += h.data_points;
        cell.placement_sum += h.average_placement * h.data_points as f32;
    }
    let games = cells.values().map(|c| c.games).sum();
    (cells, games)
}

/// Lift is measured within Firestone's per-hero sample (it is capped per hero
/// and comp), against how often each hero and comp show up in that sample.
fn rank_heroes(cells: &HashMap<&str, Cell>, hero_games: &HashMap<&str, u32>, comp_games: u32, total: u32) -> Vec<HeroEnabler> {
    let mut heroes: Vec<HeroEnabler> = cells
        .iter()
        .filter(|(_, c)| c.games >= MIN_GAMES)
        .map(|(hero, c)| {
            let expected = hero_games[hero] as f32 * comp_games as f32 / total as f32;
            HeroEnabler {
                hero: hero.to_string(),
                games: c.games,
                lift: c.games as f32 / expected,
                placement: c.placement_sum / c.games as f32,
            }
        })
        .filter(|h| h.lift >= MIN_LIFT)
        .collect();
    heroes.sort_by(|a, b| b.lift.total_cmp(&a.lift).then_with(|| a.hero.cmp(&b.hero)));
    heroes.truncate(MAX_HEROES);
    heroes
}

pub fn summarize(stats: &[u8], strategies: &[u8]) -> Result<Vec<Archetype>> {
    let stats: RawStats = serde_json::from_slice(stats).context("parsing comp stats")?;
    let strategies: Vec<RawStrategy> = serde_json::from_slice(strategies).context("parsing comp strategies")?;
    let cards: HashMap<&str, &RawStrategy> = strategies.iter().map(|s| (s.comp_id.trim(), s)).collect();

    let mut hero_games: HashMap<&str, u32> = HashMap::new();
    let mut total = 0;
    for h in stats.comp_stats.iter().flat_map(|c| &c.hero_stats) {
        *hero_games.entry(&h.hero_card_id).or_default() += h.data_points;
        total += h.data_points;
    }
    if total == 0 {
        return Ok(Vec::new());
    }
    let mut out = Vec::new();
    for comp in &stats.comp_stats {
        let id = comp.archetype.trim();
        if id.is_empty() {
            continue;
        }
        let (cells, games) = tally(&[comp]);
        let strategy = cards.get(id);
        let ids = |core_only: bool| -> Vec<String> {
            strategy
                .map(|s| s.cards.iter().filter(|c| !core_only || c.status == "CORE").map(|c| c.card_id.clone()).collect())
                .unwrap_or_default()
        };
        out.push(Archetype {
            id: id.to_string(),
            tribe_wide: false,
            core: ids(true),
            cards: ids(false),
            heroes: rank_heroes(&cells, &hero_games, games, total),
        });
    }
    // Every comp of a tribe together, for guides no archetype matches.
    for tribe in Tribe::ALL {
        let of_tribe: Vec<&RawComp> = stats.comp_stats.iter().filter(|c| tribe_of(&c.archetype) == Some(tribe)).collect();
        if of_tribe.is_empty() {
            continue;
        }
        let (cells, games) = tally(&of_tribe);
        out.push(Archetype {
            id: format!("{}_tribe", tribe.label().to_lowercase()),
            tribe_wide: true,
            core: Vec::new(),
            cards: Vec::new(),
            heroes: rank_heroes(&cells, &hero_games, games, total),
        });
    }
    Ok(out)
}

fn cache_path() -> PathBuf {
    paths::cache_dir().join("hero_enablers.json")
}

fn fetch() -> Result<Vec<Archetype>> {
    log::info!("downloading comp stats for hero enablers");
    let stats = net::get_bytes(STATS_URL)?;
    let strategies = net::get_bytes(STRATEGIES_URL)?;
    summarize(&stats, &strategies)
}

/// Enablers from the cache, computed again from fresh stats when missing or
/// stale. The stats are large (~80 MB of JSON), so only the summary is kept.
pub fn load_or_fetch() -> Result<Vec<Archetype>> {
    let path = cache_path();
    let cached = || -> Result<Vec<Archetype>> {
        let bytes = std::fs::read(&path).with_context(|| format!("reading {}", path.display()))?;
        serde_json::from_slice(&bytes).context("parsing the hero enabler cache")
    };
    let age = std::fs::metadata(&path).and_then(|m| m.modified()).ok().and_then(|m| m.elapsed().ok());
    if age.is_some_and(|a| a <= MAX_AGE) {
        if let Ok(a) = cached() {
            return Ok(a);
        }
    }
    match fetch() {
        Ok(archetypes) => {
            if let Some(dir) = path.parent() {
                std::fs::create_dir_all(dir)?;
            }
            std::fs::write(&path, serde_json::to_vec(&archetypes)?)?;
            Ok(archetypes)
        }
        Err(e) if age.is_some() => {
            log::warn!("{e:#}");
            cached()
        }
        Err(e) => Err(e.context("could not download the comp stats")),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn hero(id: &str, n: u32, place: f32) -> String {
        format!(r#"{{"heroCardId":"{id}","dataPoints":{n},"averagePlacement":{place},"finalBoards":[]}}"#)
    }

    #[test]
    fn ranks_heroes_that_overplay_a_comp() {
        // Flurgl plays Murlocs in most of its games; everyone else rarely.
        let stats = format!(
            r#"{{"lastUpdateDate":"x","compStats":[
                {{"archetype":"murloc_handbuff","dataPoints":9,"heroStats":[{},{}]}},
                {{"archetype":"beast_beetle","heroStats":[{},{}]}},
                {{"archetype":"neutral_tea_set","heroStats":[{}]}}
            ]}}"#,
            hero("FLURGL", 200, 3.0),
            hero("OTHER", 60, 4.0),
            hero("FLURGL", 50, 4.0),
            hero("OTHER", 600, 4.0),
            hero("OTHER", 300, 4.0),
        );
        let strategies = r#"[{"compId":" ","cards":[]},{"compId":"murloc_handbuff","cards":[
            {"cardId":"M1","status":"CORE"},{"cardId":"M2","status":"CORE"},{"cardId":"M3","status":"ADDON"}]}]"#;
        let a = summarize(stats.as_bytes(), strategies.as_bytes()).unwrap();
        let murloc = a.iter().find(|a| a.id == "murloc_handbuff").unwrap();
        assert_eq!(murloc.core, ["M1", "M2"]);
        assert_eq!(murloc.cards, ["M1", "M2", "M3"]);
        assert_eq!(murloc.heroes.iter().map(|h| h.hero.as_str()).collect::<Vec<_>>(), ["FLURGL"]);
        assert_eq!(murloc.heroes[0].placement, 3.0);
        assert!(a.iter().find(|a| a.id == "beast_beetle").unwrap().heroes.is_empty());
        let tribe = a.iter().find(|a| a.id == "murloc_tribe").unwrap();
        assert!(tribe.tribe_wide && tribe.heroes[0].hero == "FLURGL");
        assert!(!a.iter().any(|a| a.id == "neutral_tribe"));

        // A guide sharing two core cards is that comp; one sharing none falls
        // back to the tribe; a Mixed guide has no fallback.
        assert_eq!(for_comp(&a, Some(Tribe::Murloc), &["M1", "M2"], &[]).unwrap().id, "murloc_handbuff");
        assert_eq!(for_comp(&a, Some(Tribe::Murloc), &["X"], &["M3"]).unwrap().id, "murloc_tribe");
        assert_eq!(for_comp(&a, Some(Tribe::Beast), &["M1", "M2"], &[]).unwrap().id, "beast_tribe");
        assert!(for_comp(&a, None, &["M1"], &[]).is_none());
    }
}
