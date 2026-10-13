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
        let mut opponent = self.side(dummy_pid, dummy_e, false);
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
        let snapshot = CombatSnapshot {
            turn,
            players_alive: self.leaderboard().iter().filter(|p| p.health > 0).count() as i32,
            anomalies,
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
                (card_type::MINION, zone::HAND) if with_hand => side.hand.push(raw()),
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
            ],
        );
        assert!(g.take_events().is_empty(), "snapshot must wait for the block to close");
        feed(&mut g, &["BLOCK_END"]);
        let events = g.take_events();
        let [GameEvent::Combat(snap)] = events.as_slice() else { panic!("{events:?}") };
        assert_eq!(snap.turn, 1);
        assert_eq!(snap.player.lobby_id, 3);
        assert_eq!(snap.opponent.lobby_id, 5);
        assert_eq!(snap.opponent.hero.as_ref().unwrap().card_id, "HERO_B");
        assert_eq!(snap.player.board.len(), 1);
        assert_eq!(snap.opponent.board[0].card_id, "BG36_345");
        assert_eq!(snap.opponent.board[0].enchantments[0].card_id, "ENCH");
        // Combat minions are not shop offers.
        assert_eq!(g.shop_cards().len(), 1);
        assert_eq!(g.seen_board(5).unwrap().side.board.len(), 1);
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
}
