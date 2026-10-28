//! Battlegrounds game state rebuilt from the `PowerTaskList` stream.
//!
//! Battlegrounds-specific facts this relies on (verified against real logs):
//! - A solo lobby has exactly two `Player` entities: the local player and a
//!   dummy (`BACON_DUMMY_PLAYER=1`). The dummy plays Bob in the shop and is
//!   renamed to each combat opponent, so any unknown player name in the log
//!   refers to it.
//! - `GameEntity` `TURN` alternates shop (odd) and combat (even) phases.
//! - At combat start the dummy's `BACON_CURRENT_COMBAT_PLAYER_ID` is set to
//!   the opponent's lobby id, and the opponent's hero and board are created
//!   inside that same block. When that block closes both boards are complete
//!   and no start-of-combat effect has resolved yet: that is the snapshot the
//!   simulator needs.
//! - Leaderboard heroes carry the lobby id in `PLAYER_ID`; the per-combat
//!   copies of them are marked `BACON_COMBAT_PHASE_HERO` and removed after.

use std::collections::{BTreeMap, HashMap, HashSet};

use serde::Serialize;

use super::tags::{self, card_type, zone};
use crate::cards::Tribe;
use crate::logfile::power::{parse_line, EntityRef, PowerLine};

#[derive(Debug, Clone, Default)]
pub struct Entity {
    pub id: i32,
    pub card_id: String,
    pub tags: HashMap<String, i32>,
}

impl Entity {
    pub fn tag(&self, name: &str) -> i32 {
        self.tags.get(name).copied().unwrap_or(0)
    }

    pub fn tag_or(&self, name: &str, alt_numeric: i32) -> i32 {
        if let Some(&v) = self.tags.get(name) {
            return v;
        }
        let alt = alt_numeric.to_string();
        self.tags.get(&alt).copied().unwrap_or(0)
    }
}

/// An active Battlegrounds counter/spell (e.g. Blood Gems +1/+1, Free Rolls 25).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CounterItem {
    pub id: String,
    pub card_id: String,
    pub label: String,
    pub value: String,
    pub sort_order: i32,
}

/// An entity as shipped to the simulator: card id plus every numeric tag,
/// keyed by the name (or number) the log printed.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct RawEntity {
    pub id: i32,
    pub card_id: String,
    pub tags: BTreeMap<String, i32>,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub enchantments: Vec<RawEntity>,
}

impl RawEntity {
    pub fn tag(&self, name: &str) -> i32 {
        self.tags.get(name).copied().unwrap_or(0)
    }
}

/// One player's side of a combat.
#[derive(Debug, Clone, Default, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Side {
    pub lobby_id: i32,
    pub player_tags: BTreeMap<String, i32>,
    pub hero: Option<RawEntity>,
    pub hero_powers: Vec<RawEntity>,
    pub board: Vec<RawEntity>,
    pub hand: Vec<RawEntity>,
    pub trinkets: Vec<RawEntity>,
    pub quest_rewards: Vec<RawEntity>,
    pub secrets: Vec<RawEntity>,
    /// Enchantments attached to the player or hero entity.
    pub enchantments: Vec<RawEntity>,
}

impl Side {
    pub fn tavern_tier(&self) -> i32 {
        self.player_tags.get(tags::PLAYER_TECH_LEVEL).copied().unwrap_or(0)
    }
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CombatSnapshot {
    pub turn: i32,
    pub players_alive: i32,
    pub anomalies: Vec<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub damage_cap: Option<i32>,
    pub player: Side,
    pub opponent: Side,
}

/// An opponent's board as of one combat against them.
#[derive(Debug, Clone)]
pub struct SeenBoard {
    pub turn: i32,
    pub side: Side,
}

#[derive(Debug, Clone)]
pub struct GameResult {
    pub game_id: String,
    pub hero_card_id: String,
    pub place: i32,
    pub turn: i32,
}

#[derive(Debug)]
pub enum GameEvent {
    GameStarted,
    Combat(Box<CombatSnapshot>),
    GameEnded(GameResult),
}

#[derive(Debug, Clone)]
pub struct LeaderboardEntry {
    pub lobby_id: i32,
    pub place: i32,
    pub hero_card_id: String,
    pub health: i32,
    pub armor: i32,
    pub is_local: bool,
}

#[derive(Debug, Clone, Default)]
struct GameInfo {
    game_type: String,
    player_names: HashMap<i32, String>,
}

#[derive(Default)]
pub struct Game {
    entities: HashMap<i32, Entity>,
    game_entity: i32,
    /// `PlayerID` (the controller id) → player entity id.
    players: HashMap<i32, i32>,
    names: HashMap<String, i32>,
    /// `DebugPrintGame` lines precede the `CREATE_GAME` they describe.
    next_info: GameInfo,
    info: GameInfo,
    source: String,
    game_id: String,
    /// Entity receiving indented `tag=` lines.
    current: Option<i32>,
    /// Entity whose `FULL_ENTITY` declaration is still being read.
    created: Option<i32>,
    depth: usize,
    /// Block depth at which the combat setup started.
    pending_combat: Option<usize>,
    shop_seen: HashSet<i32>,
    shop_cards: Vec<String>,
    /// Every board seen per lobby id, oldest first.
    boards: HashMap<i32, Vec<SeenBoard>>,
    active: bool,
    over: bool,
    events: Vec<GameEvent>,
}

impl Game {
    pub fn new() -> Self {
        Self::default()
    }

    /// Names the log session, used to build stable game ids.
    pub fn set_source(&mut self, source: &str) {
        self.source = source.to_string();
    }

    /// Discards everything (a different log file is being followed).
    pub fn reset(&mut self) {
        let source = std::mem::take(&mut self.source);
        *self = Self { source, ..Self::default() };
    }

    pub fn take_events(&mut self) -> Vec<GameEvent> {
        std::mem::take(&mut self.events)
    }

    pub fn process(&mut self, line: &str) {
        let Some(pl) = parse_line(line) else { return };
        if !matches!(pl, PowerLine::Tag { .. }) {
            self.finalize_created();
        }
        match pl {
            PowerLine::GameType(t) => self.next_info.game_type = t,
            PowerLine::PlayerName { player_id, name } => {
                self.next_info.player_names.insert(player_id, name);
            }
            PowerLine::CreateGame => {
                let ts = line.split_whitespace().nth(1).unwrap_or("");
                self.start_game(ts);
            }
            PowerLine::GameEntity { id } => {
                self.game_entity = id;
                self.ensure(id);
                self.current = Some(id);
            }
            PowerLine::PlayerEntity { id, player_id } => {
                self.ensure(id);
                self.players.insert(player_id, id);
                if let Some(name) = self.info.player_names.get(&player_id) {
                    self.names.insert(name.clone(), id);
                }
                self.current = Some(id);
            }
            PowerLine::FullEntity { id, card_id } => {
                let e = self.ensure(id);
                if !card_id.is_empty() {
                    e.card_id = card_id;
                }
                self.current = Some(id);
                self.created = Some(id);
            }
            PowerLine::ShowEntity { entity, card_id } | PowerLine::ChangeEntity { entity, card_id } => {
                self.current = self.resolve(&entity);
                if let Some(id) = self.current {
                    if !card_id.is_empty() {
                        self.ensure(id).card_id = card_id;
                    }
                }
            }
            PowerLine::HideEntity { entity, tag, value } | PowerLine::TagChange { entity, tag, value } => {
                self.current = None;
                if let Some(id) = self.resolve(&entity) {
                    self.set_tag(id, &tag, &value);
                }
            }
            PowerLine::Tag { tag, value } => {
                if let Some(id) = self.current {
                    self.set_tag(id, &tag, &value);
                }
            }
            PowerLine::BlockStart { block_type, .. } => {
                self.current = None;
                if block_type == "ATTACK" && self.pending_combat.is_some() {
                    self.take_snapshot();
                }
                self.depth += 1;
            }
            PowerLine::BlockEnd => {
                self.current = None;
                self.depth = self.depth.saturating_sub(1);
                if self.pending_combat.is_some_and(|d| self.depth < d) {
                    self.take_snapshot();
                }
            }
        }
    }

    fn start_game(&mut self, ts: &str) {
        let source = std::mem::take(&mut self.source);
        let next_info = std::mem::take(&mut self.next_info);
        *self = Self {
            game_id: format!("{source}#{ts}"),
            source,
            info: next_info,
            active: true,
            events: vec![GameEvent::GameStarted],
            ..Self::default()
        };
    }

    fn ensure(&mut self, id: i32) -> &mut Entity {
        self.entities.entry(id).or_insert_with(|| Entity { id, ..Entity::default() })
    }

    fn resolve(&mut self, r: &EntityRef) -> Option<i32> {
        match r {
            EntityRef::Id(id) => Some(*id),
            EntityRef::GameEntity => Some(self.game_entity),
            EntityRef::Name(name) => {
                if let Some(&id) = self.names.get(name) {
                    return Some(id);
                }
                let id = self
                    .info
                    .player_names
                    .iter()
                    .find(|(_, n)| *n == name)
                    .and_then(|(pid, _)| self.players.get(pid).copied())
                    .or_else(|| self.dummy_player().map(|(_, e)| e))?;
                self.names.insert(name.clone(), id);
                Some(id)
            }
        }
    }

    fn set_tag(&mut self, id: i32, tag: &str, value: &str) {
        let Some(v) = tags::parse_value(tag, value) else { return };
        self.ensure(id).tags.insert(tag.to_string(), v);
        match tag {
            tags::STEP if id == self.game_entity => {
                if v == tags::step::FINAL_GAMEOVER {
                    self.end_game();
                } else if self.pending_combat == Some(0) {
                    self.take_snapshot();
                }
            }
            tags::BACON_CURRENT_COMBAT_PLAYER_ID if v > 0 => {
                if self.dummy_player().is_some_and(|(_, e)| e == id) {
                    self.pending_combat = Some(self.depth);
                }
            }
            tags::ZONE if v == zone::PLAY => self.check_shop(id),
            _ => {}
        }
    }

    fn finalize_created(&mut self) {
        if let Some(id) = self.created.take() {
            self.check_shop(id);
        }
    }

    /// Records minions offered in our shop (tribe inference).
    fn check_shop(&mut self, id: i32) {
        let turn = self.raw_turn();
        if turn % 2 == 0 && turn != 0 {
            return;
        }
        let Some((dummy_pid, _)) = self.dummy_player() else { return };
        let Some(e) = self.entities.get(&id) else { return };
        if e.tag(tags::CARDTYPE) == card_type::MINION
            && e.tag(tags::CONTROLLER) == dummy_pid
            && e.tag(tags::ZONE) == zone::PLAY
            && !e.card_id.is_empty()
            && self.shop_seen.insert(id)
        {
            self.shop_cards.push(e.card_id.clone());
        }
    }

    fn end_game(&mut self) {
        if self.over || !self.active {
            return;
        }
        self.over = true;
        if !self.is_battlegrounds() {
            return;
        }
        let hero = self.local_hero();
        let result = GameResult {
            game_id: self.game_id.clone(),
            hero_card_id: hero.map(|h| h.card_id.clone()).unwrap_or_default(),
            place: hero.map(|h| h.tag(tags::PLAYER_LEADERBOARD_PLACE)).unwrap_or(0),
            turn: self.turn(),
        };
        self.events.push(GameEvent::GameEnded(result));
    }

    fn take_snapshot(&mut self) {
        self.pending_combat = None;
        let (Some((local_pid, local_e)), Some((dummy_pid, dummy_e))) = (self.local_player(), self.dummy_player())
        else {
            return;
        };
        let mut player = self.side(local_pid, local_e, true);
        player.lobby_id = self.local_lobby_id();
        let mut opponent = self.side(dummy_pid, dummy_e, true);
        opponent.lobby_id = self.entities[&dummy_e].tag(tags::BACON_CURRENT_COMBAT_PLAYER_ID);
        let turn = self.turn();
        let seen = self.boards.entry(opponent.lobby_id).or_default();
        // A reconnect can replay the same combat.
        seen.retain(|b| b.turn != turn);
        seen.push(SeenBoard { turn, side: opponent.clone() });
        let anomalies = self
            .entities
            .values()
            .filter(|e| e.tag(tags::CARDTYPE) == card_type::BATTLEGROUND_ANOMALY && e.tag(tags::ZONE) == zone::PLAY)
            .map(|e| e.card_id.clone())
            .collect();
        let damage_cap = self.damage_cap();
        let snapshot = CombatSnapshot {
            turn,
            players_alive: self.leaderboard().iter().filter(|p| p.health > 0).count() as i32,
            anomalies,
            damage_cap,
            player,
            opponent,
        };
        self.events.push(GameEvent::Combat(Box::new(snapshot)));
    }

    fn raw(&self, e: &Entity, attached: &HashMap<i32, Vec<&Entity>>) -> RawEntity {
        RawEntity {
            id: e.id,
            card_id: e.card_id.clone(),
            tags: e.tags.iter().map(|(k, v)| (k.clone(), *v)).collect(),
            enchantments: attached
                .get(&e.id)
                .map(|v| v.iter().map(|en| self.raw(en, &HashMap::new())).collect())
                .unwrap_or_default(),
        }
    }

    fn side(&self, pid: i32, player_entity: i32, with_hand: bool) -> Side {
        let mut attached: HashMap<i32, Vec<&Entity>> = HashMap::new();
        for e in self.entities.values() {
            if e.tag(tags::CARDTYPE) == card_type::ENCHANTMENT && e.tag(tags::ZONE) == zone::PLAY {
                attached.entry(e.tag(tags::ATTACHED)).or_default().push(e);
            }
        }
        for list in attached.values_mut() {
            list.sort_by_key(|e| e.id);
        }
        let pe = &self.entities[&player_entity];
        let hero_id = pe.tag(tags::HERO_ENTITY);
        let mut side = Side {
            player_tags: pe.tags.iter().map(|(k, v)| (k.clone(), *v)).collect(),
            hero: self.entities.get(&hero_id).map(|h| self.raw(h, &HashMap::new())),
            ..Side::default()
        };
        for owner in [player_entity, hero_id] {
            if let Some(list) = attached.get(&owner) {
                side.enchantments.extend(list.iter().map(|e| self.raw(e, &HashMap::new())));
            }
        }
        let mut by_id: Vec<&Entity> = self.entities.values().filter(|e| e.tag(tags::CONTROLLER) == pid).collect();
        by_id.sort_by_key(|e| e.id);
        for e in by_id {
            let raw = || self.raw(e, &attached);
            match (e.tag(tags::CARDTYPE), e.tag(tags::ZONE)) {
                (card_type::MINION, zone::PLAY) => side.board.push(raw()),
                (card_type::MINION, zone::HAND) if with_hand && !e.card_id.is_empty() => side.hand.push(raw()),
                (card_type::HERO_POWER, zone::PLAY) => side.hero_powers.push(raw()),
                (card_type::BATTLEGROUND_TRINKET, zone::PLAY) => side.trinkets.push(raw()),
                (card_type::BATTLEGROUND_QUEST_REWARD, zone::PLAY) => side.quest_rewards.push(raw()),
                (_, zone::SECRET) => side.secrets.push(raw()),
                _ => {}
            }
        }
        side.board.sort_by_key(|e| e.tag(tags::ZONE_POSITION));
        side.hand.sort_by_key(|e| e.tag(tags::ZONE_POSITION));
        side
    }

    // ---- queries -------------------------------------------------------

    pub fn is_battlegrounds(&self) -> bool {
        self.info.game_type.starts_with("GT_BATTLEGROUNDS") || self.dummy_player().is_some()
    }

    pub fn is_duos(&self) -> bool {
        self.info.game_type.contains("DUO")
    }

    /// A Battlegrounds game is running and has not finished.
    pub fn in_progress(&self) -> bool {
        self.active && !self.over && self.is_battlegrounds()
    }

    fn raw_turn(&self) -> i32 {
        self.entities.get(&self.game_entity).map(|e| e.tag(tags::TURN)).unwrap_or(0)
    }

    /// Battlegrounds round: each round is a shop turn plus a combat turn.
    pub fn turn(&self) -> i32 {
        (self.raw_turn() + 1) / 2
    }

    pub fn in_combat(&self) -> bool {
        let t = self.raw_turn();
        t > 0 && t % 2 == 0
    }

    /// Current combat damage cap (e.g. 5, 10, 15), or `None` if uncapped.
    pub fn damage_cap(&self) -> Option<i32> {
        if !self.is_battlegrounds() {
            return None;
        }
        let ge = self.entities.get(&self.game_entity);
        if let Some(ge) = ge {
            let has_enabled_tag = ge.tags.contains_key(tags::BACON_COMBAT_DAMAGE_CAP_ENABLED)
                || ge.tags.contains_key("3403");
            if has_enabled_tag {
                let enabled = ge.tag_or(tags::BACON_COMBAT_DAMAGE_CAP_ENABLED, 3403);
                if enabled == 0 {
                    return None;
                }
                let cap = ge.tag_or(tags::BACON_COMBAT_DAMAGE_CAP, 2089);
                if cap > 2 {
                    return Some(cap);
                }
            }
        }
        let lb = self.leaderboard();
        let total_players = lb.len();
        let players_alive = lb.iter().filter(|p| p.health > 0).count();
        if total_players >= 8 && players_alive <= 4 {
            return None;
        }
        if self.is_duos() {
            return Some(15);
        }
        let turn = self.turn();
        if turn < 4 {
            Some(5)
        } else if turn < 8 {
            Some(10)
        } else {
            Some(15)
        }
    }

    fn dummy_player(&self) -> Option<(i32, i32)> {
        self.players
            .iter()
            .find(|(_, e)| self.entities.get(e).is_some_and(|e| e.tag(tags::BACON_DUMMY_PLAYER) == 1))
            .map(|(p, e)| (*p, *e))
    }

    fn local_player(&self) -> Option<(i32, i32)> {
        let mut candidates: Vec<(i32, i32)> = self
            .players
            .iter()
            .filter(|(_, e)| self.entities.get(e).is_some_and(|e| e.tag(tags::BACON_DUMMY_PLAYER) == 0))
            .map(|(p, e)| (*p, *e))
            .collect();
        candidates.sort();
        candidates.first().copied()
    }

    pub fn local_hero(&self) -> Option<&Entity> {
        let (_, e) = self.local_player()?;
        self.entities.get(&self.entities[&e].tag(tags::HERO_ENTITY))
    }

    pub fn local_lobby_id(&self) -> i32 {
        self.local_hero().map(|h| h.tag(tags::PLAYER_ID)).unwrap_or(0)
    }

    pub fn tavern_tier(&self) -> i32 {
        self.local_player().map(|(_, e)| self.entities[&e].tag(tags::PLAYER_TECH_LEVEL)).unwrap_or(0)
    }

    /// Highest tavern tier the tier bar offers. Tier 7 is always listed so
    /// its cards can be browsed whether or not this game unlocks it.
    pub fn max_tavern_tier(&self) -> i32 {
        7
    }

    /// Card ids of minions Bob has offered us this game.
    pub fn shop_cards(&self) -> &[String] {
        &self.shop_cards
    }

    pub fn seen_board(&self, lobby_id: i32) -> Option<&SeenBoard> {
        self.seen_boards(lobby_id).last()
    }

    /// Every board seen for a lobby id, oldest first.
    pub fn seen_boards(&self, lobby_id: i32) -> &[SeenBoard] {
        self.boards.get(&lobby_id).map_or(&[], Vec::as_slice)
    }

    /// Lobby heroes ordered by leaderboard place (1 = top).
    pub fn leaderboard(&self) -> Vec<LeaderboardEntry> {
        let mut best: HashMap<i32, &Entity> = HashMap::new();
        for e in self.entities.values() {
            let lobby_id = e.tag(tags::PLAYER_ID);
            if e.tag(tags::CARDTYPE) != card_type::HERO
                || lobby_id <= 0
                || e.tag(tags::PLAYER_LEADERBOARD_PLACE) <= 0
                || e.tag(tags::BACON_COMBAT_PHASE_HERO) != 0
                || e.tag(tags::ZONE) == zone::REMOVEDFROMGAME
            {
                continue;
            }
            let slot = best.entry(lobby_id).or_insert(e);
            if e.id > slot.id {
                *slot = e;
            }
        }
        // Our own hero is known exactly; other copies may linger after death.
        let local = self.local_lobby_id();
        if let Some(hero) = self.local_hero() {
            best.insert(local, hero);
        }
        let mut list: Vec<LeaderboardEntry> = best
            .into_iter()
            .map(|(lobby_id, e)| LeaderboardEntry {
                lobby_id,
                place: e.tag(tags::PLAYER_LEADERBOARD_PLACE),
                hero_card_id: e.card_id.clone(),
                health: e.tag(tags::HEALTH) - e.tag(tags::DAMAGE),
                armor: e.tag(tags::ARMOR),
                is_local: lobby_id == local,
            })
            .collect();
        list.sort_by_key(|p| (p.place, p.lobby_id));
        list
    }

    /// Active Battlegrounds counters and spells (Blood Gems, Free Rolls, etc.).
    pub fn counters(&self, lobby_tribes: &[Tribe]) -> Vec<CounterItem> {
        let (Some((local_pid, local_pe)), _hero) = (self.local_player(), self.local_hero()) else {
            return Vec::new();
        };
        let pe = match self.entities.get(&local_pe) {
            Some(e) => e,
            None => return Vec::new(),
        };
        let hero_id = pe.tag(tags::HERO_ENTITY);
        let mut counters = Vec::new();

        // 1. Blood Gems (+X / +Y)
        let mut blood_gem_atk = pe.tag_or(tags::BACON_BLOODGEMBUFFATKVALUE, 1844);
        let mut blood_gem_hp = pe.tag_or(tags::BACON_BLOODGEMBUFFHEALTHVALUE, 2827);

        for e in self.entities.values() {
            if (e.tag(tags::CONTROLLER) == local_pid || e.tag(tags::ATTACHED) == local_pe || e.tag(tags::ATTACHED) == hero_id)
                && e.card_id == "BG26_159pe"
            {
                let a = e.tag_or(tags::TAG_SCRIPT_DATA_NUM_1, 2);
                let h = e.tag_or(tags::TAG_SCRIPT_DATA_NUM_2, 3);
                blood_gem_atk = blood_gem_atk.max(a);
                blood_gem_hp = blood_gem_hp.max(h);
            }
        }

        let has_quilboar_lobby = lobby_tribes.contains(&Tribe::Quilboar);
        let has_blood_gem_entity = self.entities.values().any(|e| {
            e.tag(tags::CONTROLLER) == local_pid
                && (e.tag(tags::ZONE) == zone::PLAY || e.tag(tags::ZONE) == zone::HAND)
                && (e.card_id.starts_with("BG20_GEM") || e.card_id == "BGS_1G")
        });

        if blood_gem_atk > 0 || blood_gem_hp > 0 || has_quilboar_lobby || has_blood_gem_entity {
            let total_atk = 1 + blood_gem_atk;
            let total_hp = 1 + blood_gem_hp;
            counters.push(CounterItem {
                id: "blood_gem".into(),
                card_id: "BG20_GEM".into(),
                label: "Blood Gem".into(),
                value: format!("+{total_atk} / +{total_hp}"),
                sort_order: 1,
            });
        }

        // 2. Free Rolls
        let mut free_refreshes = pe.tag_or(tags::BACON_FREE_REFRESH_COUNT, 4536);
        for e in self.entities.values() {
            if (e.tag(tags::CONTROLLER) == local_pid || e.tag(tags::ATTACHED) == local_pe || e.tag(tags::ATTACHED) == hero_id)
                && e.tag(tags::ZONE) != zone::REMOVEDFROMGAME
            {
                let count = e.tag_or(tags::BACON_FREE_REFRESH_COUNT, 4536);
                if count > 0 {
                    free_refreshes = free_refreshes.max(count);
                }
                let hero_count = e.tag_or("BACON_NUMBER_HERO_REFRESH_AVAILABLE", 2452);
                if hero_count > 0 {
                    free_refreshes = free_refreshes.max(hero_count);
                }
            }
        }
        if free_refreshes > 0 {
            counters.push(CounterItem {
                id: "free_refresh".into(),
                card_id: "BGS_116".into(),
                label: "Free Rolls".into(),
                value: format!("{free_refreshes}"),
                sort_order: 2,
            });
        }

        // 3. Tavern Spells Buff
        let tspell_atk = pe.tag_or(tags::TAVERN_SPELL_ATTACK_INCREASE, 3989);
        let tspell_hp = pe.tag_or(tags::TAVERN_SPELL_HEALTH_INCREASE, 3990);
        if tspell_atk > 0 || tspell_hp > 0 {
            counters.push(CounterItem {
                id: "tavern_spells".into(),
                card_id: "BG28_168".into(),
                label: "Tavern Spells".into(),
                value: format!("+{tspell_atk} / +{tspell_hp}"),
                sort_order: 3,
            });
        }

        // 4. Extra Gold Next Turn
        let mut extra_gold = pe.tag_or(tags::BACON_PLAYER_EXTRA_GOLD_NEXT_TURN, 4286);
        for e in self.entities.values() {
            if e.tag(tags::CONTROLLER) == local_pid && e.tag(tags::ZONE) == zone::PLAY {
                if e.card_id == "BG26_147" {
                    extra_gold += 1;
                } else if e.card_id == "BG26_147_G" {
                    extra_gold += 2;
                }
            }
        }
        if extra_gold > 0 {
            counters.push(CounterItem {
                id: "extra_gold".into(),
                card_id: "BG28_800".into(),
                label: "Next Turn Gold".into(),
                value: format!("+{extra_gold}"),
                sort_order: 4,
            });
        }

        // 5. Spells Played for Nagas
        let spells_played = pe.tag_or("3809", 3809);
        let has_naga_synergy = self.entities.values().any(|e| {
            e.tag(tags::CONTROLLER) == local_pid
                && e.tag(tags::ZONE) == zone::PLAY
                && (e.card_id == "BG31_924" || e.card_id == "BG31_924_G" || e.card_id == "BG28_600")
        });
        if spells_played > 0 && (has_naga_synergy || spells_played > 2) {
            let procs = 1 + spells_played / 3;
            let progress = spells_played % 3;
            counters.push(CounterItem {
                id: "naga_spells".into(),
                card_id: "BG31_924".into(),
                label: "Spells Played".into(),
                value: format!("{procs} ({progress}/3)"),
                sort_order: 5,
            });
        }

        // 6. Undead Army
        let mut undead_atk = 0;
        let mut undead_hp = 0;
        for e in self.entities.values() {
            if (e.tag(tags::CONTROLLER) == local_pid || e.tag(tags::ATTACHED) == local_pe)
                && (e.card_id == "BG25_011pe" || e.card_id == "BG_ShopBuff_Undead" || e.card_id.contains("UndeadBonus"))
            {
                undead_atk = undead_atk.max(e.tag_or(tags::TAG_SCRIPT_DATA_NUM_1, 2));
                undead_hp = undead_hp.max(e.tag_or(tags::TAG_SCRIPT_DATA_NUM_2, 3));
            }
        }
        if undead_atk > 0 || undead_hp > 0 {
            let val_str = if undead_hp > 0 {
                format!("+{undead_atk} / +{undead_hp}")
            } else {
                format!("+{undead_atk}")
            };
            counters.push(CounterItem {
                id: "undead_army".into(),
                card_id: "BG25_011".into(),
                label: "Undead Army".into(),
                value: val_str,
                sort_order: 6,
            });
        }

        // 7. Beetle Army
        let mut beetle_atk = 0;
        let mut beetle_hp = 0;
        for e in self.entities.values() {
            if (e.tag(tags::CONTROLLER) == local_pid || e.tag(tags::ATTACHED) == local_pe)
                && (e.card_id == "BG31_808pe" || e.card_id.contains("BeetleArmy"))
            {
                beetle_atk = beetle_atk.max(e.tag_or(tags::TAG_SCRIPT_DATA_NUM_1, 2));
                beetle_hp = beetle_hp.max(e.tag_or(tags::TAG_SCRIPT_DATA_NUM_2, 3));
            }
        }
        if beetle_atk > 0 || beetle_hp > 0 {
            counters.push(CounterItem {
                id: "beetles".into(),
                card_id: "BG28_603t".into(),
                label: "Beetles Army".into(),
                value: format!("{} / {}", 1 + beetle_atk, 1 + beetle_hp),
                sort_order: 7,
            });
        }

        // 8. Elemental Tavern Buff
        let mut elem_atk = 0;
        let mut elem_hp = 0;
        for e in self.entities.values() {
            if (e.tag(tags::CONTROLLER) == local_pid || e.tag(tags::ATTACHED) == local_pe)
                && (e.card_id == "BGS_104pe" || e.card_id == "BG30_MagicItem_544pe" || e.card_id == "BG34_855pe" || e.card_id.contains("ElementalShopBuff"))
            {
                elem_atk = elem_atk.max(e.tag_or(tags::TAG_SCRIPT_DATA_NUM_1, 2));
                elem_hp = elem_hp.max(e.tag_or(tags::TAG_SCRIPT_DATA_NUM_2, 3));
            }
        }
        if elem_atk > 0 || elem_hp > 0 {
            counters.push(CounterItem {
                id: "elemental_tavern".into(),
                card_id: "BG_EX1_575".into(),
                label: "Elemental Shop".into(),
                value: format!("+{elem_atk} / +{elem_hp}"),
                sort_order: 8,
            });
        }

        counters.sort_by_key(|c| c.sort_order);
        counters
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const P: &str = "D 00:00:00.0000000 PowerTaskList.DebugPrintPower() - ";

    fn feed(g: &mut Game, lines: &[&str]) {
        for l in lines {
            if l.starts_with("D ") {
                g.process(l);
            } else {
                g.process(&format!("{P}{l}"));
            }
        }
    }

    fn base_game() -> Game {
        let mut g = Game::new();
        g.set_source("test");
        feed(
            &mut g,
            &[
                "D 00:00:00.0000000 GameState.DebugPrintGame() - GameType=GT_BATTLEGROUNDS",
                "D 00:00:00.0000000 GameState.DebugPrintGame() - PlayerID=8, PlayerName=Me#1",
                "D 00:00:00.0000000 GameState.DebugPrintGame() - PlayerID=16, PlayerName=Someone",
                "CREATE_GAME",
                "    GameEntity EntityID=1",
                "        tag=TURN value=1",
                "    Player EntityID=2 PlayerID=8 GameAccountId=[hi=1 lo=2]",
                "        tag=HERO_ENTITY value=10",
                "        tag=PLAYER_TECH_LEVEL value=2",
                "    Player EntityID=3 PlayerID=16 GameAccountId=[hi=0 lo=0]",
                "        tag=BACON_DUMMY_PLAYER value=1",
                "        tag=HERO_ENTITY value=11",
                "FULL_ENTITY - Creating ID=10 CardID=HERO_A",
                "    tag=CONTROLLER value=8",
                "    tag=CARDTYPE value=HERO",
                "    tag=ZONE value=PLAY",
                "    tag=PLAYER_ID value=3",
                "    tag=HEALTH value=30",
                "    tag=PLAYER_LEADERBOARD_PLACE value=2",
                "FULL_ENTITY - Creating ID=11 CardID=TB_BaconShopBob",
                "    tag=CONTROLLER value=16",
                "    tag=CARDTYPE value=HERO",
                "    tag=ZONE value=PLAY",
                "FULL_ENTITY - Creating ID=12 CardID=HERO_B",
                "    tag=CONTROLLER value=16",
                "    tag=CARDTYPE value=HERO",
                "    tag=ZONE value=SETASIDE",
                "    tag=PLAYER_ID value=5",
                "    tag=HEALTH value=30",
                "    tag=PLAYER_LEADERBOARD_PLACE value=1",
                // Shop minion offered on turn 1.
                "FULL_ENTITY - Creating ID=20 CardID=BG31_803",
                "    tag=CONTROLLER value=16",
                "    tag=CARDTYPE value=MINION",
                "    tag=ZONE value=PLAY",
                // Our board.
                "FULL_ENTITY - Creating ID=30 CardID=BG25_001",
                "    tag=CONTROLLER value=8",
                "    tag=CARDTYPE value=MINION",
                "    tag=ZONE value=PLAY",
                "    tag=ZONE_POSITION value=1",
                "    tag=ATK value=2",
                "    tag=HEALTH value=1",
            ],
        );
        g
    }

    #[test]
    fn tracks_turn_tier_shop_and_leaderboard() {
        let g = base_game();
        assert!(g.in_progress());
        assert_eq!(g.turn(), 1);
        assert_eq!(g.tavern_tier(), 2);
        assert_eq!(g.shop_cards(), ["BG31_803".to_string()]);
        let lb = g.leaderboard();
        assert_eq!(lb.iter().map(|p| p.lobby_id).collect::<Vec<_>>(), vec![5, 3]);
        assert!(lb[1].is_local);
    }

    #[test]
    fn snapshot_taken_when_combat_setup_block_closes() {
        let mut g = base_game();
        g.take_events();
        feed(
            &mut g,
            &[
                "    TAG_CHANGE Entity=GameEntity tag=TURN value=2 ",
                "    TAG_CHANGE Entity=[entityName=x id=20 zone=PLAY zonePos=1 cardId=BG31_803 player=16] tag=ZONE value=REMOVEDFROMGAME ",
                "BLOCK_START BlockType=TRIGGER Entity=GameEntity EffectCardId=x EffectIndex=-1 Target=0 SubOption=-1 TriggerKeyword=TAG_NOT_SET",
                "    TAG_CHANGE Entity=Me#1 tag=BACON_CURRENT_COMBAT_PLAYER_ID value=3 ",
                "    TAG_CHANGE Entity=Opponent Name tag=BACON_CURRENT_COMBAT_PLAYER_ID value=5 ",
                "    FULL_ENTITY - Updating [entityName=Hero id=40 zone=PLAY zonePos=0 cardId=HERO_B player=16] CardID=HERO_B",
                "        tag=CONTROLLER value=16",
                "        tag=CARDTYPE value=HERO",
                "        tag=ZONE value=PLAY",
                "    TAG_CHANGE Entity=Opponent Name tag=HERO_ENTITY value=40 ",
                "    FULL_ENTITY - Updating [entityName=M id=41 zone=PLAY zonePos=1 cardId=BG36_345 player=16] CardID=BG36_345",
                "        tag=CONTROLLER value=16",
                "        tag=CARDTYPE value=MINION",
                "        tag=ZONE value=PLAY",
                "        tag=ZONE_POSITION value=1",
                "        tag=ATK value=3",
                "        tag=HEALTH value=3",
                "    FULL_ENTITY - Updating [entityName=E id=42 zone=PLAY zonePos=0 cardId=ENCH player=16] CardID=ENCH",
                "        tag=CONTROLLER value=16",
                "        tag=CARDTYPE value=ENCHANTMENT",
                "        tag=ZONE value=PLAY",
                "        tag=ATTACHED value=41",
                "    FULL_ENTITY - Updating [entityName=HandM id=43 zone=HAND zonePos=1 cardId=BG32_330_G player=16] CardID=BG32_330_G",
                "        tag=CONTROLLER value=16",
                "        tag=CARDTYPE value=MINION",
                "        tag=ZONE value=HAND",
                "        tag=ZONE_POSITION value=1",
                "        tag=ATK value=6",
                "        tag=HEALTH value=6",
                "    FULL_ENTITY - Updating [entityName=HiddenHand id=44 zone=HAND zonePos=2 cardId= player=16] CardID=",
                "        tag=CONTROLLER value=16",
                "        tag=CARDTYPE value=MINION",
                "        tag=ZONE value=HAND",
                "        tag=ZONE_POSITION value=2",
            ],
        );
        assert!(g.take_events().is_empty(), "snapshot must wait for the block to close");
        feed(&mut g, &["BLOCK_END"]);
        let events = g.take_events();
        let [GameEvent::Combat(snap)] = events.as_slice() else { panic!("{events:?}") };
        assert_eq!(snap.turn, 1);
        assert_eq!(snap.damage_cap, Some(5));
        assert_eq!(snap.player.lobby_id, 3);
        assert_eq!(snap.opponent.lobby_id, 5);
        assert_eq!(snap.opponent.hero.as_ref().unwrap().card_id, "HERO_B");
        assert_eq!(snap.player.board.len(), 1);
        assert_eq!(snap.opponent.board[0].card_id, "BG36_345");
        assert_eq!(snap.opponent.board[0].enchantments[0].card_id, "ENCH");
        assert_eq!(snap.opponent.hand.len(), 1);
        assert_eq!(snap.opponent.hand[0].card_id, "BG32_330_G");
        // Combat minions are not shop offers.
        assert_eq!(g.shop_cards().len(), 1);
        assert_eq!(g.seen_board(5).unwrap().side.board.len(), 1);
    }

    #[test]
    fn damage_cap_progression_and_removal() {
        let mut g = base_game();
        // Turn 1..3 defaults to 5
        assert_eq!(g.damage_cap(), Some(5));

        // Turn 4..7 defaults to 10
        feed(&mut g, &["    TAG_CHANGE Entity=GameEntity tag=TURN value=7 "]);
        assert_eq!(g.turn(), 4);
        assert_eq!(g.damage_cap(), Some(10));

        // Turn 8+ defaults to 15
        feed(&mut g, &["    TAG_CHANGE Entity=GameEntity tag=TURN value=15 "]);
        assert_eq!(g.turn(), 8);
        assert_eq!(g.damage_cap(), Some(15));

        // Explicit tags on GameEntity
        feed(&mut g, &[
            "    TAG_CHANGE Entity=GameEntity tag=BACON_COMBAT_DAMAGE_CAP value=10 ",
            "    TAG_CHANGE Entity=GameEntity tag=BACON_COMBAT_DAMAGE_CAP_ENABLED value=1 ",
        ]);
        assert_eq!(g.damage_cap(), Some(10));

        // Disabled damage cap (e.g. top 4 reached)
        feed(&mut g, &[
            "    TAG_CHANGE Entity=GameEntity tag=BACON_COMBAT_DAMAGE_CAP_ENABLED value=0 ",
        ]);
        assert_eq!(g.damage_cap(), None);
    }

    #[test]
    fn game_end_reports_place() {
        let mut g = base_game();
        g.take_events();
        feed(
            &mut g,
            &[
                "    TAG_CHANGE Entity=[entityName=H id=10 zone=PLAY zonePos=0 cardId=HERO_A player=8] tag=PLAYER_LEADERBOARD_PLACE value=4 ",
                "    TAG_CHANGE Entity=GameEntity tag=STEP value=FINAL_GAMEOVER ",
            ],
        );
        let events = g.take_events();
        let [GameEvent::GameEnded(r)] = events.as_slice() else { panic!("{events:?}") };
        assert_eq!((r.place, r.hero_card_id.as_str()), (4, "HERO_A"));
        assert!(r.game_id.starts_with("test#"));
        assert!(!g.in_progress());
    }

    #[test]
    fn counters_tracks_blood_gems_and_free_rolls() {
        let mut g = base_game();
        // With no Quilboar and no buffs, counters are empty.
        assert!(g.counters(&[Tribe::Beast, Tribe::Mech]).is_empty());

        // Quilboar in lobby shows base +1 / +1 Blood Gems.
        let c = g.counters(&[Tribe::Quilboar, Tribe::Beast]);
        assert_eq!(c.len(), 1);
        assert_eq!(c[0].id, "blood_gem");
        assert_eq!(c[0].value, "+1 / +1");

        // Player receives Blood Gem buffs (+2 atk, +4 hp) -> +3 / +5
        feed(
            &mut g,
            &[
                "    TAG_CHANGE Entity=Me#1 tag=BACON_BLOODGEMBUFFATKVALUE value=2 ",
                "    TAG_CHANGE Entity=Me#1 tag=BACON_BLOODGEMBUFFHEALTHVALUE value=4 ",
            ],
        );
        let c = g.counters(&[Tribe::Quilboar]);
        assert_eq!(c[0].value, "+3 / +5");

        // Friendly minion has blood gems played on it with huge stats (+100/+100).
        // The Blood Gem counter must NOT use minion enchantments; it must show what each blood gem gives (+3/+5).
        feed(
            &mut g,
            &[
                "FULL_ENTITY - Creating ID=60 CardID=BG20_GEMe",
                "    tag=CONTROLLER value=8",
                "    tag=CARDTYPE value=ENCHANTMENT",
                "    tag=ATTACHED value=10",
                "    tag=ZONE value=PLAY",
                "    tag=TAG_SCRIPT_DATA_NUM_1 value=100",
                "    tag=TAG_SCRIPT_DATA_NUM_2 value=100",
                "FULL_ENTITY - Creating ID=61 CardID=BG20_GEMe2",
                "    tag=CONTROLLER value=8",
                "    tag=CARDTYPE value=ENCHANTMENT",
                "    tag=ATTACHED value=10",
                "    tag=ZONE value=PLAY",
                "    tag=TAG_SCRIPT_DATA_NUM_1 value=100",
                "    tag=TAG_SCRIPT_DATA_NUM_2 value=100",
            ],
        );
        let c = g.counters(&[Tribe::Quilboar]);
        assert_eq!(c[0].value, "+3 / +5", "must track what each blood gem gives, not minion cumulative stats");

        // Player enchantment BG26_159pe (Blood Gem Player Enchant) buffs blood gems (+8/+8 -> +9/+9)
        feed(
            &mut g,
            &[
                "FULL_ENTITY - Creating ID=70 CardID=BG26_159pe",
                "    tag=CONTROLLER value=8",
                "    tag=CARDTYPE value=ENCHANTMENT",
                "    tag=ATTACHED value=2",
                "    tag=ZONE value=PLAY",
                "    tag=TAG_SCRIPT_DATA_NUM_1 value=8",
                "    tag=TAG_SCRIPT_DATA_NUM_2 value=8",
            ],
        );
        let c = g.counters(&[Tribe::Quilboar]);
        assert_eq!(c[0].value, "+9 / +9", "base 1/1 + player buff 8/8 gives +9/+9");

        // Player receives 25 Free Rolls via Bacon_Free_Refresh_Player_Ench
        feed(
            &mut g,
            &[
                "FULL_ENTITY - Creating ID=50 CardID=Bacon_Free_Refresh_Player_Ench",
                "    tag=CONTROLLER value=8",
                "    tag=CARDTYPE value=ENCHANTMENT",
                "    tag=ATTACHED value=2",
                "    tag=ZONE value=PLAY",
                "    tag=BACON_FREE_REFRESH_COUNT value=25",
            ],
        );
        let c = g.counters(&[Tribe::Quilboar]);
        assert_eq!(c.len(), 2);
        let free = c.iter().find(|x| x.id == "free_refresh").unwrap();
        assert_eq!(free.label, "Free Rolls");
        assert_eq!(free.value, "25");

        // When free rolls are spent down to 0, it disappears from counters.
        feed(&mut g, &["    TAG_CHANGE Entity=50 tag=BACON_FREE_REFRESH_COUNT value=0 "]);
        let c = g.counters(&[Tribe::Quilboar]);
        assert_eq!(c.len(), 1);
        assert_eq!(c[0].id, "blood_gem");

        // Tavern spell buffs
        feed(
            &mut g,
            &[
                "    TAG_CHANGE Entity=Me#1 tag=TAVERN_SPELL_ATTACK_INCREASE value=1 ",
                "    TAG_CHANGE Entity=Me#1 tag=TAVERN_SPELL_HEALTH_INCREASE value=2 ",
            ],
        );
        let c = g.counters(&[Tribe::Quilboar]);
        let tspell = c.iter().find(|x| x.id == "tavern_spells").unwrap();
        assert_eq!(tspell.label, "Tavern Spells");
        assert_eq!(tspell.value, "+1 / +2");
    }

    #[test]
    fn test_counters_from_real_log() {
        let path = "/home/paulo/Games/battlenet/drive_c/Program Files (x86)/Hearthstone/Logs/Hearthstone_2026_10_26_19_29_35/Power.log";
        if let Ok(text) = std::fs::read_to_string(path) {
            let mut g = Game::new();
            for line in text.lines() {
                g.process(line);
            }
            let c = g.counters(&[Tribe::Quilboar]);
            let bg = c.iter().find(|x| x.id == "blood_gem").expect("blood gem counter");
            assert_eq!(bg.value, "+15 / +17");
        }
    }
}
