//! Card database: Firestone's card export, which carries the Battlegrounds
//! fields (tier, pool membership, tribes). The simulator process reads the
//! same file, so both sides always agree on card data.

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::time::{Duration, SystemTime};

use anyhow::{Context, Result};
use serde::Deserialize;

use crate::{net, paths};

const CARDS_URLS: [&str; 2] = [
    "https://static.zerotoheroes.com/data/cards/cards_enUS.gz.json",
    "https://static.firestoneapp.com/data/cards/cards_enUS.gz.json",
];
const MAX_AGE: Duration = Duration::from_secs(24 * 3600);

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum Tribe {
    Beast,
    Demon,
    Dragon,
    Elemental,
    Mech,
    Murloc,
    Naga,
    Pirate,
    Quilboar,
    Undead,
    Aberration,
}

impl Tribe {
    pub const ALL: [Tribe; 11] = [
        Tribe::Beast,
        Tribe::Demon,
        Tribe::Dragon,
        Tribe::Elemental,
        Tribe::Mech,
        Tribe::Murloc,
        Tribe::Naga,
        Tribe::Pirate,
        Tribe::Quilboar,
        Tribe::Undead,
        Tribe::Aberration,
    ];

    /// Tribes per solo lobby.
    pub const PER_LOBBY: usize = 5;

    pub fn from_db(s: &str) -> Option<Tribe> {
        Some(match s {
            "BEAST" => Tribe::Beast,
            "DEMON" => Tribe::Demon,
            "DRAGON" => Tribe::Dragon,
            "ELEMENTAL" => Tribe::Elemental,
            "MECH" => Tribe::Mech,
            "MURLOC" => Tribe::Murloc,
            "NAGA" => Tribe::Naga,
            "PIRATE" => Tribe::Pirate,
            "QUILBOAR" => Tribe::Quilboar,
            "UNDEAD" => Tribe::Undead,
            "ABERRATION" => Tribe::Aberration,
            _ => return None,
        })
    }

    /// Name of the matching `Race` enum member in the simulator.
    pub fn db_name(self) -> &'static str {
        match self {
            Tribe::Beast => "BEAST",
            Tribe::Demon => "DEMON",
            Tribe::Dragon => "DRAGON",
            Tribe::Elemental => "ELEMENTAL",
            Tribe::Mech => "MECH",
            Tribe::Murloc => "MURLOC",
            Tribe::Naga => "NAGA",
            Tribe::Pirate => "PIRATE",
            Tribe::Quilboar => "QUILBOAR",
            Tribe::Undead => "UNDEAD",
            Tribe::Aberration => "ABERRATION",
        }
    }

    pub fn label(self) -> &'static str {
        match self {
            Tribe::Beast => "Beast",
            Tribe::Demon => "Demon",
            Tribe::Dragon => "Dragon",
            Tribe::Elemental => "Elemental",
            Tribe::Mech => "Mech",
            Tribe::Murloc => "Murloc",
            Tribe::Naga => "Naga",
            Tribe::Pirate => "Pirate",
            Tribe::Quilboar => "Quilboar",
            Tribe::Undead => "Undead",
            Tribe::Aberration => "Aberration",
        }
    }
}

/// A keyword the tier browser can filter by. A card matches when it has the
/// keyword or works with it (its text references it, e.g. "give Reborn").
pub struct Keyword {
    pub label: &'static str,
    /// Card `mechanics` / `referencedTags` values.
    tags: &'static [&'static str],
    /// Lower-case text fallback for keywords without a tag.
    text: Option<&'static str>,
}

pub const KEYWORDS: &[Keyword] = &[
    Keyword { label: "Battlecry", tags: &["BATTLECRY"], text: None },
    Keyword { label: "Deathrattle", tags: &["DEATHRATTLE"], text: None },
    Keyword { label: "Divine Shield", tags: &["DIVINE_SHIELD", "GIVES_DIVINE_SHIELD"], text: None },
    Keyword { label: "Taunt", tags: &["TAUNT"], text: None },
    Keyword { label: "Reborn", tags: &["REBORN", "GIVES_REBORN", "BACON_REBORN_TOOLTIP"], text: None },
    Keyword { label: "Venomous", tags: &["VENOMOUS"], text: None },
    Keyword { label: "Windfury", tags: &["WINDFURY"], text: None },
    Keyword { label: "Avenge", tags: &["AVENGE"], text: None },
    Keyword { label: "Rally", tags: &["BACON_RALLY"], text: None },
    Keyword { label: "Start of Combat", tags: &[], text: Some("start of combat") },
    Keyword { label: "End of Turn", tags: &["END_OF_TURN", "END_OF_TURN_TRIGGER"], text: None },
    Keyword { label: "Magnetic", tags: &["MODULAR"], text: None },
    Keyword { label: "Spellcraft", tags: &["BACON_SPELLCRAFT_ID"], text: None },
    Keyword { label: "Choose One", tags: &["CHOOSE_ONE"], text: None },
    Keyword { label: "Discover", tags: &["DISCOVER"], text: None },
    Keyword { label: "Blood Gem", tags: &["BACON_BLOOD_GEM_TOOLTIP"], text: Some("blood gem") },
];

/// What the card search can be narrowed to.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum Category {
    Minion,
    Spell,
    Hero,
    Trinket,
    Anomaly,
    Reward,
}

impl Category {
    pub const ALL: [Category; 6] =
        [Category::Minion, Category::Spell, Category::Hero, Category::Trinket, Category::Anomaly, Category::Reward];

    pub fn label(self) -> &'static str {
        match self {
            Category::Minion => "Minion",
            Category::Spell => "Spell",
            Category::Hero => "Hero",
            Category::Trinket => "Trinket",
            Category::Anomaly => "Anomaly",
            Category::Reward => "Reward",
        }
    }
    /// Lower-case count noun: "1 hero", "3 anomalies".
    pub fn count(self, n: usize) -> String {
        let noun = match (self, n == 1) {
            (Category::Hero, false) => "heroes",
            (Category::Anomaly, false) => "anomalies",
            (k, true) => return format!("{n} {}", k.label().to_lowercase()),
            (k, false) => return format!("{n} {}s", k.label().to_lowercase()),
        };
        format!("{n} {noun}")
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Kind {
    Minion,
    TavernSpell,
    Other,
}

#[derive(Debug, Clone)]
pub struct Card {
    pub id: String,
    pub name: String,
    pub kind: Kind,
    pub tribes: Vec<Tribe>,
    /// "All tribes" (amalgam-style) minion.
    pub all_tribes: bool,
    pub tier: i32,
    pub cost: i32,
    pub pool: bool,
    pub timewarped: bool,
    pub solo_only: bool,
    pub duos_only: bool,
    /// Bit `i` set when the card matches `KEYWORDS[i]`.
    pub keywords: u32,
    /// Battlegrounds card the search can find.
    pub category: Option<Category>,
}

impl Card {
    pub fn has_keyword(&self, index: usize) -> bool {
        self.keywords & (1 << index) != 0
    }
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct RawCard {
    id: String,
    dbf_id: Option<i32>,
    #[serde(default)]
    name: String,
    #[serde(default, rename = "type")]
    kind: String,
    #[serde(default)]
    races: Vec<String>,
    tech_level: Option<i32>,
    #[serde(default)]
    is_bacon_pool: bool,
    cost: Option<i32>,
    #[serde(default)]
    mechanics: Vec<String>,
    #[serde(default)]
    referenced_tags: Vec<String>,
    #[serde(default)]
    text: String,
    #[serde(default)]
    battlegrounds_hero: bool,
}

/// Search category of a raw card: tavern cards, playable heroes, and the
/// pieces of the current season's modes.
fn category(r: &RawCard) -> Option<Category> {
    let tavern = r.is_bacon_pool && r.tech_level.is_some_and(|t| (1..=7).contains(&t));
    Some(match r.kind.as_str() {
        "Minion" if tavern => Category::Minion,
        "Battleground_spell" if tavern => Category::Spell,
        "Hero" if r.battlegrounds_hero => Category::Hero,
        "Battleground_trinket" if r.is_bacon_pool => Category::Trinket,
        "Battleground_anomaly" => Category::Anomaly,
        "Battleground_quest_reward" => Category::Reward,
        _ => return None,
    })
}

fn keyword_bits(r: &RawCard) -> u32 {
    let text = r.text.to_lowercase();
    let mut bits = 0;
    for (i, k) in KEYWORDS.iter().enumerate() {
        let tagged = r.mechanics.iter().chain(&r.referenced_tags).any(|m| k.tags.contains(&m.as_str()));
        if tagged || k.text.is_some_and(|t| text.contains(t)) {
            bits |= 1 << i;
        }
    }
    bits
}

pub struct CardDb {
    cards: HashMap<String, Card>,
    /// dbf id → card id (HSReplay refers to cards by dbf id).
    dbf: HashMap<i32, String>,
    /// Ids of tavern minions and spells (any mode), for the tier browser.
    pool: Vec<String>,
    /// Ids the card search looks through, one per category and name.
    searchable: Vec<String>,
    path: PathBuf,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum GroupKind {
    Tribe(Tribe),
    Neutral,
    AllTribes,
}

/// One group in the tier browser.
pub struct TierGroup<'a> {
    pub kind: GroupKind,
    pub cards: Vec<&'a Card>,
}

impl CardDb {
    pub fn cards_path() -> PathBuf {
        paths::cache_dir().join("cards_enUS.json")
    }

    /// Loads the cached export, downloading it when missing or stale. A stale
    /// copy is still used when the download fails.
    pub fn load_or_fetch() -> Result<CardDb> {
        let path = Self::cards_path();
        let age = std::fs::metadata(&path)
            .and_then(|m| m.modified())
            .ok()
            .and_then(|t| SystemTime::now().duration_since(t).ok());
        if age.is_none_or(|a| a > MAX_AGE) {
            log::info!("downloading card database");
            let mut ok = false;
            for url in CARDS_URLS {
                match net::download(url, &path) {
                    Ok(()) => {
                        ok = true;
                        break;
                    }
                    Err(e) => log::warn!("{e:#}"),
                }
            }
            if !ok && age.is_none() {
                anyhow::bail!("could not download the card database");
            }
        }
        Self::load(&path)
    }

    pub fn load(path: &Path) -> Result<CardDb> {
        let bytes = std::fs::read(path).with_context(|| format!("reading {}", path.display()))?;
        Self::parse(&bytes, path)
    }

    pub fn parse(bytes: &[u8], path: &Path) -> Result<CardDb> {
        let raw: Vec<RawCard> = serde_json::from_slice(bytes).context("parsing card database")?;
        let dbf: HashMap<i32, String> = raw.iter().filter_map(|r| Some((r.dbf_id?, r.id.clone()))).collect();
        let cards: HashMap<String, Card> = raw
            .into_iter()
            .map(|r| {
                let has = |m: &str| r.mechanics.iter().any(|x| x == m);
                let card = Card {
                    kind: match r.kind.as_str() {
                        "Minion" => Kind::Minion,
                        "Battleground_spell" => Kind::TavernSpell,
                        _ => Kind::Other,
                    },
                    tribes: r.races.iter().filter_map(|s| Tribe::from_db(s)).collect(),
                    all_tribes: r.races.iter().any(|s| s == "ALL"),
                    tier: r.tech_level.unwrap_or(0),
                    cost: r.cost.unwrap_or(0),
                    pool: r.is_bacon_pool,
                    timewarped: has("BACON_TIMEWARPED"),
                    keywords: keyword_bits(&r),
                    category: category(&r),
                    solo_only: has("BG_SOLO_EXCLUSIVE"),
                    duos_only: has("BG_DUO_EXCLUSIVE"),
                    name: r.name,
                    id: r.id.clone(),
                };
                (r.id, card)
            })
            .collect();
        let cards: HashMap<String, Card> = cards;
        let mut pool: Vec<String> = cards
            .values()
            .filter(|c| c.pool && !c.timewarped && (1..=7).contains(&c.tier) && c.kind != Kind::Other)
            .map(|c| c.id.clone())
            .collect();
        pool.sort();
        // Anomalies and rewards come in look-alike copies; keep the first id
        // of each name.
        let mut searchable: Vec<&Card> = cards.values().filter(|c| c.category.is_some() && !c.name.is_empty()).collect();
        searchable.sort_by(|a, b| (a.category, &a.name, &a.id).cmp(&(b.category, &b.name, &b.id)));
        searchable.dedup_by(|a, b| a.category == b.category && a.name == b.name);
        let searchable = searchable.into_iter().map(|c| c.id.clone()).collect();
        Ok(CardDb { cards, dbf, pool, searchable, path: path.to_path_buf() })
    }

    pub fn path(&self) -> &Path {
        &self.path
    }

    pub fn get(&self, id: &str) -> Option<&Card> {
        self.cards.get(id)
    }

    pub fn by_dbf(&self, dbf: i32) -> Option<&Card> {
        self.dbf.get(&dbf).and_then(|id| self.cards.get(id))
    }

    pub fn name<'a>(&'a self, id: &'a str) -> &'a str {
        self.get(id).map(|c| c.name.as_str()).filter(|n| !n.is_empty()).unwrap_or(id)
    }

    /// Pool cards of `kind` and `tier` for the current mode.
    fn pool_cards(&self, kind: Kind, tier: i32, duos: bool) -> impl Iterator<Item = &Card> {
        self.pool
            .iter()
            .filter_map(|id| self.cards.get(id))
            .filter(move |c| c.kind == kind && c.tier == tier && !(duos && c.solo_only) && !(!duos && c.duos_only))
    }

    /// Lobby tribes confirmed by single-tribe pool minions Bob has offered.
    /// Dual-tribe minions are in the pool when either tribe is, so they prove
    /// nothing on their own.
    pub fn infer_tribes(&self, shop_cards: &[String]) -> Vec<Tribe> {
        let mut found: Vec<Tribe> = shop_cards
            .iter()
            .filter_map(|id| self.get(id))
            .filter(|c| c.kind == Kind::Minion && c.pool && !c.all_tribes && c.tribes.len() == 1)
            .map(|c| c.tribes[0])
            .collect();
        found.sort();
        found.dedup();
        found
    }

    /// Minions of `tier` grouped by tribe (neutrals last, then all-tribe
    /// minions). Only `tribes` are listed; dual-tribe minions appear under
    /// each of their listed tribes.
    pub fn tier_minions(&self, tier: i32, tribes: &[Tribe], duos: bool) -> Vec<TierGroup<'_>> {
        let mut minions: Vec<&Card> = self.pool_cards(Kind::Minion, tier, duos).collect();
        minions.sort_by(|a, b| a.name.cmp(&b.name));
        let mut groups: Vec<TierGroup> = tribes
            .iter()
            .map(|&t| TierGroup {
                kind: GroupKind::Tribe(t),
                cards: minions.iter().copied().filter(|c| c.tribes.contains(&t)).collect(),
            })
            .filter(|g| !g.cards.is_empty())
            .collect();
        let neutral: Vec<&Card> =
            minions.iter().copied().filter(|c| c.tribes.is_empty() && !c.all_tribes).collect();
        let all: Vec<&Card> = minions.iter().copied().filter(|c| c.all_tribes).collect();
        if !neutral.is_empty() {
            groups.push(TierGroup { kind: GroupKind::Neutral, cards: neutral });
        }
        if !all.is_empty() {
            groups.push(TierGroup { kind: GroupKind::AllTribes, cards: all });
        }
        groups
    }

    /// Cards matching a typed name, best first. An empty query lists the
    /// whole category by tier.
    pub fn search(&self, query: &str, category: Option<Category>) -> Vec<&Card> {
        let query = crate::search::Query::new(query);
        let mut hits: Vec<(i32, &Card)> = self
            .searchable
            .iter()
            .filter_map(|id| self.cards.get(id))
            .filter(|c| category.is_none_or(|k| c.category == Some(k)))
            .filter_map(|c| Some((query.score(&c.name)?, c)))
            .collect();
        hits.sort_by(|(sa, a), (sb, b)| {
            sb.cmp(sa).then_with(|| (a.category, a.tier, &a.name).cmp(&(b.category, b.tier, &b.name)))
        });
        hits.into_iter().map(|(_, c)| c).collect()
    }

    pub fn tier_spells(&self, tier: i32, duos: bool) -> Vec<&Card> {
        let mut spells: Vec<&Card> = self.pool_cards(Kind::TavernSpell, tier, duos).collect();
        spells.sort_by(|a, b| a.cost.cmp(&b.cost).then_with(|| a.name.cmp(&b.name)));
        spells
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn db() -> CardDb {
        let json = r#"[
            {"id":"B1","name":"Bat","type":"Minion","races":["BEAST"],"techLevel":1,"isBaconPool":true},
            {"id":"B2","name":"Ape","type":"Minion","races":["BEAST","PIRATE"],"techLevel":1,"isBaconPool":true},
            {"id":"D1","name":"Whelp","type":"Minion","races":["DRAGON"],"techLevel":1,"isBaconPool":true},
            {"id":"N1","name":"Guard","type":"Minion","techLevel":1,"isBaconPool":true},
            {"id":"A1","name":"Amalgam","type":"Minion","races":["ALL"],"techLevel":1,"isBaconPool":true},
            {"id":"T1","name":"Timewarped Bat","type":"Minion","races":["BEAST"],"techLevel":1,"isBaconPool":true,"mechanics":["BACON_TIMEWARPED"]},
            {"id":"P1","name":"Passenger","type":"Minion","techLevel":1,"isBaconPool":true,"mechanics":["BG_DUO_EXCLUSIVE"]},
            {"id":"S1","name":"Coin","type":"Battleground_spell","techLevel":1,"isBaconPool":true,"cost":1},
            {"id":"X1","name":"Token","type":"Minion","races":["MECH"],"techLevel":1}
        ]"#;
        CardDb::parse(json.as_bytes(), Path::new("test")).unwrap()
    }

    #[test]
    fn keywords_from_mechanics_references_and_text() {
        let json = r#"[
            {"id":"K1","name":"A","type":"Minion","mechanics":["DEATHRATTLE"]},
            {"id":"K2","name":"B","type":"Minion","referencedTags":["DEATHRATTLE"]},
            {"id":"K3","name":"C","type":"Minion","text":"<b>Start of Combat:</b> Deal 3 damage."},
            {"id":"K4","name":"D","type":"Minion","mechanics":["BATTLECRY"]}
        ]"#;
        let db = CardDb::parse(json.as_bytes(), Path::new("test")).unwrap();
        let idx = |label: &str| KEYWORDS.iter().position(|k| k.label == label).unwrap();
        let (dr, soc) = (idx("Deathrattle"), idx("Start of Combat"));
        assert!(db.get("K1").unwrap().has_keyword(dr));
        assert!(db.get("K2").unwrap().has_keyword(dr));
        assert!(db.get("K3").unwrap().has_keyword(soc));
        assert!(!db.get("K4").unwrap().has_keyword(dr));
    }

    #[test]
    fn search_by_category_and_name() {
        let json = r#"[
            {"id":"B1","name":"Bat","type":"Minion","races":["BEAST"],"techLevel":1,"isBaconPool":true},
            {"id":"X1","name":"Battle Token","type":"Minion","techLevel":1},
            {"id":"H1","name":"Batlord","type":"Hero","battlegroundsHero":true},
            {"id":"H2","name":"Batlord Skin","type":"Hero"},
            {"id":"R1","name":"Bat Cave","type":"Battleground_quest_reward"},
            {"id":"R2","name":"Bat Cave","type":"Battleground_quest_reward"},
            {"id":"T1","name":"Bat Charm","type":"Battleground_trinket","isBaconPool":true}
        ]"#;
        let db = CardDb::parse(json.as_bytes(), Path::new("test")).unwrap();
        let ids = |q: &str, k: Option<Category>| db.search(q, k).iter().map(|c| c.id.as_str()).collect::<Vec<_>>();
        assert_eq!(ids("bat", None), ["B1", "H1", "T1", "R1"]);
        assert_eq!(ids("btlord", Some(Category::Hero)), ["H1"]);
        assert_eq!(ids("", Some(Category::Reward)), ["R1"]);
    }

    #[test]
    fn infers_only_from_single_tribe_pool_minions() {
        let db = db();
        let seen = ["B2", "X1", "A1", "D1", "D1"].map(String::from);
        assert_eq!(db.infer_tribes(&seen), vec![Tribe::Dragon]);
    }

    #[test]
    fn tier_groups_filter_pool() {
        let db = db();
        let groups = db.tier_minions(1, &[Tribe::Beast, Tribe::Pirate], false);
        let names: Vec<Vec<&str>> = groups.iter().map(|g| g.cards.iter().map(|c| c.name.as_str()).collect()).collect();
        assert_eq!(names, vec![vec!["Ape", "Bat"], vec!["Ape"], vec!["Guard"], vec!["Amalgam"]]);
        assert_eq!(db.tier_spells(1, false).len(), 1);
        assert!(db.tier_minions(1, &[], true).iter().any(|g| g.cards.iter().any(|c| c.id == "P1")));
    }
}
