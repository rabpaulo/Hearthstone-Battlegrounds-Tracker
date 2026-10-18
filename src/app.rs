//! The overlay application: log → game state → panel models → X11 windows.

use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

use anyhow::Result;
use x11rb::protocol::Event;

use crate::cards::{Card, CardDb, Category, GroupKind, Kind, Tribe};
use crate::comps::{CompGuide, Comps, CompsState};
use crate::config::{self, CompactMode, Config, LayoutSize, LeaderboardGeometry};
use crate::game::{tags, CombatSnapshot, Game, GameEvent, RawEntity};
use crate::history::{self, GameRecord, History};
use crate::images::{ImageKind, Images};
use crate::logfile::tail::LogTailer;
use crate::memory::{LobbyRaces, MemoryEvent};
use crate::sim::{Odds, SimClient, SimEvent};
use crate::ui::draw::{Canvas, Fonts, Rgba};
use crate::ui::panels::{
    self, BoardView, CardPreview, Calibration, CompHit, CompItem, CompPanel, CompRow, Ctx, GameRow, InfoPanel, MinionView, OddsView,
    OpponentPopup, SearchHit, SearchPanel, SearchRow, SettingsHit, SettingsPanel, SimPanel, TierHit, TierList, TierRow,
    TierSection, TopBar, TopHit,
};
use crate::ui::theme::{Theme, ThemeName};
use crate::x11::{GameWindow, Key, OverlayWindow, Rect, X11};

const TICK: Duration = Duration::from_millis(30);
/// Stop the simulator after this long without a Battlegrounds game.
const SIM_IDLE: Duration = Duration::from_secs(10 * 60);
/// Search results listed at once.
const SEARCH_ROWS: usize = 60;
const SEARCH_MAX_CHARS: usize = 40;

#[derive(Debug, Default)]
pub struct SimView {
    pub turn: i32,
    pub odds: Option<Odds>,
    pub running: bool,
    pub error: Option<String>,
}

#[derive(Debug, Default)]
pub struct UiState {
    pub selected_tier: Option<i32>,
    /// Tier list shows only the lobby's tribes.
    pub hide_unavailable: bool,
    /// Keyword filter for the tier list (index into `KEYWORDS`).
    pub keyword: Option<usize>,
    pub filter_open: bool,
    /// Top bar button under the pointer.
    pub top_hover: Option<TopHit>,
    pub scroll: f32,
    /// Element of the tier list under the pointer.
    pub list_hover: Option<TierHit>,
    /// Leaderboard slot (0 = first place) under the pointer, or whose
    /// popup the pointer is on.
    pub slot_hover: Option<usize>,
    /// Turn chip of the opponent popup under the pointer.
    pub popup_hover: Option<usize>,
    /// Turns per lobby id whose board is toggled away from the default
    /// (only the newest one shown).
    pub board_flips: HashMap<i32, HashSet<i32>>,
    pub comps_open: bool,
    /// Comp whose guide is shown instead of the tier list.
    pub comp_id: Option<i32>,
    pub comp_scroll: f32,
    /// Where the tier list was scrolled before opening a guide.
    pub comp_list_scroll: f32,
    pub comp_hover: Option<CompHit>,
    pub search_open: bool,
    pub search_query: String,
    /// Card type the search is narrowed to.
    pub search_category: Option<Category>,
    /// Key presses go to the search field.
    pub search_focus: bool,
    pub search_scroll: f32,
    pub search_hover: Option<SearchHit>,
    pub settings_open: bool,
    /// Theme row of the settings panel under the pointer.
    pub settings_hover: Option<SettingsHit>,
    /// A theme that could not be written to the config, shown in the panel.
    pub settings_error: Option<String>,
}

impl UiState {
    /// Tier list row under the pointer.
    pub fn row_hover(&self) -> Option<usize> {
        match self.list_hover {
            Some(TierHit::Row(i)) => Some(i),
            _ => None,
        }
    }

    /// Comps panel item under the pointer.
    pub fn comp_item_hover(&self) -> Option<usize> {
        match self.comp_hover {
            Some(CompHit::Item(i)) => Some(i),
            _ => None,
        }
    }

    /// Search result under the pointer or picked with the arrow keys.
    pub fn search_row_hover(&self) -> Option<usize> {
        match self.search_hover {
            Some(SearchHit::Row(i)) => Some(i),
            _ => None,
        }
    }

    /// Opens or closes a panel; only one of tiers, comps, search and
    /// settings is open.
    fn close_panels(&mut self) {
        self.selected_tier = None;
        self.comps_open = false;
        self.search_open = false;
        self.search_focus = false;
        self.settings_open = false;
        self.settings_hover = None;
    }

    /// Applies a key typed into the search field. `rows` is the number of
    /// results on screen. Returns whether the query changed.
    pub fn search_key(&mut self, key: Key, rows: usize) -> bool {
        let q = &mut self.search_query;
        let edited = match key {
            Key::Char(c) if !c.is_control() && q.chars().count() < SEARCH_MAX_CHARS => {
                q.push(c);
                true
            }
            Key::Char(_) => false,
            Key::Backspace => q.pop().is_some(),
            Key::DeleteWord => {
                let keep = q.trim_end().rfind(' ').map(|i| i + 1).unwrap_or(0);
                let changed = keep < q.len();
                q.truncate(keep);
                changed
            }
            Key::Clear => {
                let changed = !q.is_empty();
                q.clear();
                changed
            }
            Key::Escape => {
                self.search_open = false;
                self.search_focus = false;
                false
            }
            Key::Enter => {
                self.search_focus = false;
                false
            }
            Key::Tab | Key::BackTab => {
                let order: Vec<Option<Category>> = std::iter::once(None).chain(Category::ALL.map(Some)).collect();
                let at = order.iter().position(|k| *k == self.search_category).unwrap_or(0);
                let step = if key == Key::Tab { 1 } else { order.len() - 1 };
                self.search_category = order[(at + step) % order.len()];
                true
            }
            Key::Up | Key::Down | Key::PageUp | Key::PageDown => {
                if rows > 0 {
                    let cur = self.search_row_hover();
                    let next = match (key, cur) {
                        (Key::Up, Some(i)) => i.saturating_sub(1),
                        (Key::PageUp, Some(i)) => i.saturating_sub(8),
                        (Key::Down, Some(i)) => i + 1,
                        (Key::PageDown, Some(i)) => i + 8,
                        _ => 0,
                    };
                    self.search_hover = Some(SearchHit::Row(next.min(rows - 1)));
                }
                false
            }
        };
        if edited {
            // The best match is highlighted, so its card shows right away.
            self.search_scroll = 0.0;
            self.search_hover = Some(SearchHit::Row(0));
        }
        edited
    }
}

/// Where things go, relative to the game window.
#[derive(Debug, Clone, Copy)]
pub struct Layout {
    pub hs: Rect,
    pub s: f32,
    /// Top and height of the area the game actually draws in: windows
    /// narrower than 4:3 are letterboxed.
    game_y: f32,
    game_h: f32,
    lb: LeaderboardGeometry,
}

impl Layout {
    pub fn new(hs: Rect, cfg: &Config) -> Layout {
        let (w, h) = (hs.w as f32, hs.h as f32);
        let game_h = h.min(w * 3.0 / 4.0);
        let game_y = hs.y as f32 + (h - game_h) / 2.0;
        Layout { hs, s: game_h / 1080.0 * cfg.scale, game_y, game_h, lb: cfg.leaderboard }
    }

    pub fn slot(&self, i: usize) -> Rect {
        let h = self.game_h;
        let cx = self.hs.x as f32 + self.hs.w as f32 / 2.0 + self.lb.center_x * h;
        let w = self.lb.width * h;
        let y = self.game_y + (self.lb.top + i as f32 * self.lb.spacing) * h;
        Rect::new((cx - w / 2.0) as i32, y as i32, w as i32, (self.lb.height * h) as i32)
    }

    pub fn slot_at(&self, px: i32, py: i32) -> Option<usize> {
        (0..8).find(|&i| self.slot(i).contains(px, py))
    }

    fn px(&self, v: f32) -> i32 {
        (v * self.s).round() as i32
    }

    pub fn top_bar(&self, size: (i32, i32)) -> Rect {
        Rect::new(self.hs.x + self.hs.w - size.0 - self.px(8.0), self.hs.y + self.px(6.0), size.0, size.1)
    }

    pub fn tier_list(&self, top_bar: Rect, w: i32) -> (i32, i32, i32) {
        let x = self.hs.x + self.hs.w - w - self.px(8.0);
        let y = top_bar.y + top_bar.h + self.px(6.0);
        let max_h = self.hs.y + (self.hs.h as f32 * 0.9) as i32 - y;
        (x, y, max_h.max(self.px(120.0)))
    }

    /// Top centre; nudged left when it would cover the tier bar.
    pub fn sim(&self, size: (i32, i32), top_bar: Option<Rect>) -> Rect {
        let mut x = self.hs.x + (self.hs.w - size.0) / 2;
        if let Some(tb) = top_bar {
            x = x.min(tb.x - size.0 - self.px(8.0)).max(self.hs.x);
        }
        Rect::new(x, self.hs.y, size.0, size.1)
    }

    /// Unscaled width for the info panel: whatever fits left of the
    /// leaderboard, within reason.
    pub fn info_width(&self) -> f32 {
        let room = (self.slot(0).x - self.hs.x) as f32 / self.s - 16.0;
        room.clamp(180.0, 200.0)
    }

    pub fn info(&self, size: (i32, i32)) -> Rect {
        let y = (self.game_y + self.game_h * (self.lb.top - 0.01)) as i32;
        Rect::new(self.hs.x + self.px(6.0), y, size.0, size.1)
    }

    /// Tallest the opponent popup may be so it fits below the simulation panel.
    pub fn popup_max_h(&self, sim_rect: Rect) -> i32 {
        let y = sim_rect.y + sim_rect.h + self.px(6.0);
        (self.hs.y + self.hs.h - y - self.px(8.0)).max(self.px(100.0))
    }

    /// Top centre, directly below the combat simulation panel.
    pub fn popup(&self, size: (i32, i32), sim_rect: Rect) -> Rect {
        let mut x = sim_rect.x + (sim_rect.w - size.0) / 2;
        x = x.clamp(self.hs.x + self.px(8.0), (self.hs.x + self.hs.w - size.0 - self.px(8.0)).max(self.hs.x));
        let y = sim_rect.y + sim_rect.h + self.px(6.0);
        Rect::new(x, y, size.0, size.1)
    }
}

/// Everything except X11, so it can also render offline.
pub struct Overlay {
    pub cfg: Config,
    pub db: CardDb,
    pub game: Game,
    pub history: History,
    pub images: Images,
    pub fonts: Fonts,
    pub comps: Comps,
    pub sim_view: SimView,
    pub ui: UiState,
    /// Palette every panel draws with, and the choice it came from.
    pub theme: Theme,
    pub theme_name: ThemeName,
    last_combat: Option<Box<CombatSnapshot>>,
    /// Lobby tribes read from the game's memory, with the `GameState` they
    /// belong to.
    lobby_memory: Option<LobbyRaces>,
    /// `GameState` of the last finished game: its tribes are stale.
    ended_game: Option<u64>,
    /// Last search and the ids it found; panels redraw on every pointer
    /// move, the query changes far less often.
    search_cache: std::cell::RefCell<Option<SearchHits>>,
}

/// A query, the type it was narrowed to, and the ids it found.
type SearchHits = (String, Option<Category>, Vec<String>);

/// The lobby's tribes as far as they are known.
#[derive(Debug, Clone, PartialEq)]
pub struct LobbyTribes {
    pub tribes: Vec<Tribe>,
    /// All tribes of the lobby are known.
    pub complete: bool,
    /// Read from the game rather than inferred from the shop.
    pub from_game: bool,
}

fn keywords(e: &RawEntity) -> Vec<&'static str> {
    let mut k = Vec::new();
    if e.tag(tags::TAUNT) == 1 {
        k.push("Taunt");
    }
    if e.tag(tags::DIVINE_SHIELD) > 0 {
        k.push("Shield");
    }
    if e.tag(tags::REBORN) == 1 {
        k.push("Reborn");
    }
    if e.tag(tags::VENOMOUS) == 1 || e.tag(tags::POISONOUS) == 1 {
        k.push("Venomous");
    }
    match e.tag(tags::WINDFURY) {
        3 => k.push("Mega-Windfury"),
        1 => k.push("Windfury"),
        _ => {}
    }
    if e.tag(tags::STEALTH) == 1 {
        k.push("Stealth");
    }
    k
}

pub fn placement_delta(place: i32) -> i32 {
    match place {
        1 => 100,
        2 => 70,
        3 => 40,
        4 => 10,
        5 => -15,
        6 => -45,
        7 => -75,
        8 => -100,
        _ => 0,
    }
}

impl Overlay {
    pub fn new(cfg: Config, db: CardDb, history: History) -> Overlay {
        let theme_name = cfg.theme;
        Overlay {
            db,
            game: Game::new(),
            history,
            images: Images::new(),
            fonts: Fonts::load(),
            comps: Comps::new(),
            sim_view: SimView::default(),
            ui: UiState { hide_unavailable: cfg.hide_unavailable_tribes, ..UiState::default() },
            theme: theme_name.theme(),
            theme_name,
            last_combat: None,
            lobby_memory: None,
            ended_game: None,
            search_cache: Default::default(),
            cfg,
        }
    }

    /// Switches the palette; the panels pick it up on the next draw.
    pub fn set_theme(&mut self, name: ThemeName) {
        self.theme_name = name;
        self.theme = name.theme();
        self.cfg.theme = name;
    }

    pub fn set_lobby_memory(&mut self, lobby: Option<LobbyRaces>) {
        self.lobby_memory = lobby;
    }

    /// Prefers what the game client holds (known from the hero pick on);
    /// falls back to minions Bob has offered. A memory reading is ignored when
    /// it belongs to a finished game or contradicts the shop.
    pub fn lobby_tribes(&self) -> LobbyTribes {
        let shop = self.db.infer_tribes(self.game.shop_cards());
        if let Some(mem) = &self.lobby_memory {
            let current = Some(mem.game) != self.ended_game;
            if current && self.game.in_progress() && shop.iter().all(|t| mem.tribes.contains(t)) {
                return LobbyTribes { tribes: mem.tribes.clone(), complete: true, from_game: true };
            }
        }
        let complete = shop.len() >= Tribe::PER_LOBBY;
        LobbyTribes { tribes: shop, complete, from_game: false }
    }

    pub fn tribes(&self) -> Vec<Tribe> {
        self.lobby_tribes().tribes
    }

    /// Simulator input: the snapshot plus the lobby tribes once all are known.
    pub fn sim_input(&self, snap: &CombatSnapshot) -> serde_json::Value {
        let mut v = serde_json::to_value(snap).unwrap_or_default();
        let lobby = self.lobby_tribes();
        if lobby.complete {
            v["validTribes"] = lobby.tribes.iter().map(|t| t.db_name()).collect();
        }
        v
    }

    /// Applies game events. Returns combats that should be simulated now.
    pub fn handle_events(&mut self, events: Vec<GameEvent>, catchup: bool) -> Vec<CombatSnapshot> {
        let mut to_simulate = Vec::new();
        for ev in events {
            match ev {
                GameEvent::GameStarted => {
                    self.sim_view = SimView::default();
                    self.ui.selected_tier = None;
                    self.ui.scroll = 0.0;
                    self.ui.comps_open = false;
                    self.ui.comp_id = None;
                    self.ui.search_open = false;
                    self.ui.search_focus = false;
                    self.ui.settings_open = false;
                    self.ui.settings_hover = None;
                    self.ui.board_flips.clear();
                    self.last_combat = None;
                    self.comps.refresh();
                }
                GameEvent::Combat(snap) => {
                    self.sim_view = SimView { turn: snap.turn, ..SimView::default() };
                    if self.game.is_duos() {
                        self.sim_view.error = Some("Odds are not available in Duos".into());
                    } else if !catchup {
                        self.sim_view.running = true;
                        to_simulate.push((*snap).clone());
                    }
                    self.last_combat = Some(snap);
                }
                GameEvent::GameEnded(r) => {
                    if !catchup {
                        self.ended_game = self.lobby_memory.as_ref().map(|m| m.game);
                    }
                    let delta = placement_delta(r.place);
                    let prev_rating = self.cfg.mmr.or_else(|| self.history.latest(1).first().and_then(|g| g.rating));
                    let new_rating = prev_rating.map(|rt| rt + delta);
                    if let Some(nr) = new_rating {
                        self.cfg.mmr = Some(nr);
                    }
                    let record = GameRecord {
                        id: r.game_id,
                        hero: r.hero_card_id,
                        place: r.place,
                        turn: r.turn,
                        ended_at: history::now_secs(),
                        tribes: self.tribes().iter().map(|t| t.label().to_string()).collect(),
                        rating: new_rating,
                        mmr_delta: Some(delta),
                    };
                    if self.history.add(record) {
                        log::info!("game finished: place {}", r.place);
                    }
                }
            }
        }
        to_simulate
    }

    /// After replaying history: the combat on screen, if the log ended
    /// mid-combat (the overlay was started during a fight).
    pub fn combat_in_progress(&mut self) -> Option<CombatSnapshot> {
        let snap = self.last_combat.as_ref()?;
        if self.game.in_progress() && self.game.in_combat() && snap.turn == self.game.turn() && !self.game.is_duos() {
            self.sim_view.running = true;
            return Some((**snap).clone());
        }
        None
    }

    pub fn on_sim_event(&mut self, ev: SimEvent) {
        match ev {
            SimEvent::Partial(o) => self.sim_view.odds = Some(o),
            SimEvent::Done(o) => {
                self.sim_view.odds = Some(o);
                self.sim_view.running = false;
            }
            SimEvent::Failed(msg) => {
                log::warn!("simulation failed: {msg}");
                self.sim_view.running = false;
                self.sim_view.error = Some(if msg.starts_with("unknown cards") {
                    "Odds unavailable: unknown card (update the simulator)".into()
                } else {
                    "Odds unavailable (simulator error)".into()
                });
            }
        }
    }

    // ---- models ----------------------------------------------------------

    pub fn top_bar(&self) -> TopBar {
        TopBar {
            turn: self.game.turn(),
            tavern_tier: self.game.tavern_tier(),
            max_tier: if self.cfg.show_tiers { self.game.max_tavern_tier() } else { 0 },
            selected: self.ui.selected_tier,
            hover: self.ui.top_hover,
            comps: self.cfg.show_comps,
            comps_open: self.ui.comps_open,
            search: self.cfg.show_search,
            search_open: self.ui.search_open,
            settings_open: self.ui.settings_open,
        }
    }

    pub fn tier_list(&self) -> Option<TierList> {
        let tier = self.ui.selected_tier?;
        let duos = self.game.is_duos();
        let lobby = self.lobby_tribes();
        let known = &lobby.tribes;
        // Lobby tribes first; the rest only when they are not known to be
        // out of the lobby, or when the player asked to see everything.
        let mut listed = known.clone();
        if !lobby.complete || !self.ui.hide_unavailable {
            listed.extend(Tribe::ALL.iter().filter(|t| !known.contains(t)));
        }
        let keyword = self.ui.keyword;
        let wanted = |c: &crate::cards::Card| keyword.is_none_or(|k| c.has_keyword(k));
        let row = |c: &crate::cards::Card, cost: bool| TierRow {
            card_id: c.id.clone(),
            name: c.name.clone(),
            cost: cost.then_some(c.cost),
        };
        let mut sections: Vec<TierSection> = self
            .db
            .tier_minions(tier, &listed, duos)
            .into_iter()
            .map(|g| {
                let (title, color, dim) = match g.kind {
                    GroupKind::Tribe(t) => {
                        let dim = !known.contains(&t);
                        let title = match (dim, lobby.complete) {
                            (false, _) => t.label().to_string(),
                            (true, false) => format!("{} ?", t.label()),
                            (true, true) => format!("{} · not in lobby", t.label()),
                        };
                        (title, panels::tribe_color(t), dim)
                    }
                    GroupKind::Neutral => ("Neutral".to_string(), self.theme.dim, false),
                    GroupKind::AllTribes => ("All tribes".to_string(), self.theme.gold, false),
                };
                TierSection { title, color, dim, rows: g.cards.iter().filter(|c| wanted(c)).map(|c| row(c, false)).collect() }
            })
            .filter(|s| !s.rows.is_empty())
            .collect();
        let spells: Vec<_> = self.db.tier_spells(tier, duos).into_iter().filter(|c| wanted(c)).collect();
        if !spells.is_empty() {
            sections.push(TierSection {
                title: "Spells".to_string(),
                color: self.theme.accent,
                dim: false,
                rows: spells.iter().map(|c| row(c, true)).collect(),
            });
        }
        let subtitle = if lobby.complete {
            "Lobby tribes".to_string()
        } else {
            format!("{}/{} tribes seen in shop", known.len(), Tribe::PER_LOBBY)
        };
        Some(TierList {
            tier,
            subtitle,
            sections,
            scroll: self.ui.scroll,
            hover: self.ui.list_hover,
            images_rev: self.images.revision(),
            hide_unavailable: self.ui.hide_unavailable,
            keyword: self.ui.keyword,
            filter_open: self.ui.filter_open,
        })
    }

    pub fn comp_panel(&self) -> Option<CompPanel> {
        if !self.ui.comps_open {
            return None;
        }
        let lobby = self.lobby_tribes();
        let in_lobby = |g: &CompGuide| g.tribe.is_none_or(|t| !lobby.complete || lobby.tribes.contains(&t));
        let tribe = |g: &CompGuide| g.tribe.map(|t| t.label()).unwrap_or("Mixed");
        let mut panel = CompPanel {
            title: "Meta Comps".to_string(),
            subtitle: String::new(),
            detail: None,
            items: Vec::new(),
            scroll: self.ui.comp_scroll,
            hover: self.ui.comp_hover,
            images_rev: self.images.revision(),
            hide_unavailable: self.ui.hide_unavailable,
        };
        match &self.comps.state {
            CompsState::Loading => {
                panel.subtitle = "HSReplay tier list".to_string();
                panel.items.push(CompItem::Note("Loading comps…".to_string()));
                return Some(panel);
            }
            CompsState::Failed => {
                panel.subtitle = "HSReplay tier list".to_string();
                panel.items.push(CompItem::Note("Could not load comps · reopen to retry".to_string()));
                return Some(panel);
            }
            CompsState::Ready(_) => {}
        }
        if let Some(g) = self.ui.comp_id.and_then(|id| self.comps.get(id)) {
            let mut sub = vec![format!("{} tier", g.tier_letter()), tribe(g).to_string()];
            sub.extend((!g.difficulty_label().is_empty()).then(|| g.difficulty_label().to_string()));
            if !in_lobby(g) {
                sub.push("not in lobby".to_string());
            }
            panel.title = g.name.clone();
            panel.subtitle = sub.join(" · ");
            panel.detail = Some(g.tier_letter());
            panel.items = self.comp_guide_items(g);
            return Some(panel);
        }
        panel.subtitle = if lobby.complete {
            "HSReplay tier list · this lobby".to_string()
        } else {
            "HSReplay tier list · all tribes".to_string()
        };
        let shown: Vec<&CompGuide> =
            self.comps.guides().iter().filter(|g| !(self.ui.hide_unavailable && !in_lobby(g))).collect();
        let hero = self.base_hero();
        for (i, g) in shown.iter().enumerate() {
            if i == 0 || shown[i - 1].tier != g.tier {
                let count = shown.iter().filter(|o| o.tier == g.tier).count();
                panel.items.push(CompItem::Tier { letter: g.tier_letter(), count });
            }
            let mut detail = vec![tribe(g)];
            detail.extend((!g.difficulty_label().is_empty()).then(|| g.difficulty_label()));
            let dim = !in_lobby(g);
            if dim {
                detail.push("not in lobby");
            }
            let your_hero = hero.is_some_and(|h| self.comps.enablers(g, &self.db).is_some_and(|a| a.heroes.iter().any(|e| e.hero == h)));
            if your_hero {
                detail.push("your hero");
            }
            panel.items.push(CompItem::Comp(CompRow {
                id: g.id,
                name: g.name.clone(),
                detail: detail.join(" · "),
                color: g.tribe.map(panels::tribe_color).unwrap_or(self.theme.gold),
                card_id: g.representative_card.clone(),
                trend: g.trend(),
                dim,
                your_hero,
            }));
        }
        if shown.is_empty() {
            panel.items.push(CompItem::Note("No comps for this lobby's tribes".to_string()));
        }
        Some(panel)
    }

    /// The player's hero as hero stats name it, without its skin.
    fn base_hero(&self) -> Option<&str> {
        let id = &self.game.local_hero()?.card_id;
        Some(id.split("_SKIN_").next().unwrap_or(id))
    }

    /// The guide of one comp: its cards, its hero enablers, then the strategy text.
    fn comp_guide_items(&self, g: &CompGuide) -> Vec<CompItem> {
        let mut items = Vec::new();
        let text = |items: &mut Vec<CompItem>, heading: Option<&'static str>, body: &str, strong: bool| {
            let weight = if strong { crate::ui::draw::Weight::Bold } else { crate::ui::draw::Weight::Medium };
            let lines = self.fonts.wrap(body, CompPanel::TEXT_PX, weight, CompPanel::TEXT_W);
            if lines.is_empty() {
                return;
            }
            items.extend(heading.map(CompItem::Heading));
            items.extend(lines.into_iter().map(|text| CompItem::Text { text, strong }));
        };
        text(&mut items, None, &g.summary, true);
        for (heading, dbfs) in [("CORE CARDS", &g.core_cards), ("ADD-ONS", &g.addon_cards)] {
            let cards: Vec<_> = dbfs.iter().filter_map(|&d| self.db.by_dbf(d)).collect();
            if cards.is_empty() {
                continue;
            }
            items.push(CompItem::Heading(heading));
            items.extend(cards.into_iter().map(|c| CompItem::Card {
                card_id: c.id.clone(),
                name: c.name.clone(),
                tier: c.tier,
            }));
        }
        if let Some(a) = self.comps.enablers(g, &self.db) {
            items.push(CompItem::Heading("HERO ENABLERS"));
            let note = match g.tribe {
                Some(t) if a.tribe_wide => format!("× more often in {} comps · avg place", t.label()),
                _ => "× more often than other heroes · avg place".to_string(),
            };
            text(&mut items, None, &note, false);
            let hero = self.base_hero();
            items.extend(a.heroes.iter().map(|e| CompItem::Hero {
                card_id: e.hero.clone(),
                name: self.db.name(&e.hero).to_string(),
                detail: format!("{:.1}× · {:.1}", e.lift, e.placement),
                yours: hero == Some(e.hero.as_str()),
            }));
        }
        text(&mut items, Some("WHEN TO COMMIT"), &g.when_to_commit, false);
        text(&mut items, Some("KEY ENABLERS"), &g.common_enablers, false);
        text(&mut items, Some("HOW TO PLAY"), &g.how_to_play, false);
        if items.first().is_some_and(|i| matches!(i, CompItem::Text { .. })) {
            items.insert(0, CompItem::Gap);
        }
        items.push(CompItem::Gap);
        items
    }

    pub fn search_panel(&self) -> Option<SearchPanel> {
        if !self.ui.search_open {
            return None;
        }
        let query = self.ui.search_query.trim();
        let category = self.ui.search_category;
        let browsing = query.is_empty() && category.is_none();
        let hits = if browsing { Vec::new() } else { self.search(query, category) };
        let n = hits.len();
        let subtitle = if browsing {
            "Fuzzy search · Tab switches the card type".to_string()
        } else {
            match category {
                Some(k) => k.count(n),
                None => format!("{n} card{}", if n == 1 { "" } else { "s" }),
            }
        };
        Some(SearchPanel {
            query: self.ui.search_query.clone(),
            focused: self.ui.search_focus,
            category,
            rows: hits.iter().take(SEARCH_ROWS).map(|c| search_row(c, &self.theme)).collect(),
            more: n.saturating_sub(SEARCH_ROWS),
            note: if browsing { "Type a name, or pick a card type".into() } else { "No cards match".into() },
            subtitle,
            scroll: self.ui.search_scroll,
            hover: self.ui.search_hover,
            images_rev: self.images.revision(),
        })
    }

    fn search(&self, query: &str, category: Option<Category>) -> Vec<&Card> {
        let mut cache = self.search_cache.borrow_mut();
        let fresh = cache.as_ref().is_some_and(|(q, k, _)| q == query && *k == category);
        if !fresh {
            let ids = self.db.search(query, category).iter().map(|c| c.id.clone()).collect();
            *cache = Some((query.to_string(), category, ids));
        }
        cache.as_ref().map(|(_, _, ids)| ids.iter().filter_map(|id| self.db.get(id)).collect()).unwrap_or_default()
    }

    /// Hover preview of a card; none in text mode, which shows no art.
    fn card_preview(&self, card_id: String) -> Option<CardPreview> {
        (self.cfg.compact_mode != CompactMode::Text)
            .then(|| CardPreview { card_id, images_rev: self.images.revision(), compact: self.cfg.compact_mode })
    }

    pub fn search_preview(&self, panel: &SearchPanel) -> Option<CardPreview> {
        let row = panel.row(self.ui.search_row_hover()?)?;
        self.card_preview(row.card_id.clone())
    }

    pub fn comp_preview(&self, panel: &CompPanel) -> Option<CardPreview> {
        match panel.item(self.ui.comp_item_hover()?)? {
            CompItem::Card { card_id, .. } | CompItem::Hero { card_id, .. } => self.card_preview(card_id.clone()),
            _ => None,
        }
    }

    pub fn sim_panel(&self) -> SimPanel {
        let v = &self.sim_view;
        let in_combat = self.game.in_combat() && v.turn == self.game.turn();
        let title = if v.turn == 0 {
            "Odds appear when combat starts".to_string()
        } else if in_combat {
            "Current Combat".to_string()
        } else {
            format!("Last Combat · Turn {}", v.turn)
        };
        SimPanel {
            title,
            odds: v.odds.map(|o| OddsView {
                won_lethal: o.won_lethal,
                won: o.won,
                tied: o.tied,
                lost: o.lost,
                lost_lethal: o.lost_lethal,
            }),
            dim: !in_combat,
            running: v.running,
            note: v.error.clone(),
        }
    }

    /// The settings panel's model: the current theme, compact mode and
    /// layout size, and where a pick is saved.
    pub fn settings_panel(&self, config_path: &Path) -> SettingsPanel {
        SettingsPanel {
            theme: self.theme_name,
            compact: self.cfg.compact_mode,
            scale: self.cfg.scale,
            config_path: short_home(config_path),
            save_error: self.ui.settings_error.clone(),
            hover: self.ui.settings_hover,
        }
    }

    pub fn info_panel(&mut self, width: f32) -> InfoPanel {
        let latest = self.history.latest(5);
        for g in &latest {
            self.images.prefetch(ImageKind::Tile, &g.hero);
        }
        let games: Vec<GameRow> = latest
            .into_iter()
            .map(|g| GameRow {
                hero_card_id: g.hero.clone(),
                hero_name: self.db.name(&g.hero).to_string(),
                place: g.place,
                mmr_delta: g.mmr_delta.or_else(|| Some(placement_delta(g.place))),
            })
            .collect();

        let session = self.history.session(history::now_secs());
        let session_delta: i32 = session
            .iter()
            .map(|g| g.mmr_delta.unwrap_or_else(|| placement_delta(g.place)))
            .sum();

        let current_mmr = self.cfg.mmr.or_else(|| self.history.latest(1).first().and_then(|g| g.rating));
        let start_mmr = current_mmr.map(|c| c - session_delta);

        InfoPanel {
            tribes: self.lobby_tribes().tribes,
            slots: Tribe::PER_LOBBY,
            start_mmr,
            current_mmr,
            games,
            width,
        }
    }

    pub fn popup(&self, slot: usize) -> Option<OpponentPopup> {
        let lb = self.game.leaderboard();
        let entry = lb.iter().find(|p| p.place == slot as i32 + 1)?;
        if entry.is_local {
            return None;
        }
        let flips = self.ui.board_flips.get(&entry.lobby_id);
        let boards: Vec<BoardView> = self
            .game
            .seen_boards(entry.lobby_id)
            .iter()
            .rev()
            .map(|b| BoardView {
                turn: b.turn,
                tier: b.side.tavern_tier(),
                minions: b
                    .side
                    .board
                    .iter()
                    .map(|m| MinionView {
                        card_id: m.card_id.clone(),
                        name: self.db.name(&m.card_id).to_string(),
                        attack: m.tag(tags::ATK),
                        health: m.tag(tags::HEALTH) - m.tag(tags::DAMAGE),
                        golden: m.tag(tags::PREMIUM) == 1 || m.card_id.ends_with("_G"),
                        keywords: keywords(m),
                    })
                    .collect(),
            })
            .collect();
        let shown = boards.iter().enumerate().map(|(i, b)| (i == 0) != flips.is_some_and(|f| f.contains(&b.turn))).collect();
        Some(OpponentPopup {
            lobby_id: entry.lobby_id,
            hero: self.db.name(&entry.hero_card_id).to_string(),
            hero_card_id: entry.hero_card_id.clone(),
            place: entry.place,
            health: entry.health + entry.armor,
            turn: self.game.turn(),
            boards,
            shown,
            hidden: 0,
            hover: self.ui.popup_hover,
            images_rev: self.images.revision(),
            compact: self.cfg.compact_mode,
        })
    }

    pub fn preview(&self, list: &TierList) -> Option<CardPreview> {
        let row = list.row(self.ui.row_hover()?)?;
        self.card_preview(row.card_id.clone())
    }
}

/// One search result: type and the details that tell look-alikes apart.
fn search_row(c: &Card, theme: &Theme) -> SearchRow {
    let category = c.category.unwrap_or(Category::Minion);
    let cost = (c.cost > 0).then(|| format!("{} gold", c.cost));
    let (mut detail, color) = match category {
        Category::Minion => {
            let tribes = if c.all_tribes {
                "All tribes".to_string()
            } else if c.tribes.is_empty() {
                "Neutral".to_string()
            } else {
                c.tribes.iter().map(|t| t.label()).collect::<Vec<_>>().join(", ")
            };
            let kind = if c.timewarped { "Timewarped minion" } else { "Minion" };
            let color = match c.tribes.first() {
                _ if c.all_tribes => theme.gold,
                Some(&t) => panels::tribe_color(t),
                None => theme.dim,
            };
            (vec![kind.to_string(), tribes], color)
        }
        Category::Spell => (vec!["Tavern spell".to_string()], theme.accent),
        Category::Hero => (vec!["Hero".to_string()], theme.gold),
        Category::Trinket => (vec!["Trinket".to_string()], Rgba::hex(0x9c6ad6)),
        Category::Anomaly => (vec!["Anomaly".to_string()], Rgba::hex(0x2a9d8f)),
        Category::Reward => (vec!["Quest reward".to_string()], theme.win),
    };
    if matches!(category, Category::Spell | Category::Trinket) {
        detail.extend(cost);
    }
    if c.duos_only {
        detail.push("Duos only".to_string());
    }
    let tier = if matches!(c.kind, Kind::Minion | Kind::TavernSpell) && category != Category::Hero { c.tier } else { 0 };
    SearchRow { card_id: c.id.clone(), name: c.name.clone(), detail: detail.join(" · "), color, tier }
}

/// `~/.config/hsbg-overlay/config.toml` reads better in a panel than the
/// full path.
fn short_home(path: &Path) -> String {
    match dirs::home_dir().as_deref().and_then(|home| path.strip_prefix(home).ok()) {
        Some(rest) => format!("~/{}", rest.display()),
        None => path.display().to_string(),
    }
}

// ---- X11 application -----------------------------------------------------

struct Panel<M> {
    win: OverlayWindow,
    last: Option<(M, Rect)>,
}

impl<M: PartialEq + Clone> Panel<M> {
    fn new(x: &X11, name: &str, interactive: bool) -> Result<Panel<M>> {
        Ok(Panel { win: x.create_overlay(name, interactive)?, last: None })
    }

    fn model(&self) -> Option<&M> {
        self.last.as_ref().map(|(m, _)| m)
    }

    fn update(&mut self, x: &X11, rect: Rect, model: &M, ctx: &mut Ctx, render: fn(&M, &mut Canvas, &mut Ctx)) {
        let changed = self.last.as_ref().is_none_or(|(m, r)| m != model || *r != rect);
        if !changed && self.win.mapped {
            return;
        }
        self.win.set_rect(x, rect);
        self.win.show(x);
        if changed {
            let mut c = Canvas::new(rect.w, rect.h);
            render(model, &mut c, ctx);
            self.win.present(x, c.rgba());
            self.last = Some((model.clone(), rect));
        } else {
            self.win.repaint(x);
        }
    }

    fn hide(&mut self, x: &X11) {
        self.win.hide(x);
    }

    /// Forces the next update to repaint, e.g. after a theme switch.
    fn invalidate(&mut self) {
        self.last = None;
    }
}

struct Panels {
    info: Panel<InfoPanel>,
    sim: Panel<SimPanel>,
    top: Panel<TopBar>,
    tiers: Panel<TierList>,
    comps: Panel<CompPanel>,
    search: Panel<SearchPanel>,
    settings: Panel<SettingsPanel>,
    preview: Panel<CardPreview>,
    popup: Panel<OpponentPopup>,
    calib: Panel<Calibration>,
}

impl Panels {
    fn windows(&self) -> [&OverlayWindow; 10] {
        [
            &self.info.win,
            &self.sim.win,
            &self.top.win,
            &self.tiers.win,
            &self.comps.win,
            &self.search.win,
            &self.settings.win,
            &self.calib.win,
            &self.preview.win,
            &self.popup.win,
        ]
    }

    fn hide_all(&mut self, x: &X11) {
        self.info.hide(x);
        self.sim.hide(x);
        self.top.hide(x);
        self.tiers.hide(x);
        self.comps.hide(x);
        self.search.hide(x);
        self.settings.hide(x);
        self.preview.hide(x);
        self.popup.hide(x);
        self.calib.hide(x);
    }

    /// Marks every panel dirty; the theme switch repaints the whole overlay.
    fn invalidate_all(&mut self) {
        self.info.invalidate();
        self.sim.invalidate();
        self.top.invalidate();
        self.tiers.invalidate();
        self.comps.invalidate();
        self.search.invalidate();
        self.settings.invalidate();
        self.preview.invalidate();
        self.popup.invalidate();
        self.calib.invalidate();
    }
}

pub struct App {
    ov: Overlay,
    x: X11,
    lobby_reader: Option<std::sync::mpsc::Receiver<MemoryEvent>>,
    tailer: LogTailer,
    sim: SimClient,
    panels: Panels,
    game_window: Option<GameWindow>,
    active: Option<u32>,
    calibrate: bool,
    /// Where the theme picked in the settings panel is written.
    config_path: PathBuf,
    /// The search field holds the keyboard.
    keyboard_grabbed: bool,
    need_raise: bool,
    /// Something changed since the last draw.
    dirty: bool,
    last_window_check: Option<Instant>,
    last_pointer: Option<Instant>,
    last_raise: Instant,
    last_in_game: Instant,
}

impl App {
    pub fn new(mut ov: Overlay, logs_dir: &Path, sim: SimClient, calibrate: bool) -> Result<App> {
        let x = X11::connect()?;
        if ov.cfg.show_comps {
            ov.comps.refresh();
        }
        // Search results then show their art straight from the disk cache
        // instead of downloading a strip per row while the player types.
        // Text mode draws no list art, so there is nothing to warm up.
        if ov.cfg.show_search && ov.cfg.compact_mode != CompactMode::Text {
            ov.images.warm_cache(ImageKind::Tile, ov.db.searchable());
        }
        log::info!("compositor {}", if x.compositing { "detected: translucent panels" } else { "not detected: opaque panels" });
        x.watch_root()?;
        let panels = Panels {
            info: Panel::new(&x, "hsbg-info", false)?,
            sim: Panel::new(&x, "hsbg-odds", false)?,
            top: Panel::new(&x, "hsbg-tiers-bar", true)?,
            tiers: Panel::new(&x, "hsbg-tiers", true)?,
            comps: Panel::new(&x, "hsbg-comps", true)?,
            search: Panel::new(&x, "hsbg-search", true)?,
            settings: Panel::new(&x, "hsbg-settings", true)?,
            preview: Panel::new(&x, "hsbg-preview", false)?,
            popup: Panel::new(&x, "hsbg-opponent", true)?,
            calib: Panel::new(&x, "hsbg-calibration", false)?,
        };
        let lobby_reader = ov.cfg.read_game_memory.then(crate::memory::spawn_reader);
        Ok(App {
            ov,
            x,
            lobby_reader,
            tailer: LogTailer::new(logs_dir.to_path_buf()),
            sim,
            panels,
            game_window: None,
            active: None,
            calibrate,
            config_path: crate::paths::config_path(),
            keyboard_grabbed: false,
            need_raise: true,
            dirty: true,
            last_window_check: None,
            last_pointer: None,
            last_raise: Instant::now(),
            last_in_game: Instant::now(),
        })
    }

    pub fn run(&mut self) -> Result<()> {
        loop {
            let started = Instant::now();
            self.read_log();
            for ev in self.sim.poll() {
                self.ov.on_sim_event(ev);
                self.dirty = true;
            }
            self.dirty |= self.ov.images.poll();
            self.dirty |= self.ov.comps.poll();
            self.poll_lobby_reader();
            self.track_game_window();
            self.handle_x_events();
            self.update_hover();
            if std::mem::take(&mut self.dirty) {
                self.draw();
            }
            self.sync_keyboard();
            self.raise_if_needed();
            self.x.flush();
            if self.ov.game.in_progress() {
                self.last_in_game = Instant::now();
            } else if self.sim.running() && self.last_in_game.elapsed() > SIM_IDLE {
                self.sim.stop();
            }
            if let Some(rest) = TICK.checked_sub(started.elapsed()) {
                std::thread::sleep(rest);
            }
        }
    }

    fn poll_lobby_reader(&mut self) {
        let Some(rx) = &self.lobby_reader else { return };
        while let Ok(ev) = rx.try_recv() {
            match ev {
                MemoryEvent::Lobby(lobby) => {
                    if let Some(l) = &lobby {
                        log::info!(
                            "lobby tribes: {}",
                            l.tribes.iter().map(|t| t.label()).collect::<Vec<_>>().join(", ")
                        );
                    }
                    self.ov.set_lobby_memory(lobby);
                    self.dirty = true;
                }
                MemoryEvent::Unavailable(_) => self.ov.set_lobby_memory(None),
            }
        }
    }

    fn read_log(&mut self) {
        let poll = self.tailer.poll();
        if poll.reset {
            self.ov.game.reset();
            let source = self
                .tailer
                .current_path()
                .and_then(|p| p.parent())
                .and_then(|p| p.file_name())
                .map(|n| n.to_string_lossy().into_owned())
                .unwrap_or_default();
            self.ov.game.set_source(&source);
        }
        if poll.lines.is_empty() {
            self.dirty |= poll.reset;
            return;
        }
        self.dirty = true;
        let mut to_simulate = Vec::new();
        for line in &poll.lines {
            self.ov.game.process(line);
            let events = self.ov.game.take_events();
            if !events.is_empty() {
                if events.iter().any(|e| matches!(e, GameEvent::GameStarted)) && !poll.catchup {
                    self.sim.warm_up();
                }
                to_simulate.extend(self.ov.handle_events(events, poll.catchup));
            }
        }
        if poll.catchup {
            to_simulate.extend(self.ov.combat_in_progress());
            if self.ov.game.in_progress() {
                self.sim.warm_up();
            }
        }
        if let Some(snap) = to_simulate.pop() {
            log::info!("simulating turn {} combat", snap.turn);
            let input = self.ov.sim_input(&snap);
            self.sim.request(input);
        }
    }

    fn track_game_window(&mut self) {
        let due = self.last_window_check.is_none_or(|t| t.elapsed() >= Duration::from_millis(250));
        if !due {
            return;
        }
        self.last_window_check = Some(Instant::now());
        let id = match self.game_window.map(|g| g.id) {
            Some(id) => Some(id),
            None => self.x.find_game_window(),
        };
        let gw = id.and_then(|id| self.x.game_window(id));
        if gw.map(|g| g.rect) != self.game_window.map(|g| g.rect) {
            self.need_raise = true;
        }
        if gw.is_some() && self.game_window.is_none() {
            log::info!("found the Hearthstone window");
        }
        let active = self.x.active_window();
        if gw != self.game_window || active != self.active {
            self.dirty = true;
        }
        self.game_window = gw;
        self.active = active;
    }

    fn is_ours(&self, w: u32) -> bool {
        self.panels.windows().iter().any(|p| p.id == w)
    }

    fn visible(&self) -> bool {
        let Some(gw) = self.game_window else { return false };
        if !gw.visible {
            return false;
        }
        if !(self.ov.game.in_progress() || self.calibrate) {
            return false;
        }
        if self.ov.cfg.hide_when_unfocused {
            if let Some(active) = self.active {
                if active != 0 && active != gw.id && !self.is_ours(active) {
                    return false;
                }
            }
        }
        true
    }

    fn layout(&self) -> Option<Layout> {
        self.game_window.map(|g| Layout::new(g.rect, &self.ov.cfg))
    }

    /// Applies a theme picked in the settings panel, live, and writes it to
    /// the config; a failure is shown in the panel instead.
    fn pick_theme(&mut self, i: usize) {
        let Some(name) = ThemeName::ALL.get(i).copied() else { return };
        if name != self.ov.theme_name {
            self.ov.set_theme(name);
            self.panels.invalidate_all();
        }
        self.ov.ui.settings_error = None;
        if let Err(e) = config::save_theme(&self.config_path, name) {
            log::warn!("could not save the theme: {e:#}");
            self.ov.ui.settings_error = Some(e.to_string());
        }
        self.dirty = true;
    }

    /// Applies a compact mode picked in the settings panel, live, and writes
    /// it to the config; a failure is shown in the panel instead.
    fn pick_compact_mode(&mut self, i: usize) {
        let Some(mode) = CompactMode::ALL.get(i).copied() else { return };
        if mode != self.ov.cfg.compact_mode {
            self.ov.cfg.compact_mode = mode;
            self.panels.invalidate_all();
        }
        self.ov.ui.settings_error = None;
        if let Err(e) = config::save_compact_mode(&self.config_path, mode) {
            log::warn!("could not save the compact mode: {e:#}");
            self.ov.ui.settings_error = Some(e.to_string());
        }
        self.dirty = true;
    }

    /// Applies a layout size picked in the settings panel, live, and writes
    /// it to the config; a failure is shown in the panel instead.
    fn pick_layout_size(&mut self, i: usize) {
        let Some(size) = LayoutSize::ALL.get(i).copied() else { return };
        if size.scale() != self.ov.cfg.scale {
            self.ov.cfg.scale = size.scale();
            self.panels.invalidate_all();
        }
        self.ov.ui.settings_error = None;
        if let Err(e) = config::save_scale(&self.config_path, size) {
            log::warn!("could not save the layout size: {e:#}");
            self.ov.ui.settings_error = Some(e.to_string());
        }
        self.dirty = true;
    }

    fn handle_x_events(&mut self) {
        let s = self.layout().map(|l| l.s).unwrap_or(1.0);
        while let Some(ev) = self.x.poll_event() {
            if !matches!(ev, Event::Expose(_)) {
                self.dirty = true;
            }
            match ev {
                Event::Expose(e) if e.count == 0 => {
                    if let Some(w) = self.panels.windows().into_iter().find(|w| w.id == e.window) {
                        w.repaint(&self.x);
                    }
                }
                Event::ButtonPress(e) if e.event == self.panels.top.win.id && e.detail == 1 => {
                    let hit = self.panels.top.model().and_then(|m| m.hit(e.event_x as f32, s));
                    let ui = &mut self.ov.ui;
                    match hit {
                        Some(TopHit::Tier(t)) => {
                            let open = ui.selected_tier != Some(t);
                            ui.close_panels();
                            ui.selected_tier = open.then_some(t);
                            ui.scroll = 0.0;
                            ui.list_hover = None;
                        }
                        Some(TopHit::Comps) => {
                            let open = !ui.comps_open;
                            ui.close_panels();
                            ui.comps_open = open;
                            ui.comp_hover = None;
                            if ui.comps_open {
                                self.ov.comps.refresh();
                            }
                        }
                        Some(TopHit::Search) => {
                            let open = !ui.search_open;
                            ui.close_panels();
                            // Ready to type as soon as it opens.
                            ui.search_open = open;
                            ui.search_focus = open;
                            ui.search_hover = None;
                        }
                        Some(TopHit::Settings) => {
                            let open = !ui.settings_open;
                            ui.close_panels();
                            ui.settings_open = open;
                            ui.settings_hover = None;
                        }
                        None => {}
                    }
                }
                Event::ButtonPress(e) if e.event == self.panels.comps.win.id && (e.detail == 4 || e.detail == 5) => {
                    let step = 27.0 * 3.0 * s * if e.detail == 4 { -1.0 } else { 1.0 };
                    if let Some(m) = self.panels.comps.model() {
                        let max = m.max_scroll(s, self.panels.comps.win.rect.h);
                        self.ov.ui.comp_scroll = (self.ov.ui.comp_scroll + step).clamp(0.0, max);
                        self.ov.ui.comp_hover = m.hit(e.event_x as f32, e.event_y as f32, s);
                    }
                }
                Event::ButtonPress(e) if e.event == self.panels.comps.win.id && (e.detail == 1 || e.detail == 3) => {
                    let model = self.panels.comps.model();
                    let hit = model.and_then(|m| m.hit(e.event_x as f32, e.event_y as f32, s));
                    let clicked = match (model, hit) {
                        (Some(m), Some(CompHit::Item(i))) => match m.item(i) {
                            Some(CompItem::Comp(row)) => Some(row.id),
                            _ => None,
                        },
                        _ => None,
                    };
                    let ui = &mut self.ov.ui;
                    match (e.detail, hit) {
                        // Right click: back to the tier list, or close.
                        (3, _) if ui.comp_id.is_some() => {
                            ui.comp_id = None;
                            ui.comp_scroll = ui.comp_list_scroll;
                        }
                        (3, _) => ui.comps_open = false,
                        (_, Some(CompHit::LobbyToggle)) => {
                            ui.hide_unavailable = !ui.hide_unavailable;
                            ui.comp_scroll = 0.0;
                        }
                        (_, Some(CompHit::Back)) => {
                            ui.comp_id = None;
                            ui.comp_scroll = ui.comp_list_scroll;
                        }
                        _ => {
                            if let Some(id) = clicked {
                                ui.comp_id = Some(id);
                                ui.comp_list_scroll = ui.comp_scroll;
                                ui.comp_scroll = 0.0;
                            }
                        }
                    }
                    ui.comp_hover = None;
                }
                Event::ButtonPress(e) if e.event == self.panels.search.win.id && (e.detail == 4 || e.detail == 5) => {
                    let step = 36.0 * 3.0 * s * if e.detail == 4 { -1.0 } else { 1.0 };
                    if let Some(m) = self.panels.search.model() {
                        let max = m.max_scroll(s, self.panels.search.win.rect.h);
                        self.ov.ui.search_scroll = (self.ov.ui.search_scroll + step).clamp(0.0, max);
                        self.ov.ui.search_hover = m.hit(e.event_x as f32, e.event_y as f32, s);
                    }
                }
                Event::ButtonPress(e) if e.event == self.panels.search.win.id && e.detail == 3 => {
                    self.ov.ui.close_panels();
                }
                Event::ButtonPress(e) if e.event == self.panels.search.win.id && e.detail == 1 => {
                    let hit = self.panels.search.model().and_then(|m| m.hit(e.event_x as f32, e.event_y as f32, s));
                    let ui = &mut self.ov.ui;
                    match hit {
                        Some(SearchHit::Field) => ui.search_focus = true,
                        Some(SearchHit::Clear) => {
                            ui.search_query.clear();
                            ui.search_scroll = 0.0;
                            ui.search_focus = true;
                        }
                        Some(SearchHit::Category(k)) => {
                            ui.search_category = k;
                            ui.search_scroll = 0.0;
                            ui.search_hover = None;
                        }
                        Some(SearchHit::Row(_)) | None => {}
                    }
                }
                Event::ButtonPress(e) if e.event == self.panels.settings.win.id && e.detail == 3 => {
                    self.ov.ui.settings_open = false;
                    self.ov.ui.settings_hover = None;
                }
                Event::ButtonPress(e) if e.event == self.panels.settings.win.id && e.detail == 1 => {
                    let hit = self.panels.settings.model().and_then(|m| m.hit(e.event_x as f32, e.event_y as f32, s));
                    match hit {
                        Some(SettingsHit::Theme(i)) => self.pick_theme(i),
                        Some(SettingsHit::Compact(i)) => self.pick_compact_mode(i),
                        Some(SettingsHit::Layout(i)) => self.pick_layout_size(i),
                        None => {}
                    }
                    self.ov.ui.settings_hover = hit;
                }
                Event::MotionNotify(e) if e.event == self.panels.settings.win.id => {
                    self.ov.ui.settings_hover =
                        self.panels.settings.model().and_then(|m| m.hit(e.event_x as f32, e.event_y as f32, s));
                }
                Event::LeaveNotify(e) if e.event == self.panels.settings.win.id => self.ov.ui.settings_hover = None,
                Event::KeyPress(e) if self.keyboard_grabbed => {
                    if let Some(key) = self.x.key(e.detail, e.state) {
                        let rows = self.panels.search.model().map(|m| m.rows.len()).unwrap_or(0);
                        self.ov.ui.search_key(key, rows);
                        // Arrow keys: keep the picked row in view.
                        if let (Some(i), Some(m)) = (self.ov.ui.search_row_hover(), self.panels.search.model()) {
                            if matches!(key, Key::Up | Key::Down | Key::PageUp | Key::PageDown) {
                                self.ov.ui.search_scroll = m.scroll_to(i, s, self.panels.search.win.rect.h);
                            }
                        }
                    }
                }
                Event::MappingNotify(_) => self.x.reload_keymap(),
                Event::ButtonPress(e) if e.event == self.panels.popup.win.id && e.detail == 1 => {
                    let model = self.panels.popup.model();
                    let hit = model.and_then(|m| Some((m.lobby_id, m.boards.get(m.hit(e.event_x as f32, e.event_y as f32, s)?)?.turn)));
                    if let Some((lobby_id, turn)) = hit {
                        let flips = self.ov.ui.board_flips.entry(lobby_id).or_default();
                        if !flips.remove(&turn) {
                            flips.insert(turn);
                        }
                    }
                }
                Event::MotionNotify(e) if e.event == self.panels.popup.win.id => {
                    self.ov.ui.popup_hover = self.panels.popup.model().and_then(|m| m.hit(e.event_x as f32, e.event_y as f32, s));
                }
                Event::LeaveNotify(e) if e.event == self.panels.popup.win.id => self.ov.ui.popup_hover = None,
                Event::ButtonPress(e) if e.event == self.panels.tiers.win.id && (e.detail == 4 || e.detail == 5) => {
                    let step = 27.0 * 3.0 * s * if e.detail == 4 { -1.0 } else { 1.0 };
                    if let Some(m) = self.panels.tiers.model() {
                        let max = m.max_scroll(s, self.panels.tiers.win.rect.h);
                        self.ov.ui.scroll = (self.ov.ui.scroll + step).clamp(0.0, max);
                        self.ov.ui.list_hover = m.hit(e.event_x as f32, e.event_y as f32, s);
                    }
                }
                Event::ButtonPress(e) if e.event == self.panels.tiers.win.id && e.detail == 3 => {
                    self.ov.ui.selected_tier = None;
                }
                Event::ButtonPress(e) if e.event == self.panels.tiers.win.id && e.detail == 1 => {
                    let hit = self.panels.tiers.model().and_then(|m| m.hit(e.event_x as f32, e.event_y as f32, s));
                    let ui = &mut self.ov.ui;
                    match hit {
                        Some(TierHit::LobbyToggle) => {
                            ui.hide_unavailable = !ui.hide_unavailable;
                            ui.scroll = 0.0;
                        }
                        Some(TierHit::FilterBar) => ui.filter_open = !ui.filter_open,
                        Some(TierHit::FilterClear) => {
                            ui.keyword = None;
                            ui.scroll = 0.0;
                        }
                        Some(TierHit::Keyword(k)) => {
                            ui.keyword = k;
                            ui.filter_open = false;
                            ui.scroll = 0.0;
                        }
                        Some(TierHit::Row(_)) | None => {}
                    }
                    ui.list_hover = None;
                }
                Event::MotionNotify(e) if e.event == self.panels.top.win.id => {
                    self.ov.ui.top_hover = self.panels.top.model().and_then(|m| m.hit(e.event_x as f32, s));
                }
                Event::MotionNotify(e) if e.event == self.panels.comps.win.id => {
                    self.ov.ui.comp_hover = self.panels.comps.model().and_then(|m| {
                        let hit = m.hit(e.event_x as f32, e.event_y as f32, s)?;
                        match hit {
                            CompHit::Item(i) => m.item(i)?.hoverable().then_some(hit),
                            _ => Some(hit),
                        }
                    });
                }
                Event::MotionNotify(e) if e.event == self.panels.search.win.id => {
                    self.ov.ui.search_hover =
                        self.panels.search.model().and_then(|m| m.hit(e.event_x as f32, e.event_y as f32, s));
                }
                Event::MotionNotify(e) if e.event == self.panels.tiers.win.id => {
                    self.ov.ui.list_hover =
                        self.panels.tiers.model().and_then(|m| m.hit(e.event_x as f32, e.event_y as f32, s));
                }
                Event::LeaveNotify(e) if e.event == self.panels.top.win.id => self.ov.ui.top_hover = None,
                Event::LeaveNotify(e) if e.event == self.panels.comps.win.id => self.ov.ui.comp_hover = None,
                Event::LeaveNotify(e) if e.event == self.panels.tiers.win.id => self.ov.ui.list_hover = None,
                Event::LeaveNotify(e) if e.event == self.panels.search.win.id => self.ov.ui.search_hover = None,
                Event::ConfigureNotify(e) if !self.is_ours(e.window) => self.need_raise = true,
                Event::MapNotify(e) if !self.is_ours(e.window) => self.need_raise = true,
                _ => {}
            }
        }
    }

    fn update_hover(&mut self) {
        if self.last_pointer.is_some_and(|t| t.elapsed() < Duration::from_millis(50)) {
            return;
        }
        self.last_pointer = Some(Instant::now());
        let pointer = match (self.layout(), self.visible()) {
            (Some(l), true) => self.x.pointer().map(|p| (l, p)),
            _ => None,
        };
        let hover = pointer.and_then(|(l, (px, py, _))| l.slot_at(px, py).or_else(|| self.popup_held(&l, px, py)));
        // A click outside the overlay gives the keyboard back to the game
        // (clicks on the panels arrive as events).
        if let Some((_, (px, py, true))) = pointer {
            let on_panel = self.panels.windows().iter().any(|w| w.mapped && w.rect.contains(px, py));
            if self.ov.ui.search_focus && !on_panel {
                self.ov.ui.search_focus = false;
                self.dirty = true;
            }
        }
        if hover != self.ov.ui.slot_hover {
            self.ov.ui.slot_hover = hover;
            self.ov.ui.popup_hover = None;
            self.dirty = true;
        }
    }

    /// The open popup's slot while the pointer is on the popup or crossing
    /// the gap to it, so its turn chips can be clicked.
    fn popup_held(&self, l: &Layout, px: i32, py: i32) -> Option<usize> {
        let slot = self.ov.ui.slot_hover?;
        let win = &self.panels.popup.win;
        if !win.mapped {
            return None;
        }
        if win.rect.contains(px, py) {
            return Some(slot);
        }
        let r = l.slot(slot);
        let x1 = r.x + r.w;
        let x2 = win.rect.x;
        if x2 > x1 && px >= x1 && px <= x2 {
            let t = (px - x1) as f32 / (x2 - x1) as f32;
            let top = r.y as f32 + t * (win.rect.y as f32 - r.y as f32);
            let bot = (r.y + r.h) as f32 + t * ((win.rect.y + win.rect.h) as f32 - (r.y + r.h) as f32);
            let margin = l.px(24.0) as f32;
            let min_y = (top.min(bot) - margin).min(win.rect.y as f32);
            let max_y = top.max(bot) + margin;
            if (py as f32) >= min_y && (py as f32) <= max_y {
                return Some(slot);
            }
        }
        None
    }

    /// Grabs the keyboard while the search field is focused and on screen,
    /// and gives it back otherwise.
    fn sync_keyboard(&mut self) {
        let ui = &mut self.ov.ui;
        let mapped = self.panels.search.win.mapped;
        // Hidden (game unfocused or gone): stay unfocused when it returns.
        if self.keyboard_grabbed && !mapped {
            ui.search_focus = false;
        }
        let want = ui.search_open && ui.search_focus && mapped;
        if want == self.keyboard_grabbed {
            return;
        }
        if want {
            self.keyboard_grabbed = self.x.grab_keyboard(self.panels.search.win.id);
            if !self.keyboard_grabbed {
                log::warn!("could not grab the keyboard for the search field");
                ui.search_focus = false;
                self.dirty = true;
            }
        } else {
            self.x.ungrab_keyboard();
            self.keyboard_grabbed = false;
        }
    }

    fn draw(&mut self) {
        let (Some(layout), true) = (self.layout(), self.visible()) else {
            self.panels.hide_all(&self.x);
            return;
        };
        let s = layout.s;
        let in_game = self.ov.game.in_progress();
        let cfg = self.ov.cfg.clone();

        let info = self.ov.info_panel(layout.info_width());
        let sim = self.ov.sim_panel();
        let top = self.ov.top_bar();
        let tiers = self.ov.tier_list();
        let popup = self.ov.ui.slot_hover.and_then(|slot| self.ov.popup(slot).map(|p| (slot, p)));
        let comps = self.ov.comp_panel();
        let search = self.ov.search_panel();
        let settings = self.ov.ui.settings_open.then(|| self.ov.settings_panel(&self.config_path));
        // Card under the pointer, with the item it belongs to.
        let preview = tiers
            .as_ref()
            .and_then(|t| self.ov.preview(t))
            .map(|pv| (pv, self.ov.ui.row_hover().unwrap_or(0)))
            .or_else(|| comps.as_ref().and_then(|c| self.ov.comp_preview(c)).map(|pv| (pv, self.ov.ui.comp_item_hover().unwrap_or(0))))
            .or_else(|| {
                let pv = search.as_ref().and_then(|p| self.ov.search_preview(p))?;
                Some((pv, self.ov.ui.search_row_hover().unwrap_or(0)))
            });
        // Hover previews then only need a decode, not a download. Text mode
        // shows no previews at all, so it downloads no renders either.
        if self.ov.cfg.compact_mode != CompactMode::Text {
            if let Some(list) = &tiers {
                for row in list.sections.iter().flat_map(|sec| &sec.rows) {
                    self.ov.images.prefetch(ImageKind::Render, &row.card_id);
                }
            }
            if let Some(panel) = &comps {
                for item in &panel.items {
                    if let CompItem::Card { card_id, .. } = item {
                        self.ov.images.prefetch(ImageKind::Render, card_id);
                    }
                }
            }
            // Only the best matches: the results change with every key.
            if let Some(panel) = &search {
                for row in panel.rows.iter().take(8) {
                    self.ov.images.prefetch(ImageKind::Render, &row.card_id);
                }
            }
        }

        let opaque = !self.x.compositing;
        let mut ctx =
            Ctx { fonts: &mut self.ov.fonts, images: &mut self.ov.images, theme: self.ov.theme, compact: self.ov.cfg.compact_mode, s, opaque };
        let x = &self.x;
        let p = &mut self.panels;

        if in_game && cfg.show_session {
            let rect = layout.info(info.size(s));
            p.info.update(x, rect, &info, &mut ctx, |m, c, ctx| m.render(c, ctx));
        } else {
            p.info.hide(x);
        }
        let top_rect = layout.top_bar(top.size(s));
        // The settings button lives in the bar, so it shows whenever a game does.
        let show_top = in_game;
        let sim_rect = layout.sim(sim.size(s), show_top.then_some(top_rect));
        if in_game && cfg.show_simulation && self.ov.game.turn() >= 1 {
            p.sim.update(x, sim_rect, &sim, &mut ctx, |m, c, ctx| m.render(c, ctx));
        } else {
            p.sim.hide(x);
        }
        // Window of the open list and the centre of the hovered item in it.
        let mut hovered_at: Option<(Rect, f32)> = None;
        if show_top {
            p.top.update(x, top_rect, &top, &mut ctx, |m, c, ctx| m.render(c, ctx));
        } else {
            p.top.hide(x);
        }
        match &tiers {
            Some(list) if show_top && cfg.show_tiers => {
                let (tx, ty, max_h) = layout.tier_list(top_rect, (TierList::W * s).ceil() as i32);
                let (w, h) = list.size(s, max_h);
                let rect = Rect::new(tx, ty, w, h);
                p.tiers.update(x, rect, list, &mut ctx, |m, c, ctx| m.render(c, ctx));
                hovered_at = preview.as_ref().map(|(_, i)| (rect, list.row_center(*i, s).unwrap_or(0.0)));
            }
            _ => p.tiers.hide(x),
        }
        match &comps {
            Some(panel) if show_top && cfg.show_comps => {
                let (tx, ty, max_h) = layout.tier_list(top_rect, (CompPanel::W * s).ceil() as i32);
                let (w, h) = panel.size(s, max_h);
                let rect = Rect::new(tx, ty, w, h);
                p.comps.update(x, rect, panel, &mut ctx, |m, c, ctx| m.render(c, ctx));
                hovered_at = hovered_at.or_else(|| preview.as_ref().map(|(_, i)| (rect, panel.item_center(*i, s).unwrap_or(0.0))));
            }
            _ => p.comps.hide(x),
        }
        match &search {
            Some(panel) if show_top && cfg.show_search => {
                let (tx, ty, max_h) = layout.tier_list(top_rect, (SearchPanel::W * s).ceil() as i32);
                let (w, h) = panel.size(s, max_h);
                let rect = Rect::new(tx, ty, w, h);
                p.search.update(x, rect, panel, &mut ctx, |m, c, ctx| m.render(c, ctx));
                hovered_at = hovered_at.or_else(|| preview.as_ref().map(|(_, i)| (rect, panel.row_center(*i, s).unwrap_or(0.0))));
            }
            _ => p.search.hide(x),
        }
        match &settings {
            Some(panel) if show_top => {
                let (tx, ty, max_h) = layout.tier_list(top_rect, (SettingsPanel::W * s).ceil() as i32);
                let (w, h) = panel.size(s, max_h);
                let rect = Rect::new(tx, ty, w, h);
                p.settings.update(x, rect, panel, &mut ctx, |m, c, ctx| m.render(c, ctx));
            }
            _ => p.settings.hide(x),
        }
        let preview_rect = match (&preview, hovered_at) {
            (Some((pv, _)), Some((pr, center))) => pv.size(s, ctx.images).map(|(w, h)| {
                let cy = pr.y + center as i32;
                let y = (cy - h / 2).clamp(layout.hs.y, (layout.hs.y + layout.hs.h - h).max(layout.hs.y));
                Rect::new(pr.x - w - (8.0 * s) as i32, y, w, h)
            }),
            _ => None,
        };
        match (preview, preview_rect) {
            (Some((pv, _)), Some(rect)) => p.preview.update(x, rect, &pv, &mut ctx, |m, c, ctx| m.render(c, ctx)),
            _ => p.preview.hide(x),
        }
        match popup {
            Some((_slot, mut pm)) if in_game && cfg.show_opponent_boards => {
                pm.fit(s, layout.popup_max_h(sim_rect));
                let rect = layout.popup(pm.size(s), sim_rect);
                p.popup.update(x, rect, &pm, &mut ctx, |m, c, ctx| m.render(c, ctx));
            }
            _ => p.popup.hide(x),
        }
        if self.calibrate {
            let first = layout.slot(0);
            let last = layout.slot(7);
            let area = Rect::new(first.x - 4, first.y - 4, first.w + 8, last.y + last.h - first.y + 8);
            let slots = (0..8)
                .map(|i| {
                    let r = layout.slot(i);
                    ((r.x - area.x) as f32, (r.y - area.y) as f32, r.w as f32, r.h as f32)
                })
                .collect();
            let model = Calibration { slots, hover: self.ov.ui.slot_hover };
            p.calib.update(x, area, &model, &mut ctx, |m, c, ctx| m.render(c, ctx));
        } else {
            p.calib.hide(x);
        }
    }

    /// Keeps the panels above the game: the window manager restacks the game
    /// window when it is focused or toggled fullscreen.
    fn raise_if_needed(&mut self) {
        if self.need_raise || self.last_raise.elapsed() >= Duration::from_secs(2) {
            for w in self.panels.windows() {
                w.raise(&self.x);
            }
            self.need_raise = false;
            self.last_raise = Instant::now();
        }
    }
}

// ---- offline rendering (debugging aid) -----------------------------------

/// Replays a log up to the `combat`-th fight and writes every panel, plus a
/// composed 1920×1080 mock-up, as PNG files.
pub fn render_offline(mut ov: Overlay, log: &Path, out: &Path, combat: usize) -> Result<()> {
    let text = std::fs::read_to_string(log)?;
    ov.game.set_source("render");
    let mut seen = 0;
    for line in text.lines() {
        ov.game.process(line);
        let events = ov.game.take_events();
        let combats = events.iter().filter(|e| matches!(e, GameEvent::Combat(_))).count();
        ov.handle_events(events, true);
        seen += combats;
        if seen >= combat {
            break;
        }
    }
    ov.sim_view = SimView {
        turn: ov.game.turn(),
        odds: Some(Odds {
            won: 61.8,
            tied: 9.4,
            lost: 28.8,
            won_lethal: 0.0,
            lost_lethal: 0.6,
            average_damage_won: 7.0,
            average_damage_lost: 5.0,
            simulations: 8000,
        }),
        running: false,
        error: None,
    };
    ov.ui.selected_tier = Some(ov.game.tavern_tier().max(1));
    ov.ui.list_hover = Some(TierHit::Row(1));
    // The opponent faced most often, to show the stacked boards.
    let opponent = ov
        .game
        .leaderboard()
        .into_iter()
        .filter(|p| !p.is_local && ov.game.seen_board(p.lobby_id).is_some())
        .max_by_key(|p| ov.game.seen_boards(p.lobby_id).len());
    ov.ui.slot_hover = opponent.as_ref().map(|p| (p.place - 1) as usize);
    // The two newest boards toggled on, to show the chips and the stack.
    if let Some(p) = &opponent {
        let seen = ov.game.seen_boards(p.lobby_id);
        if let Some(b) = seen.len().checked_sub(2).map(|i| &seen[i]) {
            ov.ui.board_flips.entry(p.lobby_id).or_default().insert(b.turn);
        }
    }

    let hs = Rect::new(0, 0, 1920, 1080);
    let layout = Layout::new(hs, &ov.cfg);
    let s = layout.s;
    // Text mode draws no card art, so there is nothing to download.
    let art = ov.cfg.compact_mode != CompactMode::Text;
    // Let image downloads finish.
    let deadline = Instant::now() + Duration::from_secs(20);
    loop {
        let list = ov.tier_list();
        let popup = ov.ui.slot_hover.and_then(|i| ov.popup(i));
        let mut pending = 0;
        let mut ids: Vec<(ImageKind, String)> = Vec::new();
        if let Some(l) = &list {
            if art {
                for sec in &l.sections {
                    ids.extend(sec.rows.iter().map(|r| (ImageKind::Tile, r.card_id.clone())));
                }
            }
            if let Some(pv) = ov.preview(l) {
                ids.push((ImageKind::Render, pv.card_id));
            }
        }
        if art {
            if let Some(p) = &popup {
                ids.push((ImageKind::Tile, p.hero_card_id.clone()));
                ids.extend(p.boards.iter().flat_map(|b| &b.minions).map(|m| (ImageKind::Portrait, m.card_id.clone())));
            }
        }
        for (kind, id) in &ids {
            if ov.images.get(*kind, id).is_none() {
                pending += 1;
            }
        }
        ov.images.poll();
        if pending == 0 || Instant::now() > deadline {
            if pending > 0 {
                log::warn!("{pending} images still missing");
            }
            break;
        }
        std::thread::sleep(Duration::from_millis(100));
    }

    std::fs::create_dir_all(out)?;
    let mut screen = Canvas::new(hs.w, hs.h);
    screen.hgradient(0.0, 0.0, hs.w as f32, hs.h as f32, crate::ui::draw::Rgba(70, 40, 110, 255), crate::ui::draw::Rgba(40, 60, 120, 255));
    for i in 0..8 {
        let r = layout.slot(i);
        screen.stroke_rrect(r.x as f32, r.y as f32, r.w as f32, r.h as f32, 6.0, 2.0, crate::ui::draw::Rgba(255, 255, 255, 120));
    }
    let info = ov.info_panel(layout.info_width());
    let sim = ov.sim_panel();
    let top = ov.top_bar();
    let list = ov.tier_list();
    let popup = ov.ui.slot_hover.and_then(|i| ov.popup(i));
    let preview = list.as_ref().and_then(|l| ov.preview(l));
    // Tier list variants: keyword grid open, and a keyword applied.
    let mut variants = Vec::new();
    ov.ui.filter_open = true;
    variants.extend(ov.tier_list().map(|l| ("tiers-filter", l)));
    ov.ui.filter_open = false;
    ov.ui.keyword = crate::cards::KEYWORDS.iter().position(|k| k.label == "Deathrattle");
    variants.extend(ov.tier_list().map(|l| ("tiers-deathrattle", l)));
    // Comps: the tier list and the first comp's guide.
    let mut comp_panels = Vec::new();
    match crate::comps::load_or_fetch() {
        Ok(guides) => {
            let first = guides.first().map(|g| g.id);
            let archetypes = crate::enablers::load_or_fetch().unwrap_or_else(|e| {
                log::warn!("{e:#}");
                Vec::new()
            });
            ov.comps = Comps::with_guides(guides, archetypes);
            ov.ui.comps_open = true;
            ov.ui.comp_hover = Some(CompHit::Item(1));
            comp_panels.extend(ov.comp_panel().map(|p| ("comps", p)));
            ov.ui.comp_id = first;
            ov.ui.comp_hover = None;
            comp_panels.extend(ov.comp_panel().map(|p| ("comp-guide", p)));
            ov.ui.comps_open = false;
        }
        Err(e) => log::warn!("{e:#}"),
    }
    // Search: a typed name with its best match highlighted, and a browsed type.
    let mut search_panels = Vec::new();
    ov.ui.search_open = true;
    ov.ui.search_focus = true;
    ov.ui.search_query = "brann bronz".into();
    ov.ui.search_hover = Some(SearchHit::Row(0));
    search_panels.extend(ov.search_panel().map(|p| ("search", p)));
    ov.ui.search_query = "rivnedare".into();
    search_panels.extend(ov.search_panel().map(|p| ("search-typo", p)));
    ov.ui.search_query.clear();
    ov.ui.search_focus = false;
    ov.ui.search_hover = None;
    ov.ui.search_category = Some(Category::Hero);
    search_panels.extend(ov.search_panel().map(|p| ("search-heroes", p)));
    ov.ui.search_open = false;
    // Settings: the compact mode list, second row under the pointer.
    ov.ui.settings_open = true;
    ov.ui.settings_hover = Some(SettingsHit::Compact(1));
    let settings = ov.settings_panel(&crate::paths::config_path());
    ov.ui.settings_open = false;
    ov.ui.settings_hover = None;
    let ids: Vec<String> = comp_panels
        .iter()
        .flat_map(|(_, p)| &p.items)
        .filter_map(|i| match i {
            CompItem::Comp(r) => Some(r.card_id.clone()),
            CompItem::Card { card_id, .. } | CompItem::Hero { card_id, .. } => Some(card_id.clone()),
            _ => None,
        })
        .chain(search_panels.iter().flat_map(|(_, p)| p.rows.iter().take(25).map(|r| r.card_id.clone())))
        .collect();
    let deadline = Instant::now() + Duration::from_secs(20);
    while art
        && ids.iter().filter(|id| ov.images.get(ImageKind::Tile, id).is_none()).count() > 0
        && Instant::now() < deadline
    {
        std::thread::sleep(Duration::from_millis(100));
        ov.images.poll();
    }
    let mut ctx =
        Ctx { fonts: &mut ov.fonts, images: &mut ov.images, theme: ov.theme, compact: ov.cfg.compact_mode, s, opaque: false };
    for (name, l) in &variants {
        let (w, h) = l.size(s, (hs.h as f32 * 0.85) as i32);
        let mut scratch = Canvas::new(w, h);
        emit(&mut scratch, &mut ctx, out, name, Rect::new(0, 0, w, h), &|c, ctx| l.render(c, ctx))?;
    }
    for (name, p) in &comp_panels {
        let (w, h) = p.size(s, (hs.h as f32 * 0.85) as i32);
        let mut scratch = Canvas::new(w, h);
        emit(&mut scratch, &mut ctx, out, name, Rect::new(0, 0, w, h), &|c, ctx| p.render(c, ctx))?;
    }
    for (name, p) in &search_panels {
        let (w, h) = p.size(s, (hs.h as f32 * 0.85) as i32);
        let mut scratch = Canvas::new(w, h);
        emit(&mut scratch, &mut ctx, out, name, Rect::new(0, 0, w, h), &|c, ctx| p.render(c, ctx))?;
    }
    let (w, h) = settings.size(s, (hs.h as f32 * 0.85) as i32);
    let mut scratch = Canvas::new(w, h);
    emit(&mut scratch, &mut ctx, out, "settings", Rect::new(0, 0, w, h), &|c, ctx| settings.render(c, ctx))?;
    let sc = &mut screen;
    emit(sc, &mut ctx, out, "info", layout.info(info.size(s)), &|c, ctx| info.render(c, ctx))?;
    let top_rect = layout.top_bar(top.size(s));
    let sim_rect = layout.sim(sim.size(s), Some(top_rect));
    emit(sc, &mut ctx, out, "sim", sim_rect, &|c, ctx| sim.render(c, ctx))?;
    emit(sc, &mut ctx, out, "top", top_rect, &|c, ctx| top.render(c, ctx))?;
    if let Some(l) = &list {
        let (tx, ty, max_h) = layout.tier_list(top_rect, (TierList::W * s).ceil() as i32);
        let (w, h) = l.size(s, max_h);
        let tr = Rect::new(tx, ty, w, h);
        emit(sc, &mut ctx, out, "tiers", tr, &|c, ctx| l.render(c, ctx))?;
        if let Some(pv) = &preview {
            if let Some((w, h)) = pv.size(s, ctx.images) {
                let cy = tr.y + l.row_center(1, s).unwrap_or(0.0) as i32;
                let r = Rect::new(tr.x - w - 8, (cy - h / 2).max(0), w, h);
                emit(sc, &mut ctx, out, "preview", r, &|c, ctx| pv.render(c, ctx))?;
            }
        }
    }
    if let (Some(mut p), Some(_slot)) = (popup, ov.ui.slot_hover) {
        p.fit(s, layout.popup_max_h(sim_rect));
        emit(sc, &mut ctx, out, "popup", layout.popup(p.size(s), sim_rect), &|c, ctx| p.render(c, ctx))?;
    }
    screen.pixmap.save_png(out.join("screen.png"))?;
    Ok(())
}

fn emit(
    screen: &mut Canvas,
    ctx: &mut Ctx,
    out: &Path,
    name: &str,
    rect: Rect,
    draw: &dyn Fn(&mut Canvas, &mut Ctx),
) -> Result<()> {
    let mut c = Canvas::new(rect.w, rect.h);
    draw(&mut c, ctx);
    c.pixmap.save_png(out.join(format!("{name}.png")))?;
    screen.pixmap.draw_pixmap(
        rect.x,
        rect.y,
        c.pixmap.as_ref(),
        &tiny_skia::PixmapPaint::default(),
        tiny_skia::Transform::identity(),
        None,
    );
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    const P: &str = "D 00:00:00.0000000 PowerTaskList.DebugPrintPower() - ";

    fn overlay() -> Overlay {
        let cards = r#"[
            {"id":"B1","name":"Bat","type":"Minion","races":["BEAST"],"techLevel":1,"isBaconPool":true},
            {"id":"D1","name":"Whelp","type":"Minion","races":["DRAGON"],"techLevel":1,"isBaconPool":true}
        ]"#;
        let db = CardDb::parse(cards.as_bytes(), Path::new("test")).unwrap();
        let mut ov = Overlay::new(Config::default(), db, History::in_memory(Vec::new()));
        let lines = [
            "D 00:00:00.0000000 GameState.DebugPrintGame() - GameType=GT_BATTLEGROUNDS",
            "CREATE_GAME",
            "    GameEntity EntityID=1",
            "        tag=TURN value=1",
            "    Player EntityID=2 PlayerID=8 GameAccountId=[hi=1 lo=2]",
            "    Player EntityID=3 PlayerID=16 GameAccountId=[hi=0 lo=0]",
            "        tag=BACON_DUMMY_PLAYER value=1",
            "FULL_ENTITY - Creating ID=20 CardID=D1",
            "    tag=CONTROLLER value=16",
            "    tag=CARDTYPE value=MINION",
            "    tag=ZONE value=PLAY",
            "BLOCK_END",
        ];
        for l in lines {
            let l = if l.starts_with("D ") { l.to_string() } else { format!("{P}{l}") };
            ov.game.process(&l);
        }
        ov.game.take_events();
        ov
    }

    fn memory(game: u64, tribes: &[Tribe]) -> Option<LobbyRaces> {
        Some(LobbyRaces { game, tribes: tribes.to_vec() })
    }

    const LOBBY: [Tribe; 5] = [Tribe::Beast, Tribe::Dragon, Tribe::Mech, Tribe::Murloc, Tribe::Pirate];

    #[test]
    fn shop_inference_until_the_game_is_read() {
        let ov = overlay();
        let lobby = ov.lobby_tribes();
        assert_eq!((lobby.tribes, lobby.complete, lobby.from_game), (vec![Tribe::Dragon], false, false));
    }

    #[test]
    fn memory_gives_every_tribe_at_once() {
        let mut ov = overlay();
        ov.set_lobby_memory(memory(1, &LOBBY));
        let lobby = ov.lobby_tribes();
        assert_eq!((lobby.tribes, lobby.complete, lobby.from_game), (LOBBY.to_vec(), true, true));
    }

    #[test]
    fn stale_or_contradicted_memory_is_ignored() {
        let mut ov = overlay();
        // Shop offered a Dragon, so a lobby without Dragons is wrong.
        ov.set_lobby_memory(memory(1, &[Tribe::Beast, Tribe::Mech, Tribe::Murloc, Tribe::Pirate, Tribe::Undead]));
        assert!(!ov.lobby_tribes().from_game);
        // The previous game's GameState lingering into the next game.
        ov.set_lobby_memory(memory(1, &LOBBY));
        ov.ended_game = Some(1);
        assert!(!ov.lobby_tribes().from_game);
        ov.set_lobby_memory(memory(2, &LOBBY));
        assert!(ov.lobby_tribes().from_game);
    }

    #[test]
    fn keyword_filter_keeps_only_matching_cards() {
        let cards = r#"[
            {"id":"B1","name":"Bat","type":"Minion","races":["BEAST"],"techLevel":1,"isBaconPool":true,"mechanics":["DEATHRATTLE"]},
            {"id":"B2","name":"Boar","type":"Minion","races":["BEAST"],"techLevel":1,"isBaconPool":true},
            {"id":"D1","name":"Whelp","type":"Minion","races":["DRAGON"],"techLevel":1,"isBaconPool":true},
            {"id":"S1","name":"Coin","type":"Battleground_spell","techLevel":1,"isBaconPool":true,"cost":1}
        ]"#;
        let mut ov = overlay();
        ov.db = CardDb::parse(cards.as_bytes(), Path::new("test")).unwrap();
        ov.set_lobby_memory(memory(1, &LOBBY));
        ov.ui.selected_tier = Some(1);
        ov.ui.keyword = crate::cards::KEYWORDS.iter().position(|k| k.label == "Deathrattle");
        let list = ov.tier_list().unwrap();
        let names: Vec<_> = list.sections.iter().flat_map(|s| s.rows.iter().map(|r| r.name.as_str())).collect();
        assert_eq!(names, vec!["Bat"]);
        assert_eq!(list.sections.len(), 1, "empty tribes and spells are dropped");
    }

    #[test]
    fn tier_list_hit_testing() {
        let mut ov = overlay();
        ov.set_lobby_memory(memory(1, &LOBBY));
        ov.ui.selected_tier = Some(1);
        let list = ov.tier_list().unwrap();
        // Filter bar sits right under the 40px title band.
        assert_eq!(list.hit(100.0, 55.0, 1.0), Some(TierHit::FilterBar));
        ov.ui.filter_open = true;
        let open = ov.tier_list().unwrap();
        // First chip ("Any") right under the bar, second is the first keyword.
        assert_eq!(open.hit(20.0, 82.0, 1.0), Some(TierHit::Keyword(None)));
        assert_eq!(open.hit(110.0, 82.0, 1.0), Some(TierHit::Keyword(Some(0))));
    }

    fn guide(id: i32, tier: i32, tribe: Option<Tribe>) -> CompGuide {
        CompGuide {
            id,
            name: format!("Comp {id}"),
            tier,
            tier_rank: 0,
            difficulty: 1,
            tribe,
            core_cards: vec![1],
            addon_cards: vec![],
            representative_card: String::new(),
            summary: "Scale.".into(),
            how_to_play: String::new(),
            when_to_commit: String::new(),
            common_enablers: String::new(),
            previous_tier: None,
        }
    }

    #[test]
    fn comp_list_follows_lobby_and_opens_guides() {
        let mut ov = overlay();
        ov.db = CardDb::parse(br#"[{"id":"B1","dbfId":1,"name":"Bat","type":"Minion","techLevel":2}]"#, Path::new("test")).unwrap();
        let beasts = crate::enablers::Archetype {
            id: "beast_tribe".into(),
            tribe_wide: true,
            core: vec![],
            cards: vec![],
            heroes: vec![crate::enablers::HeroEnabler { hero: "H1".into(), games: 90, lift: 4.04, placement: 3.21 }],
        };
        ov.comps = Comps::with_guides(
            vec![guide(1, 1, Some(Tribe::Beast)), guide(2, 1, Some(Tribe::Undead)), guide(3, 2, None)],
            vec![beasts],
        );
        ov.set_lobby_memory(memory(1, &LOBBY));
        ov.ui.comps_open = true;
        let shown = |ov: &Overlay| -> Vec<String> {
            ov.comp_panel()
                .unwrap()
                .items
                .iter()
                .map(|i| match i {
                    CompItem::Tier { letter, .. } => letter.to_string(),
                    CompItem::Comp(r) => format!("{}{}", r.id, if r.dim { "-" } else { "" }),
                    _ => "?".into(),
                })
                .collect()
        };
        ov.ui.hide_unavailable = true;
        assert_eq!(shown(&ov), ["S", "1", "A", "3"]);
        ov.ui.hide_unavailable = false;
        assert_eq!(shown(&ov), ["S", "1", "2-", "A", "3"]);
        ov.ui.comp_id = Some(1);
        let guide = ov.comp_panel().unwrap();
        assert_eq!(guide.detail, Some("S"));
        assert!(guide.items.iter().any(|i| matches!(i, CompItem::Card { card_id, tier: 2, .. } if card_id == "B1")));
        assert!(guide.items.contains(&CompItem::Heading("HERO ENABLERS")));
        let hero = guide.items.iter().find_map(|i| match i {
            CompItem::Hero { card_id, detail, .. } => Some((card_id.as_str(), detail.as_str())),
            _ => None,
        });
        assert_eq!(hero, Some(("H1", "4.0× · 3.2")));
        ov.ui.comp_id = Some(2);
        assert!(!ov.comp_panel().unwrap().items.contains(&CompItem::Heading("HERO ENABLERS")), "no stats for Undead");
    }

    #[test]
    fn search_typing_tab_and_escape() {
        let cards = r#"[
            {"id":"B1","name":"Brann Bronzebeard","type":"Minion","techLevel":5,"isBaconPool":true},
            {"id":"H1","name":"Dinotamer Brann","type":"Hero","battlegroundsHero":true},
            {"id":"T1","name":"Bronze Charm","type":"Battleground_trinket","isBaconPool":true,"cost":2}
        ]"#;
        let mut ov = overlay();
        ov.db = CardDb::parse(cards.as_bytes(), Path::new("test")).unwrap();
        ov.ui.search_open = true;
        assert!(ov.search_panel().unwrap().rows.is_empty(), "nothing listed before typing");
        for c in "bran".chars() {
            ov.ui.search_key(Key::Char(c), 0);
        }
        let panel = ov.search_panel().unwrap();
        let names: Vec<_> = panel.rows.iter().map(|r| r.name.as_str()).collect();
        assert_eq!(names, ["Brann Bronzebeard", "Dinotamer Brann", "Bronze Charm"]);
        assert_eq!((panel.rows[0].tier, panel.rows[0].detail.as_str()), (5, "Minion · Neutral"));
        assert_eq!(panel.rows[2].detail, "Trinket · 2 gold");
        assert_eq!(ov.search_preview(&panel).unwrap().card_id, "B1", "best match previewed while typing");
        ov.ui.search_key(Key::Down, panel.rows.len());
        assert_eq!(ov.ui.search_row_hover(), Some(1));
        // Tab narrows to the next type: minions.
        ov.ui.search_key(Key::Tab, 0);
        assert_eq!(ov.search_panel().unwrap().rows.len(), 1);
        ov.ui.search_key(Key::Tab, 0);
        ov.ui.search_key(Key::Tab, 0);
        assert_eq!(ov.search_panel().unwrap().rows[0].card_id, "H1");
        ov.ui.search_key(Key::DeleteWord, 0);
        assert_eq!(ov.ui.search_query, "");
        ov.ui.search_key(Key::Escape, 0);
        assert!(ov.search_panel().is_none());
    }

    #[test]
    fn search_panel_hit_testing() {
        let mut ov = overlay();
        ov.ui.search_open = true;
        ov.ui.search_query = "bat".into();
        let panel = ov.search_panel().unwrap();
        // Field right under the 40px title, its "×" at the right end.
        assert_eq!(panel.hit(100.0, 55.0, 1.0), Some(SearchHit::Field));
        assert_eq!(panel.hit(260.0, 55.0, 1.0), Some(SearchHit::Clear));
        // Chip grid: "All" first, then the types in order.
        assert_eq!(panel.hit(20.0, 88.0, 1.0), Some(SearchHit::Category(None)));
        assert_eq!(panel.hit(100.0, 88.0, 1.0), Some(SearchHit::Category(Some(Category::Minion))));
    }

    #[test]
    fn lobby_only_toggle_filters_the_tier_list() {
        let mut ov = overlay();
        ov.set_lobby_memory(memory(1, &LOBBY));
        ov.ui.selected_tier = Some(1);
        let titles = |ov: &Overlay| -> Vec<String> {
            ov.tier_list().unwrap().sections.iter().map(|s| s.title.clone()).collect()
        };
        ov.ui.hide_unavailable = true;
        assert_eq!(titles(&ov), vec!["Beast", "Dragon"]);
        ov.ui.hide_unavailable = false;
        assert!(titles(&ov).iter().all(|t| t == "Beast" || t == "Dragon" || t.ends_with("not in lobby")));
    }

    #[test]
    fn opponent_popup_placed_below_sim_panel() {
        let hs = Rect::new(0, 0, 1920, 1080);
        let cfg = Config::default();
        let layout = Layout::new(hs, &cfg);
        let sim_size = (340, 72);
        let top_rect = layout.top_bar((300, 38));
        let sim_rect = layout.sim(sim_size, Some(top_rect));
        let popup_size = (496, 220);
        let popup_rect = layout.popup(popup_size, sim_rect);

        // Positioned in upper middle, directly below sim.
        assert_eq!(popup_rect.y, sim_rect.y + sim_rect.h + layout.px(6.0));
        assert_eq!(popup_rect.x + popup_rect.w / 2, sim_rect.x + sim_rect.w / 2);
        assert!(layout.popup_max_h(sim_rect) > 0);
        assert!(popup_rect.y + popup_rect.h < hs.h);
    }
}
