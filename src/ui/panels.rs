//! Panel models and their rendering. Models are plain data compared with
//! `PartialEq`; a panel is only re-rendered when its model changes.
//!
//! Layout constants are in unscaled pixels at a 1080p game window; `Ctx::s`
//! scales them to the actual window.

use super::draw::{Align, Canvas, Fonts, Rgba, Weight};
use super::theme::{Theme, ThemeName};
use crate::cards::{Category, Tribe, KEYWORDS};
use crate::config::{CompactMode, LayoutSize};
use crate::images::{ImageKind, Images};

pub fn tribe_color(t: Tribe) -> Rgba {
    Rgba::hex(match t {
        Tribe::Beast => 0x5d8a2f,
        Tribe::Demon => 0x7b2d8e,
        Tribe::Dragon => 0xb33a3a,
        Tribe::Elemental => 0x2a9d8f,
        Tribe::Mech => 0x5f7a8a,
        Tribe::Murloc => 0x2b7bb9,
        Tribe::Naga => 0x4a55b5,
        Tribe::Pirate => 0x8b5a2b,
        Tribe::Quilboar => 0xc2185b,
        Tribe::Undead => 0x56705a,
        Tribe::Aberration => 0xd4762c,
    })
}

pub struct Ctx<'a> {
    pub fonts: &'a mut Fonts,
    pub images: &'a mut Images,
    /// Colours every panel draws with; the settings panel switches them.
    pub theme: Theme,
    /// How much card art is drawn; the settings panel switches it.
    pub compact: CompactMode,
    pub s: f32,
    /// No compositor: paint opaque, square panels.
    pub opaque: bool,
}

impl Ctx<'_> {
    /// Top of a line box whose capitals are vertically centred on `cy`.
    fn mid(&self, cy: f32, px: f32, w: Weight) -> f32 {
        cy + 0.36 * px - self.fonts.ascent(px, w)
    }

    #[allow(clippy::too_many_arguments)]
    fn label(&mut self, c: &mut Canvas, text: &str, x: f32, cy: f32, px: f32, w: Weight, color: Rgba, align: Align) {
        let px = px * self.s;
        let y = self.mid(cy, px, w);
        c.text(self.fonts, text, x, y, px, w, color, align);
    }

    #[allow(clippy::too_many_arguments)]
    fn label_shadow(&mut self, c: &mut Canvas, text: &str, x: f32, cy: f32, px: f32, w: Weight, color: Rgba) {
        let shadow = self.theme.shadow;
        self.label_shadow_on(c, text, x, cy, px, w, color, shadow);
    }

    #[allow(clippy::too_many_arguments)]
    fn label_shadow_align(
        &mut self,
        c: &mut Canvas,
        text: &str,
        x: f32,
        cy: f32,
        px: f32,
        w: Weight,
        color: Rgba,
        align: Align,
    ) {
        let px = px * self.s;
        let y = self.mid(cy, px, w);
        let shadow = self.theme.shadow;
        c.text_shadow(self.fonts, text, x, y, px, w, color, align, shadow);
    }

    /// `label_shadow` with an explicit shadow: badges keep a dark one in
    /// every theme, because their fills are always bright.
    #[allow(clippy::too_many_arguments)]
    fn label_shadow_on(
        &mut self,
        c: &mut Canvas,
        text: &str,
        x: f32,
        cy: f32,
        px: f32,
        w: Weight,
        color: Rgba,
        shadow: Rgba,
    ) {
        let px = px * self.s;
        let y = self.mid(cy, px, w);
        c.text_shadow(self.fonts, text, x, y, px, w, color, Align::Left, shadow);
    }

    fn panel(&self, c: &mut Canvas) {
        let (w, h) = (c.width(), c.height());
        let t = self.theme;
        if self.opaque {
            c.rect(0.0, 0.0, w, h, t.bg_opaque);
            c.stroke_rrect(0.0, 0.0, w, h, 0.0, 1.0, t.border);
        } else {
            let r = 8.0 * self.s;
            c.rrect(0.0, 0.0, w, h, r, t.bg);
            c.stroke_rrect(0.0, 0.0, w, h, r, 1.0, t.border);
        }
    }

    /// A list row with the card's tile art fading in from the right. The
    /// compact mode halves the art, or drops it altogether.
    #[allow(clippy::too_many_arguments)]
    fn tile_row(&mut self, c: &mut Canvas, card_id: &str, x: f32, y: f32, w: f32, h: f32, base: Rgba) {
        c.rect(x, y, w, h, base);
        if self.compact == CompactMode::Text {
            return;
        }
        if let Some(img) = self.images.get(ImageKind::Tile, card_id) {
            let cap = match self.compact {
                CompactMode::Small => 0.35,
                _ => 0.7,
            };
            let tw = (h * img.width() as f32 / img.height() as f32).min(w * cap);
            let tx = x + w - tw;
            c.image_cover(&img, tx, y, tw, h, 0.0, 1.0);
            c.hgradient(tx - 1.0, y, tw * 0.55, h, base, base.alpha(0));
        }
    }
}

fn s_ceil(v: f32, s: f32) -> i32 {
    (v * s).ceil() as i32
}

// ---- top bar: turn counter and tavern tier buttons -----------------------

/// What the pointer is over in the top bar.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TopHit {
    Tier(i32),
    Comps,
    Search,
    Settings,
}

#[derive(Debug, Clone, PartialEq)]
pub struct TopBar {
    pub turn: i32,
    pub tavern_tier: i32,
    pub max_tier: i32,
    pub selected: Option<i32>,
    pub hover: Option<TopHit>,
    /// Show the comps button, and whether its panel is open.
    pub comps: bool,
    pub comps_open: bool,
    /// Same for the card search button.
    pub search: bool,
    pub search_open: bool,
    /// The settings panel; its button is always there.
    pub settings_open: bool,
}

impl TopBar {
    const H: f32 = 38.0;
    const TURN_W: f32 = 84.0;
    const TOOL_W: f32 = 64.0;
    const BTN: f32 = 30.0;
    const GAP: f32 = 4.0;
    const PAD: f32 = 4.0;

    /// Buttons between the turn counter and the tiers, left to right.
    fn tools(&self) -> Vec<(TopHit, &'static str, bool)> {
        let mut tools = Vec::new();
        if self.comps {
            tools.push((TopHit::Comps, "Comps", self.comps_open));
        }
        if self.search {
            tools.push((TopHit::Search, "Search", self.search_open));
        }
        // The settings button shows an icon instead of a label.
        tools.push((TopHit::Settings, "", self.settings_open));
        tools
    }

    /// The settings button is square like the tier buttons; the rest fit a label.
    fn tool_w(hit: TopHit) -> f32 {
        match hit {
            TopHit::Settings => Self::BTN,
            _ => Self::TOOL_W,
        }
    }

    fn tool_x(&self, i: usize) -> f32 {
        self.tools()
            .into_iter()
            .take(i)
            .fold(Self::PAD + Self::TURN_W + Self::GAP * 2.0, |x, (hit, _, _)| x + Self::tool_w(hit) + Self::GAP)
    }

    /// Right edge of the last tool button.
    fn tools_end(&self) -> f32 {
        let tools = self.tools();
        match tools.len() {
            0 => Self::PAD + Self::TURN_W + Self::GAP * 2.0,
            n => self.tool_x(n - 1) + Self::tool_w(tools[n - 1].0),
        }
    }

    fn tiers_x(&self) -> f32 {
        self.tools_end() + if self.tools().is_empty() { 0.0 } else { Self::GAP * 3.0 }
    }

    pub fn size(&self, s: f32) -> (i32, i32) {
        let w = match self.max_tier {
            0 => self.tools_end() + if self.tools().is_empty() { 0.0 } else { Self::PAD },
            n => self.tiers_x() + n as f32 * (Self::BTN + Self::GAP) - Self::GAP + Self::PAD,
        };
        (s_ceil(w, s), s_ceil(Self::H, s))
    }

    fn button_x(&self, tier: i32) -> f32 {
        self.tiers_x() + (tier - 1) as f32 * (Self::BTN + Self::GAP)
    }

    pub fn hit(&self, x: f32, s: f32) -> Option<TopHit> {
        let x = x / s;
        for (i, (hit, _, _)) in self.tools().into_iter().enumerate() {
            let bx = self.tool_x(i);
            if x >= bx && x < bx + Self::tool_w(hit) {
                return Some(hit);
            }
        }
        (1..=self.max_tier)
            .find(|&t| {
                let bx = self.button_x(t);
                x >= bx && x < bx + Self::BTN
            })
            .map(TopHit::Tier)
    }

    pub fn render(&self, c: &mut Canvas, ctx: &mut Ctx) {
        let s = ctx.s;
        let t = ctx.theme;
        ctx.panel(c);
        let label = if self.turn > 0 { format!("Turn {}", self.turn) } else { "Hero pick".to_string() };
        ctx.label(c, &label, (Self::PAD + Self::TURN_W / 2.0) * s, Self::H / 2.0 * s, 15.0, Weight::Bold, t.text, Align::Center);
        let sep = |c: &mut Canvas, x: f32| c.rect(x * s, 7.0 * s, 1.0, (Self::H - 14.0) * s, t.border);
        sep(c, Self::PAD + Self::TURN_W + Self::GAP);
        let by = (Self::H - Self::BTN) / 2.0 * s;
        let b = Self::BTN * s;
        let fill = |selected: bool, hover: bool| if selected {
            t.accent
        } else if hover {
            t.button_hover
        } else {
            t.button
        };
        let tools = self.tools();
        for (i, (hit, label, open)) in tools.iter().enumerate() {
            let (bx, bw) = (self.tool_x(i) * s, Self::tool_w(*hit) * s);
            c.rrect(bx, by, bw, b, 5.0 * s, fill(*open, self.hover == Some(*hit)));
            let color = if *open { t.on_accent } else { t.text };
            if *hit == TopHit::Settings {
                settings_icon(c, ctx, bx + bw / 2.0, by + b / 2.0, color);
            } else {
                ctx.label(c, label, bx + bw / 2.0, by + b / 2.0, 13.0, Weight::Bold, color, Align::Center);
            }
        }
        if !tools.is_empty() && self.max_tier > 0 {
            sep(c, self.tools_end() + Self::GAP * 1.5);
        }
        for tier in 1..=self.max_tier {
            let bx = self.button_x(tier) * s;
            let selected = self.selected == Some(tier);
            c.rrect(bx, by, b, b, 5.0 * s, fill(selected, self.hover == Some(TopHit::Tier(tier))));
            if tier == self.tavern_tier {
                c.stroke_rrect(bx, by, b, b, 5.0 * s, 2.0 * s, t.gold);
            }
            let color = if selected {
                t.on_accent
            } else if tier <= self.tavern_tier {
                t.text
            } else {
                t.dim
            };
            ctx.label(c, &tier.to_string(), bx + b / 2.0, by + b / 2.0, 15.0, Weight::Bold, color, Align::Center);
        }
    }
}

/// The settings button: three sliders.
fn settings_icon(c: &mut Canvas, ctx: &mut Ctx, cx: f32, cy: f32, color: Rgba) {
    let s = ctx.s;
    let w = 16.0 * s;
    let x = cx - w / 2.0;
    let (track, knob) = (1.5 * s, 2.6 * s);
    for (i, k) in [0.72, 0.32, 0.58].into_iter().enumerate() {
        let y = cy + (i as f32 - 1.0) * 5.5 * s;
        c.rrect(x, y - track / 2.0, w, track, track / 2.0, color);
        c.circle(x + w * k, y, knob, color);
    }
}

// ---- tier browser --------------------------------------------------------

#[derive(Debug, Clone, PartialEq)]
pub struct TierRow {
    pub card_id: String,
    pub name: String,
    /// Gold cost, shown for tavern spells.
    pub cost: Option<i32>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct TierSection {
    pub title: String,
    pub color: Rgba,
    /// Tribe not confirmed in this lobby yet.
    pub dim: bool,
    pub rows: Vec<TierRow>,
}

/// What the pointer is over in the tier browser.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TierHit {
    LobbyToggle,
    /// The keyword filter bar (opens/closes the keyword grid).
    FilterBar,
    /// The "×" that clears the active keyword.
    FilterClear,
    /// A keyword chip; `None` is "Any".
    Keyword(Option<usize>),
    Row(usize),
}

#[derive(Debug, Clone, PartialEq)]
pub struct TierList {
    pub tier: i32,
    pub subtitle: String,
    pub sections: Vec<TierSection>,
    pub scroll: f32,
    pub hover: Option<TierHit>,
    pub images_rev: u64,
    /// State of the "lobby only" toggle in the header.
    pub hide_unavailable: bool,
    /// Active keyword filter, an index into `KEYWORDS`.
    pub keyword: Option<usize>,
    /// Keyword grid expanded.
    pub filter_open: bool,
}

/// Unscaled rectangle.
#[derive(Clone, Copy)]
struct Box2 {
    x: f32,
    y: f32,
    w: f32,
    h: f32,
}

impl Box2 {
    fn contains(&self, x: f32, y: f32) -> bool {
        x >= self.x && x < self.x + self.w && y >= self.y && y < self.y + self.h
    }

    fn scaled(&self, s: f32) -> (f32, f32, f32, f32) {
        (self.x * s, self.y * s, self.w * s, self.h * s)
    }
}

impl TierList {
    pub const W: f32 = 252.0;
    const TITLE: f32 = 40.0;
    const FILTER: f32 = 30.0;
    const TOGGLE_W: f32 = 94.0;
    const TOGGLE_H: f32 = 22.0;
    const CHIP_H: f32 = 22.0;
    const CHIP_COLS: usize = 3;
    const GAP: f32 = 4.0;
    const HEAD: f32 = 24.0;
    const ROW: f32 = 27.0;
    const PAD: f32 = 4.0;
    const CLEAR_W: f32 = 26.0;

    fn chip_count() -> usize {
        KEYWORDS.len() + 1
    }

    fn grid_h(&self) -> f32 {
        if !self.filter_open {
            return 0.0;
        }
        let rows = Self::chip_count().div_ceil(Self::CHIP_COLS) as f32;
        rows * (Self::CHIP_H + Self::GAP) + Self::GAP
    }

    /// Height of the fixed header (title, filter bar, keyword grid).
    fn header_h(&self) -> f32 {
        Self::TITLE + Self::FILTER + self.grid_h()
    }

    fn toggle_box() -> Box2 {
        Box2 {
            x: Self::W - Self::TOGGLE_W - 10.0,
            y: (Self::TITLE - Self::TOGGLE_H) / 2.0,
            w: Self::TOGGLE_W,
            h: Self::TOGGLE_H,
        }
    }

    fn filter_box() -> Box2 {
        Box2 { x: Self::PAD + 2.0, y: Self::TITLE + 3.0, w: Self::W - 2.0 * Self::PAD - 4.0, h: Self::FILTER - 6.0 }
    }

    fn chip_box(i: usize) -> Box2 {
        let inner = Self::W - 2.0 * Self::PAD - 4.0;
        let cw = (inner - Self::GAP * (Self::CHIP_COLS - 1) as f32) / Self::CHIP_COLS as f32;
        let (col, row) = (i % Self::CHIP_COLS, i / Self::CHIP_COLS);
        Box2 {
            x: Self::PAD + 2.0 + col as f32 * (cw + Self::GAP),
            y: Self::TITLE + Self::FILTER + Self::GAP + row as f32 * (Self::CHIP_H + Self::GAP),
            w: cw,
            h: Self::CHIP_H,
        }
    }

    fn content_h(&self) -> f32 {
        let rows: f32 = self.sections.iter().map(|s| Self::HEAD + s.rows.len() as f32 * Self::ROW).sum();
        self.header_h() + if self.sections.is_empty() { Self::ROW } else { rows } + Self::PAD
    }

    pub fn size(&self, s: f32, max_h: i32) -> (i32, i32) {
        (s_ceil(Self::W, s), s_ceil(self.content_h(), s).min(max_h))
    }

    pub fn max_scroll(&self, s: f32, h: i32) -> f32 {
        (self.content_h() * s - h as f32).max(0.0)
    }

    /// Row positions (unscaled, unscrolled): flat index → (row, top).
    fn rows(&self) -> Vec<(&TierRow, f32)> {
        let mut y = self.header_h();
        let mut out = Vec::new();
        for sec in &self.sections {
            y += Self::HEAD;
            for r in &sec.rows {
                out.push((r, y));
                y += Self::ROW;
            }
        }
        out
    }

    /// Element under a window-local point.
    pub fn hit(&self, x: f32, y: f32, s: f32) -> Option<TierHit> {
        let (ux, uy) = (x / s, y / s);
        if uy < Self::TITLE {
            return Self::toggle_box().contains(ux, uy).then_some(TierHit::LobbyToggle);
        }
        if uy < Self::TITLE + Self::FILTER {
            let bar = Self::filter_box();
            if !bar.contains(ux, uy) {
                return None;
            }
            let on_clear = self.keyword.is_some() && ux >= bar.x + bar.w - Self::CLEAR_W;
            return Some(if on_clear { TierHit::FilterClear } else { TierHit::FilterBar });
        }
        if uy < self.header_h() {
            return (0..Self::chip_count())
                .find(|&i| Self::chip_box(i).contains(ux, uy))
                .map(|i| TierHit::Keyword(i.checked_sub(1)));
        }
        let cy = (y + self.scroll) / s;
        self.rows().iter().position(|(_, top)| cy >= *top && cy < top + Self::ROW).map(TierHit::Row)
    }

    pub fn row(&self, i: usize) -> Option<&TierRow> {
        self.rows().get(i).map(|(r, _)| *r)
    }

    /// Window-local vertical centre of a row.
    pub fn row_center(&self, i: usize, s: f32) -> Option<f32> {
        self.rows().get(i).map(|(_, top)| (top + Self::ROW / 2.0) * s - self.scroll)
    }

    fn keyword_label(&self) -> &'static str {
        self.keyword.and_then(|k| KEYWORDS.get(k)).map(|k| k.label).unwrap_or("Any")
    }

    pub fn render(&self, c: &mut Canvas, ctx: &mut Ctx) {
        let s = ctx.s;
        let t = ctx.theme;
        let w = c.width();
        ctx.panel(c);
        self.render_rows(c, ctx);
        // Header drawn last so scrolled rows slide under it.
        let hh = self.header_h() * s;
        if ctx.opaque {
            c.rect(0.0, 0.0, w, hh, t.bg_opaque);
        } else {
            c.rrect(0.0, 0.0, w, hh.min(16.0 * s), 8.0 * s, t.bg_opaque);
            c.rect(0.0, 8.0 * s, w, hh - 8.0 * s, t.bg_opaque);
        }
        self.render_title(c, ctx);
        self.render_filter(c, ctx);
        c.rect(0.0, hh - 1.0, w, 1.0, t.border);
        let max_scroll = self.max_scroll(s, c.height() as i32);
        if max_scroll > 0.0 {
            let track = c.height() - hh - 8.0 * s;
            let thumb = (track * c.height() / (self.content_h() * s)).max(20.0 * s);
            let ty = hh + 4.0 * s + (track - thumb) * (self.scroll / max_scroll);
            c.rrect(w - 4.0 * s, ty, 2.5 * s, thumb, 1.5 * s, t.scrollbar);
        }
    }

    fn render_rows(&self, c: &mut Canvas, ctx: &mut Ctx) {
        let s = ctx.s;
        let t = ctx.theme;
        let inner_x = Self::PAD * s;
        let inner_w = c.width() - 2.0 * Self::PAD * s;
        let base = if ctx.opaque { t.bg_opaque } else { t.row };
        let mut y = self.header_h() * s - self.scroll;
        if self.sections.is_empty() {
            let msg = format!("No {} cards in tier {}", self.keyword_label(), self.tier);
            ctx.label(c, &msg, c.width() / 2.0, y + Self::ROW * s / 2.0, 11.5, Weight::Medium, t.dim, Align::Center);
            return;
        }
        let mut index = 0;
        for sec in &self.sections {
            let hh = Self::HEAD * s;
            if y + hh > 0.0 && y < c.height() {
                c.rect(inner_x, y, inner_w, hh, t.header);
                c.rect(inner_x, y, 4.0 * s, hh, if sec.dim { sec.color.alpha(110) } else { sec.color });
                let color = if sec.dim { t.dim } else { t.text };
                ctx.label(c, &sec.title, inner_x + 11.0 * s, y + hh / 2.0, 13.0, Weight::Bold, color, Align::Left);
            }
            y += hh;
            for row in &sec.rows {
                let rh = Self::ROW * s;
                if y + rh > 0.0 && y < c.height() {
                    ctx.tile_row(c, &row.card_id, inner_x, y + 1.0 * s, inner_w, rh - 1.0 * s, base);
                    if self.hover == Some(TierHit::Row(index)) {
                        c.rect(inner_x, y + 1.0 * s, inner_w, rh - 1.0 * s, t.tint);
                        c.rect(inner_x, y + 1.0 * s, 3.0 * s, rh - 1.0 * s, t.gold);
                    }
                    let max = inner_w - if row.cost.is_some() { 40.0 * s } else { 14.0 * s };
                    let name = ctx.fonts.fit(&row.name, 12.5 * s, Weight::Medium, max);
                    let color = if sec.dim { t.dim } else { t.text };
                    ctx.label_shadow(c, &name, inner_x + 9.0 * s, y + rh / 2.0, 12.5, Weight::Medium, color);
                    if let Some(cost) = row.cost {
                        let cx = inner_x + inner_w - 14.0 * s;
                        c.circle(cx, y + rh / 2.0, 9.5 * s, Rgba(0, 0, 0, 160));
                        c.circle(cx, y + rh / 2.0, 8.5 * s, BADGE_GOLD);
                        ctx.label(c, &cost.to_string(), cx, y + rh / 2.0, 11.5, Weight::Bold, Rgba(40, 28, 0, 255), Align::Center);
                    }
                }
                y += rh;
                index += 1;
            }
        }
    }

    fn render_title(&self, c: &mut Canvas, ctx: &mut Ctx) {
        let s = ctx.s;
        let t = ctx.theme;
        let (tx, ty, tw, th) = Self::toggle_box().scaled(s);
        ctx.label(c, &format!("Tavern Tier {}", self.tier), 12.0 * s, 15.0 * s, 14.0, Weight::Bold, t.gold, Align::Left);
        let subtitle = ctx.fonts.fit(&self.subtitle, 10.0 * s, Weight::Medium, tx - 18.0 * s);
        ctx.label(c, &subtitle, 12.0 * s, 31.0 * s, 10.0, Weight::Medium, t.dim, Align::Left);
        // "Lobby only" toggle: hides tribes that are not in this lobby.
        let hover = self.hover == Some(TierHit::LobbyToggle);
        c.rrect(tx, ty, tw, th, 5.0 * s, if hover { t.button_hover } else { t.button });
        let bx = tx + 7.0 * s;
        let b = 12.0 * s;
        let by = ty + (th - b) / 2.0;
        if self.hide_unavailable {
            c.rrect(bx, by, b, b, 3.0 * s, t.accent);
            c.rrect(bx + 3.0 * s, by + 3.0 * s, b - 6.0 * s, b - 6.0 * s, 1.5 * s, t.on_accent);
        } else {
            c.stroke_rrect(bx, by, b, b, 3.0 * s, 1.5 * s, t.dim);
        }
        let color = if self.hide_unavailable { t.text } else { t.dim };
        ctx.label(c, "Lobby only", bx + b + 6.0 * s, ty + th / 2.0, 10.5, Weight::Bold, color, Align::Left);
    }

    fn render_filter(&self, c: &mut Canvas, ctx: &mut Ctx) {
        let s = ctx.s;
        let t = ctx.theme;
        let (x, y, w, h) = Self::filter_box().scaled(s);
        let active = self.keyword.is_some();
        let hover = matches!(self.hover, Some(TierHit::FilterBar | TierHit::FilterClear));
        let fill = match (active, hover) {
            (true, _) => t.accent,
            (false, true) => t.button_hover,
            (false, false) => t.button,
        };
        c.rrect(x, y, w, h, 5.0 * s, fill);
        let cy = y + h / 2.0;
        let on_color = if active { t.on_accent } else { t.dim };
        ctx.label(c, "Keyword", x + 9.0 * s, cy, 10.5, Weight::Bold, on_color, Align::Left);
        let value_x = x + 66.0 * s;
        ctx.label(c, self.keyword_label(), value_x, cy, 12.0, Weight::Bold, if active { t.on_accent } else { t.text }, Align::Left);
        if active {
            let clear_hover = self.hover == Some(TierHit::FilterClear);
            let cx = x + w - Self::CLEAR_W * s / 2.0;
            if clear_hover {
                c.circle(cx, cy, 8.5 * s, t.tint);
            }
            ctx.label(c, "×", cx, cy, 15.0, Weight::Bold, t.on_accent, Align::Center);
        } else {
            // Disclosure arrow: down when closed, up when open.
            let (ax, aw, ah) = (x + w - 16.0 * s, 8.0 * s, 4.5 * s);
            let (tip, base) = if self.filter_open { (cy - ah / 2.0, cy + ah / 2.0) } else { (cy + ah / 2.0, cy - ah / 2.0) };
            c.triangle([(ax - aw / 2.0, base), (ax + aw / 2.0, base), (ax, tip)], t.dim);
        }
        if !self.filter_open {
            return;
        }
        for i in 0..Self::chip_count() {
            let (cx, cy, cw, ch) = Self::chip_box(i).scaled(s);
            let keyword = i.checked_sub(1);
            let selected = keyword == self.keyword;
            let hover = self.hover == Some(TierHit::Keyword(keyword));
            let fill = if selected {
                t.accent
            } else if hover {
                t.button_hover
            } else {
                t.button
            };
            c.rrect(cx, cy, cw, ch, 4.0 * s, fill);
            let label = keyword.map(|k| KEYWORDS[k].label).unwrap_or("Any");
            // Shrink long labels a little before resorting to an ellipsis.
            let mut px = 10.0;
            while px > 8.5 && ctx.fonts.measure(label, px * s, Weight::Bold) > cw - 6.0 * s {
                px -= 0.5;
            }
            let label = ctx.fonts.fit(label, px * s, Weight::Bold, cw - 6.0 * s);
            let color = if selected {
                t.on_accent
            } else if hover {
                t.text
            } else {
                t.soft
            };
            ctx.label(c, &label, cx + cw / 2.0, cy + ch / 2.0, px, Weight::Bold, color, Align::Center);
        }
    }
}

/// Badges (gold costs, placements, comp tiers) keep bright fills with dark
/// text in every theme.
const BADGE_GOLD: Rgba = Rgba(242, 193, 78, 255);

// ---- meta comps ----------------------------------------------------------

pub fn comp_tier_color(letter: &str) -> Rgba {
    Rgba::hex(match letter {
        "S" => 0xe8603c,
        "A" => 0xf0a030,
        "B" => 0xd8c040,
        "C" => 0x78b850,
        _ => 0x5890c0,
    })
}

#[derive(Debug, Clone, PartialEq)]
pub struct CompRow {
    pub id: i32,
    pub name: String,
    /// Tribe and difficulty.
    pub detail: String,
    pub color: Rgba,
    /// Card whose art stands for the comp.
    pub card_id: String,
    /// +1 moved up a tier since the last snapshot, -1 down.
    pub trend: i32,
    /// Tribe not in this lobby.
    pub dim: bool,
    /// The player's hero enables this comp.
    pub your_hero: bool,
}

#[derive(Debug, Clone, PartialEq)]
pub enum CompItem {
    /// Band opening a tier of the meta list.
    Tier { letter: &'static str, count: usize },
    Comp(CompRow),
    Heading(&'static str),
    /// One wrapped line of guide text.
    Text { text: String, strong: bool },
    Card { card_id: String, name: String, tier: i32 },
    /// A hero that enables the comp; `detail` sums up its stats.
    Hero { card_id: String, name: String, detail: String, yours: bool },
    /// Loading, errors, nothing to show.
    Note(String),
    Gap,
}

impl CompItem {
    fn height(&self) -> f32 {
        match self {
            CompItem::Tier { .. } => 26.0,
            CompItem::Comp(_) => 36.0,
            CompItem::Heading(_) => 24.0,
            CompItem::Text { .. } => 16.0,
            CompItem::Card { .. } | CompItem::Hero { .. } => 27.0,
            CompItem::Note(_) => 30.0,
            CompItem::Gap => 6.0,
        }
    }

    /// Reacts to the pointer (highlight, click or card preview).
    pub fn hoverable(&self) -> bool {
        matches!(self, CompItem::Comp(_) | CompItem::Card { .. } | CompItem::Hero { .. })
    }
}

/// What the pointer is over in the comps panel.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CompHit {
    LobbyToggle,
    Back,
    Item(usize),
}

/// Meta tier list of comps, or the guide of one comp.
#[derive(Debug, Clone, PartialEq)]
pub struct CompPanel {
    pub title: String,
    pub subtitle: String,
    /// Showing one comp: the header has a back button and its tier badge.
    pub detail: Option<&'static str>,
    pub items: Vec<CompItem>,
    pub scroll: f32,
    pub hover: Option<CompHit>,
    pub images_rev: u64,
    pub hide_unavailable: bool,
}

impl CompPanel {
    pub const W: f32 = 300.0;
    /// Guide text size and the width it is wrapped to (unscaled).
    pub const TEXT_PX: f32 = 11.5;
    pub const TEXT_W: f32 = Self::W - 2.0 * Self::PAD - 24.0;
    const TITLE: f32 = 46.0;
    const PAD: f32 = 4.0;
    const BACK: f32 = 30.0;

    fn content_h(&self) -> f32 {
        Self::TITLE + self.items.iter().map(CompItem::height).sum::<f32>() + Self::PAD
    }

    pub fn size(&self, s: f32, max_h: i32) -> (i32, i32) {
        (s_ceil(Self::W, s), s_ceil(self.content_h(), s).min(max_h))
    }

    pub fn max_scroll(&self, s: f32, h: i32) -> f32 {
        (self.content_h() * s - h as f32).max(0.0)
    }

    /// Item tops (unscaled, unscrolled).
    fn tops(&self) -> impl Iterator<Item = (usize, &CompItem, f32)> {
        let mut y = Self::TITLE;
        self.items.iter().enumerate().map(move |(i, item)| {
            let top = y;
            y += item.height();
            (i, item, top)
        })
    }

    fn toggle_box() -> Box2 {
        Box2 { x: Self::W - TierList::TOGGLE_W - 10.0, y: (Self::TITLE - TierList::TOGGLE_H) / 2.0, w: TierList::TOGGLE_W, h: TierList::TOGGLE_H }
    }

    fn back_box() -> Box2 {
        Box2 { x: 8.0, y: (Self::TITLE - Self::BACK) / 2.0, w: Self::BACK, h: Self::BACK }
    }

    pub fn hit(&self, x: f32, y: f32, s: f32) -> Option<CompHit> {
        let (ux, uy) = (x / s, y / s);
        if uy < Self::TITLE {
            return match self.detail {
                Some(_) => Self::back_box().contains(ux, uy).then_some(CompHit::Back),
                None => Self::toggle_box().contains(ux, uy).then_some(CompHit::LobbyToggle),
            };
        }
        let cy = (y + self.scroll) / s;
        self.tops().find(|(_, item, top)| cy >= *top && cy < top + item.height()).map(|(i, _, _)| CompHit::Item(i))
    }

    pub fn item(&self, i: usize) -> Option<&CompItem> {
        self.items.get(i)
    }

    /// Window-local vertical centre of an item.
    pub fn item_center(&self, i: usize, s: f32) -> Option<f32> {
        self.tops().find(|(j, _, _)| *j == i).map(|(_, item, top)| (top + item.height() / 2.0) * s - self.scroll)
    }

    pub fn render(&self, c: &mut Canvas, ctx: &mut Ctx) {
        let s = ctx.s;
        let t = ctx.theme;
        let w = c.width();
        ctx.panel(c);
        self.render_items(c, ctx);
        // Header drawn last so scrolled items slide under it.
        let hh = Self::TITLE * s;
        if ctx.opaque {
            c.rect(0.0, 0.0, w, hh, t.bg_opaque);
        } else {
            c.rrect(0.0, 0.0, w, 16.0 * s, 8.0 * s, t.bg_opaque);
            c.rect(0.0, 8.0 * s, w, hh - 8.0 * s, t.bg_opaque);
        }
        self.render_header(c, ctx);
        c.rect(0.0, hh - 1.0, w, 1.0, t.border);
        let max_scroll = self.max_scroll(s, c.height() as i32);
        if max_scroll > 0.0 {
            let track = c.height() - hh - 8.0 * s;
            let thumb = (track * c.height() / (self.content_h() * s)).max(20.0 * s);
            let ty = hh + 4.0 * s + (track - thumb) * (self.scroll / max_scroll);
            c.rrect(w - 4.0 * s, ty, 2.5 * s, thumb, 1.5 * s, t.scrollbar);
        }
    }

    fn render_header(&self, c: &mut Canvas, ctx: &mut Ctx) {
        let s = ctx.s;
        let t = ctx.theme;
        // Left edge of the title and right edge of the room it has.
        let (tx, right) = match self.detail {
            Some(letter) => {
                let (bx, by, bw, bh) = Self::back_box().scaled(s);
                c.rrect(bx, by, bw, bh, 5.0 * s, if self.hover == Some(CompHit::Back) { t.button_hover } else { t.button });
                let (cx, cy, a) = (bx + bw / 2.0, by + bh / 2.0, 5.0 * s);
                c.triangle([(cx + a * 0.6, cy - a), (cx + a * 0.6, cy + a), (cx - a * 0.7, cy)], t.text);
                let tx = bx + bw + 9.0 * s;
                let b = 26.0 * s;
                let bx = c.width() - b - 10.0 * s;
                c.rrect(bx, (Self::TITLE * s - b) / 2.0, b, b, 5.0 * s, comp_tier_color(letter));
                ctx.label(c, letter, bx + b / 2.0, Self::TITLE * s / 2.0, 15.0, Weight::Bold, Rgba(30, 20, 10, 255), Align::Center);
                (tx, bx - 8.0 * s)
            }
            None => {
                let (bx, by, bw, bh) = Self::toggle_box().scaled(s);
                let hover = self.hover == Some(CompHit::LobbyToggle);
                c.rrect(bx, by, bw, bh, 5.0 * s, if hover { t.button_hover } else { t.button });
                let b = 12.0 * s;
                let (cx, cy) = (bx + 7.0 * s, by + (bh - b) / 2.0);
                if self.hide_unavailable {
                    c.rrect(cx, cy, b, b, 3.0 * s, t.accent);
                    c.rrect(cx + 3.0 * s, cy + 3.0 * s, b - 6.0 * s, b - 6.0 * s, 1.5 * s, t.on_accent);
                } else {
                    c.stroke_rrect(cx, cy, b, b, 3.0 * s, 1.5 * s, t.dim);
                }
                let color = if self.hide_unavailable { t.text } else { t.dim };
                ctx.label(c, "Lobby only", cx + b + 6.0 * s, by + bh / 2.0, 10.5, Weight::Bold, color, Align::Left);
                (12.0 * s, bx - 8.0 * s)
            }
        };
        let title = ctx.fonts.fit(&self.title, 14.0 * s, Weight::Bold, right - tx);
        ctx.label(c, &title, tx, 16.0 * s, 14.0, Weight::Bold, t.gold, Align::Left);
        let subtitle = ctx.fonts.fit(&self.subtitle, 10.0 * s, Weight::Medium, right - tx);
        ctx.label(c, &subtitle, tx, 33.0 * s, 10.0, Weight::Medium, t.dim, Align::Left);
    }

    fn render_items(&self, c: &mut Canvas, ctx: &mut Ctx) {
        let s = ctx.s;
        let t = ctx.theme;
        let x = Self::PAD * s;
        let iw = c.width() - 2.0 * Self::PAD * s;
        let base = if ctx.opaque { t.bg_opaque } else { t.row };
        for (i, item, top) in self.tops() {
            let y = top * s - self.scroll;
            let h = item.height() * s;
            if y + h <= 0.0 || y >= c.height() {
                continue;
            }
            let hover = self.hover == Some(CompHit::Item(i));
            let cy = y + h / 2.0;
            match item {
                CompItem::Tier { letter, count } => {
                    let color = comp_tier_color(letter);
                    c.rect(x, y + 2.0 * s, iw, h - 2.0 * s, t.header);
                    let b = 18.0 * s;
                    let cy = cy + 1.0 * s;
                    c.rrect(x + 6.0 * s, cy - b / 2.0, b, b, 4.0 * s, color);
                    ctx.label(c, letter, x + 6.0 * s + b / 2.0, cy, 12.0, Weight::Bold, Rgba(30, 20, 10, 255), Align::Center);
                    ctx.label(c, &format!("{letter} tier"), x + 32.0 * s, cy, 12.5, Weight::Bold, t.text, Align::Left);
                    let n = format!("{count} comp{}", if *count == 1 { "" } else { "s" });
                    let nw = ctx.fonts.measure(&n, 10.0 * s, Weight::Medium);
                    ctx.label(c, &n, x + iw - nw - 10.0 * s, cy, 10.0, Weight::Medium, t.dim, Align::Left);
                }
                CompItem::Comp(row) => {
                    let (ry, rh) = (y + 1.0 * s, h - 1.0 * s);
                    ctx.tile_row(c, &row.card_id, x, ry, iw, rh, base);
                    if row.dim {
                        c.rect(x, ry, iw, rh, base.alpha(130));
                    }
                    c.rect(x, ry, 4.0 * s, rh, if row.dim { row.color.alpha(110) } else { row.color });
                    if hover {
                        c.rect(x, ry, iw, rh, t.tint);
                        c.rect(x, ry, 3.0 * s, rh, t.gold);
                    }
                    if row.your_hero {
                        c.rect(x + iw - 3.0 * s, ry, 3.0 * s, rh, t.gold);
                    }
                    let max = iw - 40.0 * s;
                    let name = ctx.fonts.fit(&row.name, 12.5 * s, Weight::Bold, max);
                    ctx.label_shadow(c, &name, x + 11.0 * s, cy - 7.0 * s, 12.5, Weight::Bold, if row.dim { t.dim } else { t.text });
                    let detail = ctx.fonts.fit(&row.detail, 10.0 * s, Weight::Medium, max);
                    ctx.label_shadow(c, &detail, x + 11.0 * s, cy + 8.0 * s, 10.0, Weight::Medium, t.soft);
                    if row.trend != 0 {
                        let (ax, a) = (x + iw - 14.0 * s, 4.5 * s);
                        c.circle(ax, cy, 8.5 * s, Rgba(0, 0, 0, 170));
                        let (tip, foot, color) =
                            if row.trend > 0 { (cy - a, cy + a * 0.8, t.win) } else { (cy + a, cy - a * 0.8, t.loss) };
                        c.triangle([(ax - a, foot), (ax + a, foot), (ax, tip)], color);
                    }
                }
                CompItem::Heading(text) => {
                    ctx.label(c, text, x + 8.0 * s, cy + 3.0 * s, 10.0, Weight::Bold, t.gold.alpha(220), Align::Left);
                }
                CompItem::Text { text, strong } => {
                    let color = if *strong { t.text } else { t.soft };
                    let weight = if *strong { Weight::Bold } else { Weight::Medium };
                    ctx.label(c, text, x + 8.0 * s, cy, Self::TEXT_PX, weight, color, Align::Left);
                }
                CompItem::Card { card_id, name, tier } => {
                    let (ry, rh) = (y + 1.0 * s, h - 1.0 * s);
                    ctx.tile_row(c, card_id, x, ry, iw, rh, base);
                    if hover {
                        c.rect(x, ry, iw, rh, t.tint);
                        c.rect(x, ry, 3.0 * s, rh, t.gold);
                    }
                    let name = ctx.fonts.fit(name, 12.0 * s, Weight::Medium, iw - 44.0 * s);
                    ctx.label_shadow(c, &name, x + 9.0 * s, cy, 12.0, Weight::Medium, t.text);
                    if *tier > 0 {
                        let tx = x + iw - 14.0 * s;
                        c.circle(tx, cy, 9.5 * s, Rgba(0, 0, 0, 170));
                        c.circle(tx, cy, 8.5 * s, t.accent);
                        ctx.label(c, &tier.to_string(), tx, cy, 11.0, Weight::Bold, t.on_accent, Align::Center);
                    }
                }
                CompItem::Hero { card_id, name, detail, yours } => {
                    let (ry, rh) = (y + 1.0 * s, h - 1.0 * s);
                    ctx.tile_row(c, card_id, x, ry, iw, rh, base);
                    if *yours {
                        c.rect(x, ry, 3.0 * s, rh, t.gold);
                    }
                    if hover {
                        c.rect(x, ry, iw, rh, t.tint);
                        c.rect(x, ry, 3.0 * s, rh, t.gold);
                    }
                    let dw = ctx.fonts.measure(detail, 10.5 * s, Weight::Bold);
                    let dx = x + iw - dw - 12.0 * s;
                    c.rrect(dx - 6.0 * s, cy - 8.5 * s, dw + 12.0 * s, 17.0 * s, 8.5 * s, Rgba(0, 0, 0, 185));
                    let name = ctx.fonts.fit(name, 12.0 * s, Weight::Medium, dx - x - 18.0 * s);
                    ctx.label_shadow(c, &name, x + 9.0 * s, cy, 12.0, Weight::Medium, if *yours { t.gold } else { t.text });
                    ctx.label(c, detail, dx, cy, 10.5, Weight::Bold, t.on_accent, Align::Left);
                }
                CompItem::Note(text) => {
                    let text = ctx.fonts.fit(text, 11.5 * s, Weight::Medium, iw - 16.0 * s);
                    ctx.label(c, &text, c.width() / 2.0, cy, 11.5, Weight::Medium, t.dim, Align::Center);
                }
                CompItem::Gap => {}
            }
        }
    }
}

// ---- card search ---------------------------------------------------------

#[derive(Debug, Clone, PartialEq)]
pub struct SearchRow {
    pub card_id: String,
    pub name: String,
    /// Type, tier, tribes, cost.
    pub detail: String,
    pub color: Rgba,
    /// Tavern tier badge; 0 for cards without one.
    pub tier: i32,
}

/// What the pointer is over in the search panel.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SearchHit {
    Field,
    /// The "×" that empties the field.
    Clear,
    /// A type chip; `None` is "All".
    Category(Option<Category>),
    Row(usize),
}

/// Text field, card type chips and the cards matching the query.
#[derive(Debug, Clone, PartialEq)]
pub struct SearchPanel {
    pub query: String,
    /// Typing goes to the field (the keyboard is grabbed).
    pub focused: bool,
    pub category: Option<Category>,
    pub rows: Vec<SearchRow>,
    /// Matches left out of `rows`.
    pub more: usize,
    /// Shown instead of rows when there are none.
    pub note: String,
    pub subtitle: String,
    pub scroll: f32,
    pub hover: Option<SearchHit>,
    pub images_rev: u64,
}

impl SearchPanel {
    pub const W: f32 = 280.0;
    const TITLE: f32 = 40.0;
    const FIELD: f32 = 36.0;
    const CHIP_H: f32 = 22.0;
    const CHIP_COLS: usize = 4;
    const GAP: f32 = 4.0;
    const ROW: f32 = 36.0;
    const FOOT: f32 = 26.0;
    const PAD: f32 = 4.0;
    const CLEAR_W: f32 = 28.0;

    fn chips() -> impl Iterator<Item = Option<Category>> {
        std::iter::once(None).chain(Category::ALL.into_iter().map(Some))
    }

    fn header_h() -> f32 {
        let rows = (Category::ALL.len() + 1).div_ceil(Self::CHIP_COLS) as f32;
        Self::TITLE + Self::FIELD + rows * (Self::CHIP_H + Self::GAP) + Self::GAP
    }

    fn field_box() -> Box2 {
        Box2 { x: Self::PAD + 2.0, y: Self::TITLE, w: Self::W - 2.0 * Self::PAD - 4.0, h: Self::FIELD - 6.0 }
    }

    fn chip_box(i: usize) -> Box2 {
        let inner = Self::W - 2.0 * Self::PAD - 4.0;
        let cw = (inner - Self::GAP * (Self::CHIP_COLS - 1) as f32) / Self::CHIP_COLS as f32;
        let (col, row) = (i % Self::CHIP_COLS, i / Self::CHIP_COLS);
        Box2 {
            x: Self::PAD + 2.0 + col as f32 * (cw + Self::GAP),
            y: Self::TITLE + Self::FIELD + Self::GAP / 2.0 + row as f32 * (Self::CHIP_H + Self::GAP),
            w: cw,
            h: Self::CHIP_H,
        }
    }

    fn content_h(&self) -> f32 {
        let rows = if self.rows.is_empty() { Self::ROW } else { self.rows.len() as f32 * Self::ROW };
        Self::header_h() + rows + if self.more > 0 { Self::FOOT } else { 0.0 } + Self::PAD
    }

    pub fn size(&self, s: f32, max_h: i32) -> (i32, i32) {
        (s_ceil(Self::W, s), s_ceil(self.content_h(), s).min(max_h))
    }

    pub fn max_scroll(&self, s: f32, h: i32) -> f32 {
        (self.content_h() * s - h as f32).max(0.0)
    }

    fn row_top(i: usize) -> f32 {
        Self::header_h() + i as f32 * Self::ROW
    }

    pub fn hit(&self, x: f32, y: f32, s: f32) -> Option<SearchHit> {
        let (ux, uy) = (x / s, y / s);
        if uy < Self::TITLE {
            return None;
        }
        if uy < Self::TITLE + Self::FIELD {
            let field = Self::field_box();
            if !field.contains(ux, uy) {
                return None;
            }
            let on_clear = !self.query.is_empty() && ux >= field.x + field.w - Self::CLEAR_W;
            return Some(if on_clear { SearchHit::Clear } else { SearchHit::Field });
        }
        if uy < Self::header_h() {
            return Self::chips().enumerate().find(|(i, _)| Self::chip_box(*i).contains(ux, uy)).map(|(_, k)| SearchHit::Category(k));
        }
        let cy = (y + self.scroll) / s;
        let i = ((cy - Self::header_h()) / Self::ROW).floor();
        (i >= 0.0 && (i as usize) < self.rows.len()).then_some(SearchHit::Row(i as usize))
    }

    pub fn row(&self, i: usize) -> Option<&SearchRow> {
        self.rows.get(i)
    }

    /// Window-local vertical centre of a row.
    pub fn row_center(&self, i: usize, s: f32) -> Option<f32> {
        (i < self.rows.len()).then(|| (Self::row_top(i) + Self::ROW / 2.0) * s - self.scroll)
    }

    /// Scroll offset that brings row `i` into view in a window `h` tall.
    pub fn scroll_to(&self, i: usize, s: f32, h: i32) -> f32 {
        let top = Self::row_top(i) * s;
        let bottom = top + Self::ROW * s;
        let view = h as f32 - Self::header_h() * s;
        let scroll = if top - Self::header_h() * s < self.scroll {
            top - Self::header_h() * s
        } else if bottom - Self::header_h() * s > self.scroll + view {
            bottom - Self::header_h() * s - view
        } else {
            self.scroll
        };
        scroll.clamp(0.0, self.max_scroll(s, h))
    }

    pub fn render(&self, c: &mut Canvas, ctx: &mut Ctx) {
        let s = ctx.s;
        let t = ctx.theme;
        let w = c.width();
        ctx.panel(c);
        self.render_rows(c, ctx);
        // Header drawn last so scrolled rows slide under it.
        let hh = Self::header_h() * s;
        if ctx.opaque {
            c.rect(0.0, 0.0, w, hh, t.bg_opaque);
        } else {
            c.rrect(0.0, 0.0, w, 16.0 * s, 8.0 * s, t.bg_opaque);
            c.rect(0.0, 8.0 * s, w, hh - 8.0 * s, t.bg_opaque);
        }
        ctx.label(c, "Card Search", 12.0 * s, 15.0 * s, 14.0, Weight::Bold, t.gold, Align::Left);
        let subtitle = ctx.fonts.fit(&self.subtitle, 10.0 * s, Weight::Medium, w - 24.0 * s);
        ctx.label(c, &subtitle, 12.0 * s, 31.0 * s, 10.0, Weight::Medium, t.dim, Align::Left);
        self.render_field(c, ctx);
        self.render_chips(c, ctx);
        c.rect(0.0, hh - 1.0, w, 1.0, t.border);
        let max_scroll = self.max_scroll(s, c.height() as i32);
        if max_scroll > 0.0 {
            let track = c.height() - hh - 8.0 * s;
            let thumb = (track * c.height() / (self.content_h() * s)).max(20.0 * s);
            let ty = hh + 4.0 * s + (track - thumb) * (self.scroll / max_scroll);
            c.rrect(w - 4.0 * s, ty, 2.5 * s, thumb, 1.5 * s, t.scrollbar);
        }
    }

    fn render_field(&self, c: &mut Canvas, ctx: &mut Ctx) {
        let s = ctx.s;
        let t = ctx.theme;
        let (x, y, w, h) = Self::field_box().scaled(s);
        let cy = y + h / 2.0;
        let hover = matches!(self.hover, Some(SearchHit::Field | SearchHit::Clear));
        c.rrect(x, y, w, h, 5.0 * s, if hover && !self.focused { t.button_hover } else { t.field });
        let border = if self.focused { t.accent } else { t.border };
        c.stroke_rrect(x, y, w, h, 5.0 * s, if self.focused { 1.5 * s } else { 1.0 }, border);
        // Magnifier: a ring and its handle.
        let (mx, my, r) = (x + 13.0 * s, cy - 1.5 * s, 4.5 * s);
        c.stroke_rrect(mx - r, my - r, 2.0 * r, 2.0 * r, r, 1.6 * s, t.dim);
        let (hx, hy, handle) = (mx + r * 0.7, my + r * 0.7, 1.1 * s);
        c.triangle([(hx - handle, hy + handle), (hx + handle, hy - handle), (hx + 4.0 * s, hy + 4.0 * s)], t.dim);
        c.triangle([(hx - handle, hy + handle), (hx + 4.0 * s, hy + 4.0 * s), (hx + 4.0 * s - handle, hy + 4.0 * s + handle)], t.dim);
        let tx = x + 26.0 * s;
        let room = w - 26.0 * s - if self.query.is_empty() { 8.0 * s } else { Self::CLEAR_W * s };
        let px = 12.5 * s;
        if self.query.is_empty() {
            let hint = if self.focused { "Type a card name…" } else { "Click to search cards" };
            ctx.label(c, hint, tx, cy, 12.0, Weight::Medium, t.dim.alpha(200), Align::Left);
        } else {
            // Keep the end of a long query, where the caret is.
            let mut shown = self.query.clone();
            while ctx.fonts.measure(&shown, px, Weight::Medium) > room - 4.0 * s && !shown.is_empty() {
                shown.remove(0);
            }
            let text = if shown.len() < self.query.len() { format!("…{shown}") } else { shown };
            let tw = ctx.fonts.measure(&text, px, Weight::Medium);
            ctx.label(c, &text, tx, cy, 12.5, Weight::Medium, t.text, Align::Left);
            if self.focused {
                c.rect(tx + tw + 1.5 * s, cy - 8.0 * s, 1.5 * s, 16.0 * s, t.text);
            }
            let cx = x + w - Self::CLEAR_W * s / 2.0;
            if self.hover == Some(SearchHit::Clear) {
                c.circle(cx, cy, 8.5 * s, t.tint);
            }
            ctx.label(c, "×", cx, cy, 15.0, Weight::Bold, t.text, Align::Center);
            return;
        }
        if self.focused {
            c.rect(tx - 1.0 * s, cy - 8.0 * s, 1.5 * s, 16.0 * s, t.text);
        }
    }

    fn render_chips(&self, c: &mut Canvas, ctx: &mut Ctx) {
        let s = ctx.s;
        let t = ctx.theme;
        for (i, k) in Self::chips().enumerate() {
            let (cx, cy, cw, ch) = Self::chip_box(i).scaled(s);
            let selected = k == self.category;
            let hover = self.hover == Some(SearchHit::Category(k));
            let fill = if selected {
                t.accent
            } else if hover {
                t.button_hover
            } else {
                t.button
            };
            c.rrect(cx, cy, cw, ch, 4.0 * s, fill);
            let label = k.map(Category::label).unwrap_or("All");
            let color = if selected {
                t.on_accent
            } else if hover {
                t.text
            } else {
                t.soft
            };
            ctx.label(c, label, cx + cw / 2.0, cy + ch / 2.0, 10.0, Weight::Bold, color, Align::Center);
        }
    }

    fn render_rows(&self, c: &mut Canvas, ctx: &mut Ctx) {
        let s = ctx.s;
        let t = ctx.theme;
        let x = Self::PAD * s;
        let iw = c.width() - 2.0 * Self::PAD * s;
        let base = if ctx.opaque { t.bg_opaque } else { t.row };
        let top = Self::header_h() * s - self.scroll;
        if self.rows.is_empty() {
            let note = ctx.fonts.fit(&self.note, 11.5 * s, Weight::Medium, iw - 16.0 * s);
            ctx.label(c, &note, c.width() / 2.0, top + Self::ROW * s / 2.0, 11.5, Weight::Medium, t.dim, Align::Center);
            return;
        }
        let rh = Self::ROW * s;
        for (i, row) in self.rows.iter().enumerate() {
            let y = top + i as f32 * rh;
            if y + rh <= 0.0 || y >= c.height() {
                continue;
            }
            let (ry, h) = (y + 1.0 * s, rh - 1.0 * s);
            let cy = ry + h / 2.0;
            ctx.tile_row(c, &row.card_id, x, ry, iw, h, base);
            c.rect(x, ry, 4.0 * s, h, row.color);
            if self.hover == Some(SearchHit::Row(i)) {
                c.rect(x, ry, iw, h, t.tint);
                c.rect(x, ry, 3.0 * s, h, t.gold);
            }
            let max = iw - if row.tier > 0 { 40.0 * s } else { 18.0 * s };
            let name = ctx.fonts.fit(&row.name, 12.5 * s, Weight::Bold, max);
            ctx.label_shadow(c, &name, x + 11.0 * s, cy - 7.0 * s, 12.5, Weight::Bold, t.text);
            let detail = ctx.fonts.fit(&row.detail, 10.0 * s, Weight::Medium, max);
            ctx.label_shadow(c, &detail, x + 11.0 * s, cy + 8.0 * s, 10.0, Weight::Medium, t.soft);
            if row.tier > 0 {
                let tx = x + iw - 14.0 * s;
                c.circle(tx, cy, 9.5 * s, Rgba(0, 0, 0, 170));
                c.circle(tx, cy, 8.5 * s, t.accent);
                ctx.label(c, &row.tier.to_string(), tx, cy, 11.0, Weight::Bold, t.on_accent, Align::Center);
            }
        }
        if self.more > 0 {
            let y = top + self.rows.len() as f32 * rh + Self::FOOT * s / 2.0;
            let msg = format!("+{} more · keep typing to narrow down", self.more);
            let msg = ctx.fonts.fit(&msg, 10.5 * s, Weight::Medium, iw - 16.0 * s);
            ctx.label(c, &msg, c.width() / 2.0, y, 10.5, Weight::Medium, t.dim, Align::Center);
        }
    }
}

// ---- combat odds ---------------------------------------------------------

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct OddsView {
    pub won_lethal: f32,
    pub won: f32,
    pub tied: f32,
    pub lost: f32,
    pub lost_lethal: f32,
}

#[derive(Debug, Clone, PartialEq)]
pub struct SimPanel {
    pub title: String,
    pub odds: Option<OddsView>,
    /// Not the combat on screen (shop phase): faded.
    pub dim: bool,
    pub running: bool,
    pub note: Option<String>,
}

/// Rounds the way players read odds: never show 0%/100% for something that
/// can still happen.
pub fn percent(v: f32) -> String {
    if v <= 0.0 {
        "0%".into()
    } else if v < 1.0 {
        "<1%".into()
    } else if v >= 100.0 {
        "100%".into()
    } else if v > 99.0 {
        ">99%".into()
    } else {
        format!("{:.0}%", v)
    }
}

impl SimPanel {
    const W: f32 = 340.0;
    const H: f32 = 72.0;

    pub fn size(&self, s: f32) -> (i32, i32) {
        (s_ceil(Self::W, s), s_ceil(Self::H, s))
    }

    pub fn render(&self, c: &mut Canvas, ctx: &mut Ctx) {
        let s = ctx.s;
        let t = ctx.theme;
        ctx.panel(c);
        let col_w = Self::W * s / 5.0;
        let fade = |c: Rgba, dim: bool| if dim { c.alpha(150) } else { c };
        let cols: [(&str, Option<f32>, Rgba); 5] = [
            ("LETHAL", self.odds.map(|o| o.won_lethal), t.win_lethal),
            ("WIN", self.odds.map(|o| o.won), t.win),
            ("TIE", self.odds.map(|o| o.tied), t.tie),
            ("LOSS", self.odds.map(|o| o.lost), t.loss),
            ("LETHAL", self.odds.map(|o| o.lost_lethal), t.loss_lethal),
        ];
        for (i, (label, value, color)) in cols.iter().enumerate() {
            let cx = col_w * (i as f32 + 0.5);
            let lethal = i == 0 || i == 4;
            let label_color = if lethal { t.dim.alpha(170) } else { t.dim };
            ctx.label(c, label, cx, 13.0 * s, 9.5, Weight::Bold, fade(label_color, self.dim), Align::Center);
            let text = value.map(percent).unwrap_or_else(|| "–".into());
            let zero = value.is_none_or(|v| v <= 0.0);
            let vc = if lethal && zero { t.dim } else { *color };
            let px = if lethal { 15.0 } else { 19.0 };
            ctx.label(c, &text, cx, 33.0 * s, px, Weight::Bold, fade(vc, self.dim), Align::Center);
        }
        let footer = match (&self.note, self.running) {
            (Some(note), _) => note.clone(),
            (None, true) => format!("{} · simulating…", self.title),
            (None, false) => self.title.clone(),
        };
        let max = Self::W * s - 16.0 * s;
        let footer = ctx.fonts.fit(&footer, 10.5 * s, Weight::Medium, max);
        let fc = if self.note.is_some() { t.loss.alpha(220) } else { t.dim };
        ctx.label(c, &footer, Self::W * s / 2.0, 52.0 * s, 10.5, Weight::Medium, fc, Align::Center);
        if let Some(o) = self.odds {
            let (x0, y, w, h) = (10.0 * s, (Self::H - 9.0) * s, (Self::W - 20.0) * s, 3.0 * s);
            let total = (o.won + o.tied + o.lost).max(0.001);
            let mut x = x0;
            for (v, col) in [(o.won, t.win), (o.tied, t.tie), (o.lost, t.loss)] {
                let seg = w * v / total;
                if seg > 0.0 {
                    c.rect(x, y, seg, h, fade(col, self.dim));
                }
                x += seg;
            }
        }
    }
}

// ---- lobby tribes and recent games ---------------------------------------

#[derive(Debug, Clone, PartialEq)]
pub struct GameRow {
    pub hero_card_id: String,
    pub hero_name: String,
    pub place: i32,
    pub mmr_delta: Option<i32>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct InfoPanel {
    pub tribes: Vec<Tribe>,
    pub slots: usize,
    pub start_mmr: Option<i32>,
    pub current_mmr: Option<i32>,
    pub games: Vec<GameRow>,
    /// Unscaled width chosen by the layout.
    pub width: f32,
}

pub fn place_color(place: i32, theme: &Theme) -> Rgba {
    match place {
        1 => BADGE_GOLD,
        2..=4 => theme.win,
        _ => theme.loss,
    }
}

pub fn ordinal(place: i32) -> String {
    match place {
        1 => "1st".to_string(),
        2 => "2nd".to_string(),
        3 => "3rd".to_string(),
        4 => "4th".to_string(),
        5 => "5th".to_string(),
        6 => "6th".to_string(),
        7 => "7th".to_string(),
        8 => "8th".to_string(),
        n => format!("{n}th"),
    }
}

pub fn format_thousands(n: i32) -> String {
    let s = n.abs().to_string();
    let mut out = String::new();
    let len = s.len();
    for (i, ch) in s.chars().enumerate() {
        if i > 0 && (len - i).is_multiple_of(3) {
            out.push(',');
        }
        out.push(ch);
    }
    if n < 0 {
        format!("-{out}")
    } else {
        out
    }
}

impl InfoPanel {
    pub const HEAD_H: f32 = 18.0;
    pub const MINIONS_BODY_H: f32 = 56.0;
    pub const MMR_BODY_H: f32 = 40.0;
    pub const GAMES_COLS_H: f32 = 16.0;
    pub const GAME_ROW_H: f32 = 24.0;

    fn height(&self) -> f32 {
        let games_count = self.games.len().max(1) as f32;
        Self::HEAD_H + Self::MINIONS_BODY_H
            + Self::HEAD_H + Self::MMR_BODY_H
            + Self::HEAD_H + Self::GAMES_COLS_H + games_count * Self::GAME_ROW_H
    }

    pub fn size(&self, s: f32) -> (i32, i32) {
        (s_ceil(self.width, s), s_ceil(self.height(), s))
    }

    pub fn render(&self, c: &mut Canvas, ctx: &mut Ctx) {
        let s = ctx.s;
        let t = ctx.theme;
        ctx.panel(c);
        let w = self.width * s;
        let mut y = 0.0;

        // Section header helper: dark background strip with centered bold text.
        let render_header = |c: &mut Canvas, ctx: &mut Ctx, title: &str, top: f32| {
            let h = Self::HEAD_H * s;
            c.rect(0.0, top, w, h, t.header);
            c.rect(0.0, top + h - 1.0 * s, w, 1.0 * s, t.border);
            ctx.label_shadow_align(c, title, w / 2.0, top + h / 2.0, 11.0, Weight::Bold, t.text, Align::Center);
        };

        // 1. Available Minions
        render_header(c, ctx, "Available Minions", y);
        y += Self::HEAD_H * s;

        let tribe_slots = self.slots.max(5);
        let col_w = w / tribe_slots as f32;
        let circle_r = 13.5 * s;
        let circle_cy = y + 17.5 * s;

        for i in 0..tribe_slots {
            let cx = (i as f32 + 0.5) * col_w;
            match self.tribes.get(i) {
                Some(&tribe) => {
                    c.circle(cx, circle_cy, circle_r, Rgba(18, 22, 26, 255));
                    if let Some(icon) = ctx.images.tribe_icon(tribe) {
                        c.image_cover(&icon, cx - circle_r, circle_cy - circle_r, circle_r * 2.0, circle_r * 2.0, circle_r, 1.0);
                    }
                    c.stroke_circle(cx, circle_cy, circle_r, 1.5 * s, Rgba(46, 139, 87, 255));
                    let label = tribe.label();
                    let fit_label = ctx.fonts.fit(label, 9.5 * s, Weight::Bold, col_w - 2.0 * s);
                    ctx.label_shadow_align(c, &fit_label, cx, circle_cy + circle_r + 8.5 * s, 9.5, Weight::Bold, t.text, Align::Center);
                }
                None => {
                    c.circle(cx, circle_cy, circle_r, t.button);
                    c.stroke_circle(cx, circle_cy, circle_r, 1.0 * s, t.border);
                    ctx.label(c, "?", cx, circle_cy, 12.0, Weight::Bold, t.dim, Align::Center);
                }
            }
        }
        y += Self::MINIONS_BODY_H * s;

        // 2. MMR
        render_header(c, ctx, "MMR", y);
        y += Self::HEAD_H * s;

        // Vertical divider between Start and Current
        c.rect(w * 0.5, y + 4.0 * s, 1.0 * s, (Self::MMR_BODY_H - 8.0) * s, t.border);

        let sub_y = y + 10.0 * s;
        let val_y = y + 26.5 * s;
        let col1_x = w * 0.25;
        let col2_x = w * 0.75;

        ctx.label(c, "Start", col1_x, sub_y, 10.5, Weight::Medium, t.dim, Align::Center);
        ctx.label(c, "Current", col2_x, sub_y, 10.5, Weight::Medium, t.dim, Align::Center);

        let start_str = self.start_mmr.map(format_thousands).unwrap_or_else(|| "-".to_string());
        let current_str = self.current_mmr.map(format_thousands).unwrap_or_else(|| "-".to_string());

        ctx.label_shadow_align(c, &start_str, col1_x, val_y, 14.5, Weight::Bold, t.text, Align::Center);
        ctx.label_shadow_align(c, &current_str, col2_x, val_y, 14.5, Weight::Bold, t.text, Align::Center);

        y += Self::MMR_BODY_H * s;

        // 3. Latest Games
        render_header(c, ctx, "Latest Games", y);
        y += Self::HEAD_H * s;

        let ch_y = y + Self::GAMES_COLS_H * s / 2.0;
        ctx.label(c, "Hero", 8.0 * s, ch_y, 10.5, Weight::Medium, t.dim, Align::Left);
        let place_x = w * 0.62;
        let mmr_x = w * 0.86;
        ctx.label(c, "Place", place_x, ch_y, 10.5, Weight::Medium, t.dim, Align::Center);
        ctx.label(c, "MMR", mmr_x, ch_y, 10.5, Weight::Medium, t.dim, Align::Center);

        c.rect(0.0, y + Self::GAMES_COLS_H * s - 1.0 * s, w, 1.0 * s, t.border.alpha(120));
        y += Self::GAMES_COLS_H * s;

        if self.games.is_empty() {
            let row_h = Self::GAME_ROW_H * s;
            ctx.label(c, "No games yet", 8.0 * s, y + row_h / 2.0, 11.0, Weight::Medium, t.dim, Align::Left);
        } else {
            for game in &self.games {
                let row_h = Self::GAME_ROW_H * s;
                let mid_y = y + row_h / 2.0;

                // Hero tile art on left with fade
                if ctx.compact != CompactMode::Text {
                    if let Some(img) = ctx.images.get(ImageKind::Tile, &game.hero_card_id) {
                        let cap = match ctx.compact {
                            CompactMode::Small => 0.25,
                            _ => 0.46,
                        };
                        let tw = (row_h * img.width() as f32 / img.height() as f32).min(w * cap);
                        c.image_cover(&img, 0.0, y, tw, row_h, 0.0, 1.0);
                        c.hgradient(tw * 0.35, y, tw * 0.65, row_h, t.bg.alpha(0), t.bg);
                    }
                }

                // Hero name
                let name = ctx.fonts.fit(&game.hero_name, 11.0 * s, Weight::Bold, place_x - 14.0 * s);
                ctx.label_shadow_align(c, &name, 8.0 * s, mid_y, 11.0, Weight::Bold, t.text, Align::Left);

                // Placement (ordinal: 1st, 2nd, etc.)
                let ord = ordinal(game.place);
                let p_color = place_color(game.place, &t);
                ctx.label_shadow_align(c, &ord, place_x, mid_y, 11.5, Weight::Bold, p_color, Align::Center);

                // MMR Delta (+35, -44, or -)
                let (delta_str, delta_color) = match game.mmr_delta {
                    Some(d) => match d.cmp(&0) {
                        std::cmp::Ordering::Greater => (format!("+{d}"), t.win),
                        std::cmp::Ordering::Less => (d.to_string(), t.loss),
                        std::cmp::Ordering::Equal => ("0".to_string(), t.dim),
                    },
                    None => ("-".to_string(), t.dim),
                };
                ctx.label_shadow_align(c, &delta_str, mmr_x, mid_y, 11.5, Weight::Bold, delta_color, Align::Center);

                // Divider line below row
                c.rect(0.0, y + row_h - 1.0 * s, w, 1.0 * s, t.border.alpha(70));
                y += row_h;
            }
        }
    }
}

// ---- opponent board popup ------------------------------------------------

#[derive(Debug, Clone, PartialEq)]
pub struct MinionView {
    pub card_id: String,
    pub name: String,
    pub attack: i32,
    pub health: i32,
    pub golden: bool,
    pub keywords: Vec<&'static str>,
}

/// One combat's worth of an opponent's board.
#[derive(Debug, Clone, PartialEq)]
pub struct BoardView {
    pub turn: i32,
    pub tier: i32,
    pub minions: Vec<MinionView>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct OpponentPopup {
    pub lobby_id: i32,
    pub hero: String,
    pub hero_card_id: String,
    pub place: i32,
    pub health: i32,
    pub turn: i32,
    /// Every board faced, newest first; one toggle chip each.
    pub boards: Vec<BoardView>,
    /// Which of `boards` are toggled on.
    pub shown: Vec<bool>,
    /// Oldest toggled-on boards left out because they did not fit.
    pub hidden: usize,
    /// Chip under the pointer.
    pub hover: Option<usize>,
    pub images_rev: u64,
    /// How the minion portraits are drawn; shrinks the whole board.
    pub compact: CompactMode,
}

impl OpponentPopup {
    const HEAD: f32 = 52.0;
    const CHIP_COLS: usize = 10;
    const CHIP_H: f32 = 20.0;
    const GAP: f32 = 4.0;
    const BOARD_HEAD: f32 = 22.0;
    /// A board laid out like in game: up to seven minions side by side.
    const SLOTS: usize = 7;
    const MINION_GAP: f32 = 6.0;
    /// Portrait width at the full size; the other modes scale off it.
    const MINION_W: f32 = 64.0;
    const ROW: f32 = 32.0;
    const MORE: f32 = 20.0;
    const PAD: f32 = 6.0;

    /// Width of one minion slot; text mode keeps it for the name and gems.
    fn minion_w(&self) -> f32 {
        match self.compact {
            CompactMode::Small => 40.0,
            _ => Self::MINION_W,
        }
    }

    /// Height of a minion portrait; none in text mode.
    fn portrait_h(&self) -> f32 {
        match self.compact {
            CompactMode::Small => 50.0,
            CompactMode::Text => 0.0,
            CompactMode::Full => 82.0,
        }
    }

    /// Portrait, the stat gems hanging below it and a two-line name; in text
    /// mode the gems and name alone, without any art.
    fn minions_h(&self) -> f32 {
        match self.compact {
            CompactMode::Text => 6.0 + 48.0,
            _ => 8.0 + self.portrait_h() + 8.0 + 24.0,
        }
    }

    fn w(&self) -> f32 {
        Self::PAD * 2.0 + Self::SLOTS as f32 * self.minion_w() + (Self::SLOTS - 1) as f32 * Self::MINION_GAP
    }

    fn chips_h(&self) -> f32 {
        if self.boards.is_empty() {
            return 0.0;
        }
        self.boards.len().div_ceil(Self::CHIP_COLS) as f32 * (Self::CHIP_H + Self::GAP) + Self::GAP
    }

    fn chip_box(&self, i: usize) -> Box2 {
        let inner = self.w() - 2.0 * Self::PAD;
        let cw = (inner - Self::GAP * (Self::CHIP_COLS - 1) as f32) / Self::CHIP_COLS as f32;
        let (col, row) = (i % Self::CHIP_COLS, i / Self::CHIP_COLS);
        Box2 {
            x: Self::PAD + col as f32 * (cw + Self::GAP),
            y: Self::HEAD + row as f32 * (Self::CHIP_H + Self::GAP),
            w: cw,
            h: Self::CHIP_H,
        }
    }

    /// Toggled-on boards that fit, newest first.
    fn visible(&self) -> impl Iterator<Item = (usize, &BoardView)> {
        let on = self.shown.iter().filter(|s| **s).count();
        self.boards.iter().enumerate().filter(|(i, _)| self.shown[*i]).take(on.saturating_sub(self.hidden))
    }

    fn board_h(&self, b: &BoardView) -> f32 {
        Self::BOARD_HEAD + if b.minions.is_empty() { Self::ROW } else { self.minions_h() }
    }

    fn content_h(&self) -> f32 {
        let boards = self.visible().map(|(_, b)| self.board_h(b)).sum::<f32>();
        // Nothing toggled on: room for the hint.
        let boards = if boards > 0.0 { boards } else { Self::ROW };
        let more = if self.hidden > 0 { Self::MORE } else { 0.0 };
        Self::HEAD + self.chips_h() + boards + more + Self::PAD
    }

    /// Leaves out the oldest toggled-on boards until the popup fits in
    /// `max_h` pixels, always keeping the newest one.
    pub fn fit(&mut self, s: f32, max_h: i32) {
        while self.visible().count() > 1 && s_ceil(self.content_h(), s) > max_h {
            self.hidden += 1;
        }
    }

    pub fn size(&self, s: f32) -> (i32, i32) {
        (s_ceil(self.w(), s), s_ceil(self.content_h(), s))
    }

    /// Turn chip at a point.
    pub fn hit(&self, x: f32, y: f32, s: f32) -> Option<usize> {
        (0..self.boards.len()).find(|&i| self.chip_box(i).contains(x / s, y / s))
    }

    pub fn render(&self, c: &mut Canvas, ctx: &mut Ctx) {
        let s = ctx.s;
        let t = ctx.theme;
        ctx.panel(c);
        let w = c.width();
        let x = Self::PAD * s;
        let iw = w - 2.0 * Self::PAD * s;
        // Header: hero art strip behind the name.
        let hh = (Self::HEAD - Self::PAD) * s;
        ctx.tile_row(c, &self.hero_card_id, x, x, iw, hh, t.header);
        let name = ctx.fonts.fit(&self.hero, 14.0 * s, Weight::Bold, iw - 20.0 * s);
        ctx.label_shadow(c, &name, x + 10.0 * s, x + 15.0 * s, 14.0, Weight::Bold, t.text);
        let faced = self.boards.len();
        let sub = match self.boards.first() {
            Some(b) => format!("Tier {} · faced {faced} time{}", b.tier, if faced == 1 { "" } else { "s" }),
            None => "Not faced yet".to_string(),
        };
        let sub = ctx.fonts.fit(&sub, 11.0 * s, Weight::Medium, iw - 20.0 * s);
        ctx.label_shadow(c, &sub, x + 10.0 * s, x + 34.0 * s, 11.0, Weight::Medium, t.dim);
        let mut y = Self::HEAD * s;
        if self.boards.is_empty() {
            ctx.label(c, "No board seen yet", w / 2.0, y + Self::ROW * s / 2.0, 12.0, Weight::Medium, t.dim, Align::Center);
            return;
        }
        self.render_chips(c, ctx);
        y += self.chips_h() * s;
        let mut any = false;
        for (i, b) in self.visible() {
            y = self.render_board(c, ctx, b, i == 0, y);
            any = true;
        }
        if !any {
            ctx.label(c, "Click a turn to show its board", w / 2.0, y + Self::ROW * s / 2.0, 11.5, Weight::Medium, t.dim, Align::Center);
        }
        if self.hidden > 0 {
            let msg = format!("+{} older board{} not shown", self.hidden, if self.hidden == 1 { "" } else { "s" });
            ctx.label(c, &msg, w / 2.0, y + Self::MORE * s / 2.0, 10.5, Weight::Medium, t.dim, Align::Center);
        }
    }

    fn render_chips(&self, c: &mut Canvas, ctx: &mut Ctx) {
        let s = ctx.s;
        let t = ctx.theme;
        for (i, b) in self.boards.iter().enumerate() {
            let (cx, cy, cw, ch) = self.chip_box(i).scaled(s);
            let on = self.shown[i];
            let hover = self.hover == Some(i);
            let fill = match (on, hover) {
                (true, _) => t.accent,
                (false, true) => t.button_hover,
                (false, false) => t.button,
            };
            c.rrect(cx, cy, cw, ch, 4.0 * s, fill);
            if hover && on {
                c.rrect(cx, cy, cw, ch, 4.0 * s, t.tint);
            }
            // The latest board gets the same gold mark as its header.
            if i == 0 {
                c.rect(cx + 3.0 * s, cy + ch - 3.0 * s, cw - 6.0 * s, 1.5 * s, t.gold);
            }
            let color = if on {
                t.on_accent
            } else if hover {
                t.text
            } else {
                t.soft
            };
            ctx.label(c, &format!("T{}", b.turn), cx + cw / 2.0, cy + ch / 2.0, 10.5, Weight::Bold, color, Align::Center);
        }
    }

    fn render_board(&self, c: &mut Canvas, ctx: &mut Ctx, b: &BoardView, latest: bool, mut y: f32) -> f32 {
        let s = ctx.s;
        let t = ctx.theme;
        let x = Self::PAD * s;
        let iw = c.width() - 2.0 * Self::PAD * s;
        let w = c.width();
        // Board header: turn, tier and how long ago.
        let bh = (Self::BOARD_HEAD - 2.0) * s;
        let by = y + 1.0 * s;
        c.rect(x, by, iw, bh, t.header);
        c.rect(x, by, 3.0 * s, bh, if latest { t.gold } else { t.dim });
        let ago = self.turn - b.turn;
        let when = match ago {
            0 => "this turn".to_string(),
            1 => "1 turn ago".to_string(),
            n => format!("{n} turns ago"),
        };
        let title = format!("Turn {} · Tier {}", b.turn, b.tier);
        ctx.label(c, &title, x + 9.0 * s, by + bh / 2.0, 11.5, Weight::Bold, if latest { t.text } else { t.soft }, Align::Left);
        ctx.label(c, &when, x + iw - 7.0 * s, by + bh / 2.0, 10.5, Weight::Medium, t.dim, Align::Right);
        y += Self::BOARD_HEAD * s;
        if b.minions.is_empty() {
            ctx.label(c, "Empty board", w / 2.0, y + Self::ROW * s / 2.0, 12.0, Weight::Medium, t.dim, Align::Center);
            return y + Self::ROW * s;
        }
        // Centred like the game centres a board that is not full.
        let mw = self.minion_w();
        let n = b.minions.len().min(Self::SLOTS) as f32;
        let row_w = n * mw + (n - 1.0) * Self::MINION_GAP;
        let x0 = (self.w() - row_w) / 2.0;
        let top = if self.compact == CompactMode::Text { 6.0 } else { 8.0 };
        for (i, m) in b.minions.iter().take(Self::SLOTS).enumerate() {
            let mx = (x0 + i as f32 * (mw + Self::MINION_GAP)) * s;
            self.render_minion(c, ctx, m, mx, y + top * s);
        }
        y + self.minions_h() * s
    }

    /// One minion as on the game board: portrait, attack bottom left,
    /// health bottom right, keyword effects around it, name below. Text mode
    /// keeps the stats, keywords and name but drops the art entirely.
    fn render_minion(&self, c: &mut Canvas, ctx: &mut Ctx, m: &MinionView, x: f32, y: f32) {
        let s = ctx.s;
        let t = ctx.theme;
        let mw = self.minion_w();
        let has = |k: &str| m.keywords.contains(&k);
        // Letters for the keywords: every one of them in text mode (there is
        // no art to hint at them), only the ones without a look otherwise.
        let letters: &[(&str, &str, Rgba)] = if self.compact == CompactMode::Text {
            &[
                ("Taunt", "T", Rgba(132, 136, 148, 255)),
                ("Shield", "D", Rgba(255, 214, 90, 255)),
                ("Reborn", "R", Rgba(110, 220, 255, 255)),
                ("Stealth", "S", Rgba(120, 90, 180, 255)),
                ("Venomous", "V", Rgba(70, 170, 60, 255)),
                ("Windfury", "W", Rgba(70, 150, 220, 255)),
                ("Mega-Windfury", "M", Rgba(70, 110, 230, 255)),
            ]
        } else {
            &[
                ("Venomous", "V", Rgba(70, 170, 60, 255)),
                ("Windfury", "W", Rgba(70, 150, 220, 255)),
                ("Mega-Windfury", "M", Rgba(70, 110, 230, 255)),
            ]
        };
        let badges: Vec<(&str, Rgba)> =
            letters.iter().filter(|(k, ..)| has(k)).map(|(_, l, col)| (*l, *col)).collect();
        if self.compact == CompactMode::Text {
            let (w, h) = (mw * s, 58.0 * s);
            let r = 5.0 * s;
            c.rrect(x, y, w, h, r, Rgba(30, 34, 48, 255));
            let frame = if m.golden { t.gold } else { Rgba(96, 84, 70, 255) };
            c.stroke_rrect(x, y, w, h, r, 1.0, frame);
            // Gems at the top corners, where the game hangs them.
            let gy = y + 12.0 * s;
            for (cx, value, color) in [(x + 10.0 * s, m.attack, t.attack), (x + w - 10.0 * s, m.health, t.health)] {
                c.circle(cx, gy, 8.5 * s, Rgba(0, 0, 0, 220));
                c.circle(cx, gy, 7.0 * s, color);
                let text = value.to_string();
                let px = if text.len() >= 3 { 8.0 } else { 10.0 };
                c.text_shadow(ctx.fonts, &text, cx, ctx.mid(gy, px * s, Weight::Bold), px * s, Weight::Bold, t.on_accent, Align::Center, Rgba(0, 0, 0, 200));
            }
            // Keyword letters on their own row under the gems.
            let bw = 11.0 * s;
            let bx0 = x + w / 2.0 - (badges.len() as f32 * bw) / 2.0 + bw / 2.0;
            for (i, (label, col)) in badges.iter().enumerate() {
                let (cx, cy) = (bx0 + i as f32 * bw, y + 27.0 * s);
                c.circle(cx, cy, 4.5 * s, Rgba(0, 0, 0, 200));
                c.circle(cx, cy, 3.5 * s, *col);
                ctx.label(c, label, cx, cy, 7.0, Weight::Bold, t.on_accent, Align::Center);
            }
            let lines = ctx.fonts.wrap(&m.name, 9.0 * s, Weight::Bold, w - 4.0 * s);
            let name_color = if m.golden { t.gold } else { t.text };
            for (i, line) in lines.iter().take(2).enumerate() {
                let cy = y + 41.0 * s + i as f32 * 10.0 * s;
                ctx.label(c, line, x + w / 2.0, cy, 9.0, Weight::Bold, name_color, Align::Center);
            }
            return;
        }
        let (w, h) = (mw * s, self.portrait_h() * s);
        let r = w / 2.0;
        // Everything around the art shrinks with it in "small" mode.
        let k = mw / Self::MINION_W;
        if has("Taunt") {
            // Stone shield behind the portrait.
            let (tx, ty, tw, th) = (x - 4.0 * k * s, y - 3.0 * k * s, w + 8.0 * k * s, h + 5.0 * k * s);
            c.rrect(tx, ty, tw, th, 12.0 * k * s, Rgba(132, 136, 148, 255));
            c.stroke_rrect(tx, ty, tw, th, 12.0 * k * s, 1.5 * s, Rgba(60, 62, 72, 255));
        }
        c.rrect(x, y, w, h, r, Rgba(30, 34, 48, 255));
        if let Some(img) = ctx.images.get(ImageKind::Portrait, &m.card_id) {
            c.image_cover(&img, x, y, w, h, r, 1.0);
        } else if let Some(img) = ctx.images.get(ImageKind::Tile, &m.card_id) {
            c.image_cover(&img, x, y, w, h, r, 1.0);
        }
        if has("Stealth") {
            c.rrect(x, y, w, h, r, Rgba(20, 10, 40, 110));
        }
        if has("Reborn") {
            c.rrect(x, y, w, h, r, Rgba(110, 220, 255, 60));
        }
        let frame = if m.golden { t.gold } else { Rgba(96, 84, 70, 255) };
        c.stroke_rrect(x, y, w, h, r, 2.0 * k * s, frame);
        if has("Shield") {
            c.stroke_rrect(
                x - 2.5 * k * s,
                y - 2.5 * k * s,
                w + 5.0 * k * s,
                h + 5.0 * k * s,
                r + 2.5 * k * s,
                3.0 * k * s,
                Rgba(255, 214, 90, 210),
            );
        }
        let bw = 14.0 * k * s;
        let bx0 = x + w / 2.0 - (badges.len() as f32 * bw) / 2.0 + bw / 2.0;
        for (i, (label, col)) in badges.iter().enumerate() {
            let (cx, cy) = (bx0 + i as f32 * bw, y + 4.0 * k * s);
            c.circle(cx, cy, 6.5 * k * s, Rgba(0, 0, 0, 200));
            c.circle(cx, cy, 5.5 * k * s, *col);
            ctx.label(c, label, cx, cy, (8.0 * k).max(7.0), Weight::Bold, t.on_accent, Align::Center);
        }
        // Stats where the game puts them.
        let gy = y + h - 6.0 * k * s;
        for (cx, value, color) in [(x + 9.0 * k * s, m.attack, t.attack), (x + w - 9.0 * k * s, m.health, t.health)] {
            c.circle(cx, gy, 11.5 * k * s, Rgba(0, 0, 0, 220));
            c.circle(cx, gy, 10.0 * k * s, color);
            let text = value.to_string();
            let px = match text.len() {
                1 | 2 => 12.5,
                3 => 10.0,
                _ => 8.5,
            } * k;
            // The gems are always bright, so their number keeps a dark shadow.
            c.text_shadow(ctx.fonts, &text, cx, ctx.mid(gy, px * s, Weight::Bold), px * s, Weight::Bold, t.on_accent, Align::Center, Rgba(0, 0, 0, 200));
        }
        let lines = ctx.fonts.wrap(&m.name, 9.5 * k * s, Weight::Bold, w + Self::MINION_GAP * k * s - 2.0 * s);
        let name_color = if m.golden { t.gold } else { t.text };
        for (i, line) in lines.iter().take(2).enumerate() {
            let cy = y + h + (15.0 + i as f32 * 11.0) * k * s;
            ctx.label(c, line, x + w / 2.0, cy, 9.5 * k, Weight::Bold, name_color, Align::Center);
        }
    }
}

// ---- card preview --------------------------------------------------------

#[derive(Debug, Clone, PartialEq)]
pub struct CardPreview {
    pub card_id: String,
    pub images_rev: u64,
    /// "Small" mode shows a narrower card; text mode never builds a preview.
    pub compact: CompactMode,
}

impl CardPreview {
    /// Preview width; the height follows the card's aspect ratio.
    fn w(&self) -> f32 {
        match self.compact {
            CompactMode::Small => 150.0,
            _ => 230.0,
        }
    }

    /// Size once the image has loaded.
    pub fn size(&self, s: f32, images: &mut Images) -> Option<(i32, i32)> {
        let img = images.get(ImageKind::Render, &self.card_id)?;
        let w = self.w() * s;
        Some((w.ceil() as i32, (w * img.height() as f32 / img.width() as f32).ceil() as i32))
    }

    pub fn render(&self, c: &mut Canvas, ctx: &mut Ctx) {
        if let Some(img) = ctx.images.get(ImageKind::Render, &self.card_id) {
            if ctx.opaque {
                c.rect(0.0, 0.0, c.width(), c.height(), ctx.theme.bg_opaque);
            }
            let (w, h) = (c.width(), c.height());
            c.image_cover(&img, 0.0, 0.0, w, h, 0.0, 1.0);
        }
    }
}

// ---- leaderboard calibration ---------------------------------------------

#[derive(Debug, Clone, PartialEq)]
pub struct Calibration {
    /// Slot rectangles relative to the calibration window: (x, y, w, h).
    pub slots: Vec<(f32, f32, f32, f32)>,
    pub hover: Option<usize>,
}

impl Calibration {
    pub fn render(&self, c: &mut Canvas, ctx: &mut Ctx) {
        let gold = ctx.theme.gold;
        for (i, &(x, y, w, h)) in self.slots.iter().enumerate() {
            let color = if self.hover == Some(i) { gold } else { Rgba(80, 220, 255, 255) };
            c.stroke_rrect(x, y, w, h, 4.0, 2.0, color);
            ctx.label(c, &format!("{}", i + 1), x + 6.0, y + 12.0, 12.0, Weight::Bold, color, Align::Left);
        }
    }
}

// ---- settings ------------------------------------------------------------

/// What the pointer is over in the settings panel.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SettingsHit {
    /// A theme row; index into `ThemeName::ALL`.
    Theme(usize),
    /// A compact mode row; index into `CompactMode::ALL`.
    Compact(usize),
    /// A layout size row; index into `LayoutSize::ALL`.
    Layout(usize),
}

/// In-overlay settings: the theme, the compact mode and the layout size,
/// applied and saved as soon as they are picked.
#[derive(Debug, Clone, PartialEq)]
pub struct SettingsPanel {
    pub theme: ThemeName,
    pub compact: CompactMode,
    /// Current `scale` from the config, to mark the matching size row.
    pub scale: f32,
    /// Where the choice is written, shown in the footer.
    pub config_path: String,
    /// The last save failed; shown instead of the path.
    pub save_error: Option<String>,
    pub hover: Option<SettingsHit>,
}

impl SettingsPanel {
    pub const W: f32 = TierList::W;
    const TITLE: f32 = 40.0;
    const HEADING: f32 = 26.0;
    const ROW: f32 = 38.0;
    const FOOT: f32 = 30.0;
    const PAD: f32 = 4.0;
    /// Miniature of a palette, or of a list row, at the left of its row.
    const SWATCH_W: f32 = 44.0;
    const SWATCH_H: f32 = 24.0;

    fn content_h(&self) -> f32 {
        Self::TITLE
            + Self::HEADING
            + ThemeName::ALL.len() as f32 * Self::ROW
            + Self::HEADING
            + CompactMode::ALL.len() as f32 * Self::ROW
            + Self::HEADING
            + LayoutSize::ALL.len() as f32 * Self::ROW
            + Self::FOOT
            + Self::PAD
    }

    pub fn size(&self, s: f32, max_h: i32) -> (i32, i32) {
        (s_ceil(Self::W, s), s_ceil(self.content_h(), s).min(max_h))
    }

    fn theme_row_top() -> f32 {
        Self::TITLE + Self::HEADING
    }

    fn compact_heading_top() -> f32 {
        Self::theme_row_top() + ThemeName::ALL.len() as f32 * Self::ROW
    }

    fn compact_row_top() -> f32 {
        Self::compact_heading_top() + Self::HEADING
    }

    fn layout_heading_top() -> f32 {
        Self::compact_row_top() + CompactMode::ALL.len() as f32 * Self::ROW
    }

    fn layout_row_top() -> f32 {
        Self::layout_heading_top() + Self::HEADING
    }

    fn row_box(y0: f32, i: usize) -> Box2 {
        Box2 { x: Self::PAD + 2.0, y: y0 + i as f32 * Self::ROW, w: Self::W - 2.0 * Self::PAD - 4.0, h: Self::ROW }
    }

    /// Row under a window-local point.
    pub fn hit(&self, x: f32, y: f32, s: f32) -> Option<SettingsHit> {
        let (ux, uy) = (x / s, y / s);
        (0..ThemeName::ALL.len())
            .find(|&i| Self::row_box(Self::theme_row_top(), i).contains(ux, uy))
            .map(SettingsHit::Theme)
            .or_else(|| {
                (0..CompactMode::ALL.len())
                    .find(|&i| Self::row_box(Self::compact_row_top(), i).contains(ux, uy))
                    .map(SettingsHit::Compact)
            })
            .or_else(|| {
                (0..LayoutSize::ALL.len())
                    .find(|&i| Self::row_box(Self::layout_row_top(), i).contains(ux, uy))
                    .map(SettingsHit::Layout)
            })
    }

    pub fn render(&self, c: &mut Canvas, ctx: &mut Ctx) {
        let s = ctx.s;
        let t = ctx.theme;
        ctx.panel(c);
        let x = (Self::PAD + 2.0) * s;
        let w = c.width() - 2.0 * x;
        ctx.label(c, "Settings", 12.0 * s, 15.0 * s, 14.0, Weight::Bold, t.gold, Align::Left);
        let sub = ctx.fonts.fit("Click a row to switch and save right away", 10.0 * s, Weight::Medium, w - 12.0 * s);
        ctx.label(c, &sub, 12.0 * s, 31.0 * s, 10.0, Weight::Medium, t.dim, Align::Left);
        ctx.label(c, "THEME", x + 6.0 * s, (Self::theme_row_top() - Self::HEADING / 2.0) * s, 10.0, Weight::Bold, t.dim, Align::Left);
        for (i, name) in ThemeName::ALL.iter().enumerate() {
            self.render_theme_row(c, ctx, i, *name);
        }
        ctx.label(
            c,
            "COMPACT MODE",
            x + 6.0 * s,
            (Self::compact_row_top() - Self::HEADING / 2.0) * s,
            10.0,
            Weight::Bold,
            t.dim,
            Align::Left,
        );
        for (i, mode) in CompactMode::ALL.iter().enumerate() {
            self.render_compact_row(c, ctx, i, *mode);
        }
        ctx.label(
            c,
            "LAYOUT SIZE",
            x + 6.0 * s,
            (Self::layout_row_top() - Self::HEADING / 2.0) * s,
            10.0,
            Weight::Bold,
            t.dim,
            Align::Left,
        );
        for (i, size) in LayoutSize::ALL.iter().enumerate() {
            self.render_layout_row(c, ctx, i, *size);
        }
        // Footer: where the choice went, or why it did not.
        let (text, color) = match &self.save_error {
            Some(msg) => (format!("Not saved: {msg}"), t.loss),
            None => (format!("Saved to {}", self.config_path), t.dim),
        };
        let text = ctx.fonts.fit(&text, 10.0 * s, Weight::Medium, w);
        let fy = (Self::layout_row_top() + LayoutSize::ALL.len() as f32 * Self::ROW + Self::FOOT / 2.0) * s;
        ctx.label(c, &text, c.width() / 2.0, fy, 10.0, Weight::Medium, color, Align::Center);
    }

    /// One theme row: a miniature of the palette, its name, and an accent bar
    /// when it is the active one.
    fn render_theme_row(&self, c: &mut Canvas, ctx: &mut Ctx, i: usize, name: ThemeName) {
        let s = ctx.s;
        let t = ctx.theme;
        let (x, y, w, _) = Self::row_box(Self::theme_row_top(), i).scaled(s);
        let selected = self.theme == name;
        let hover = self.hover == Some(SettingsHit::Theme(i));
        let fill = if hover { t.button_hover } else { t.button };
        c.rrect(x, y + 1.0 * s, w, (Self::ROW - 2.0) * s, 5.0 * s, fill);
        if selected {
            c.rrect(x, y + 1.0 * s, 4.0 * s, (Self::ROW - 2.0) * s, 2.0 * s, t.accent);
        }
        // The palette itself: panel background, accent block and body text.
        let pal = name.theme();
        let (sx, sy) = (x + 10.0 * s, y + (Self::ROW - Self::SWATCH_H) / 2.0 * s);
        let (sw, sh) = (Self::SWATCH_W * s, Self::SWATCH_H * s);
        c.rrect(sx, sy, sw, sh, 4.0 * s, pal.bg_opaque);
        c.stroke_rrect(sx, sy, sw, sh, 4.0 * s, 1.0, pal.border);
        let a = 16.0 * s;
        let (ax, ay) = (sx + 4.0 * s, sy + (sh - a) / 2.0);
        c.rrect(ax, ay, a, a, 3.0 * s, pal.accent);
        ctx.label(c, "A", ax + a / 2.0, ay + a / 2.0, 10.0, Weight::Bold, pal.on_accent, Align::Center);
        ctx.label(c, "Aa", ax + a + 6.0 * s, sy + sh / 2.0, 11.5, Weight::Bold, pal.text, Align::Left);
        let color = if selected || hover { t.text } else { t.soft };
        let label = ctx.fonts.fit(name.label(), 12.5 * s, Weight::Bold, x + w - (sx + sw) - 12.0 * s);
        ctx.label(c, &label, sx + sw + 8.0 * s, y + Self::ROW * s / 2.0, 12.5, Weight::Bold, color, Align::Left);
    }

    /// One compact mode row: a miniature of a list row showing how much art
    /// it keeps, its name, and an accent bar when it is the active one.
    fn render_compact_row(&self, c: &mut Canvas, ctx: &mut Ctx, i: usize, mode: CompactMode) {
        let s = ctx.s;
        let t = ctx.theme;
        let (x, y, w, _) = Self::row_box(Self::compact_row_top(), i).scaled(s);
        let selected = self.compact == mode;
        let hover = self.hover == Some(SettingsHit::Compact(i));
        let fill = if hover { t.button_hover } else { t.button };
        c.rrect(x, y + 1.0 * s, w, (Self::ROW - 2.0) * s, 5.0 * s, fill);
        if selected {
            c.rrect(x, y + 1.0 * s, 4.0 * s, (Self::ROW - 2.0) * s, 2.0 * s, t.accent);
        }
        // A miniature list row: base, a text line, and art filling the rest.
        let (sx, sy) = (x + 10.0 * s, y + (Self::ROW - Self::SWATCH_H) / 2.0 * s);
        let (sw, sh) = (Self::SWATCH_W * s, Self::SWATCH_H * s);
        c.rrect(sx, sy, sw, sh, 4.0 * s, t.row);
        c.stroke_rrect(sx, sy, sw, sh, 4.0 * s, 1.0, t.border);
        let art = match mode {
            CompactMode::Full => 24.0,
            CompactMode::Small => 11.0,
            CompactMode::Text => 0.0,
        } * s;
        if art > 0.0 {
            c.rrect(sx + sw - art - 2.0 * s, sy + 2.0 * s, art, sh - 4.0 * s, 3.0 * s, t.accent);
        }
        let line_w = (sw - art - 12.0 * s).max(8.0 * s);
        c.rrect(sx + 5.0 * s, sy + (sh - 4.0 * s) / 2.0, line_w, 4.0 * s, 2.0 * s, t.soft);
        let color = if selected || hover { t.text } else { t.soft };
        let label = ctx.fonts.fit(mode.label(), 12.5 * s, Weight::Bold, x + w - (sx + sw) - 12.0 * s);
        ctx.label(c, &label, sx + sw + 8.0 * s, y + Self::ROW * s / 2.0, 12.5, Weight::Bold, color, Align::Left);
    }

    /// One layout size row: a miniature panel shrinking with the percentage,
    /// its label, and an accent bar when it is the active one.
    fn render_layout_row(&self, c: &mut Canvas, ctx: &mut Ctx, i: usize, size: LayoutSize) {
        let s = ctx.s;
        let t = ctx.theme;
        let (x, y, w, _) = Self::row_box(Self::layout_row_top(), i).scaled(s);
        let selected = self.scale == size.scale();
        let hover = self.hover == Some(SettingsHit::Layout(i));
        let fill = if hover { t.button_hover } else { t.button };
        c.rrect(x, y + 1.0 * s, w, (Self::ROW - 2.0) * s, 5.0 * s, fill);
        if selected {
            c.rrect(x, y + 1.0 * s, 4.0 * s, (Self::ROW - 2.0) * s, 2.0 * s, t.accent);
        }
        // A miniature overlay: a panel taking its share of the swatch.
        let (sx, sy) = (x + 10.0 * s, y + (Self::ROW - Self::SWATCH_H) / 2.0 * s);
        let (sw, sh) = (Self::SWATCH_W * s, Self::SWATCH_H * s);
        c.rrect(sx, sy, sw, sh, 4.0 * s, t.row);
        c.stroke_rrect(sx, sy, sw, sh, 4.0 * s, 1.0, t.border);
        let pct = size.scale();
        let (pw, ph) = ((sw - 8.0 * s) * pct, (sh - 8.0 * s) * pct);
        c.rrect(sx + (sw - pw) / 2.0, sy + (sh - ph) / 2.0, pw, ph, 2.0 * s, t.accent);
        let color = if selected || hover { t.text } else { t.soft };
        let label = ctx.fonts.fit(size.label(), 12.5 * s, Weight::Bold, x + w - (sx + sw) - 12.0 * s);
        ctx.label(c, &label, sx + sw + 8.0 * s, y + Self::ROW * s / 2.0, 12.5, Weight::Bold, color, Align::Left);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn popup(boards: Vec<BoardView>, shown: Vec<bool>) -> OpponentPopup {
        OpponentPopup {
            lobby_id: 1,
            hero: "H".into(),
            hero_card_id: "H".into(),
            place: 1,
            health: 30,
            turn: 9,
            boards,
            shown,
            hidden: 0,
            hover: None,
            images_rev: 0,
            compact: CompactMode::Full,
        }
    }

    fn board(turn: i32) -> BoardView {
        let minion = MinionView { card_id: "X".into(), name: "X".into(), attack: 1, health: 1, golden: false, keywords: vec![] };
        BoardView { turn, tier: 1, minions: vec![minion; 7] }
    }

    #[test]
    fn popup_drops_oldest_boards_to_fit() {
        let mut p = popup(vec![board(9), board(6), board(3)], vec![true; 3]);
        let full = p.size(1.0).1;
        p.fit(1.0, full);
        assert_eq!(p.hidden, 0);
        p.fit(1.0, full - 1);
        assert_eq!(p.visible().map(|(_, b)| b.turn).collect::<Vec<_>>(), [9, 6]);
        assert_eq!(p.hidden, 1);
        assert!(p.size(1.0).1 < full);
        // The newest board stays even when it does not fit.
        p.fit(1.0, 10);
        assert_eq!((p.visible().count(), p.hidden), (1, 2));
    }

    #[test]
    fn popup_shows_only_toggled_boards() {
        let p = popup(vec![board(9), board(6), board(3)], vec![false, false, true]);
        assert_eq!(p.visible().map(|(i, b)| (i, b.turn)).collect::<Vec<_>>(), [(2, 3)]);
        let (x, y, w, h) = p.chip_box(1).scaled(2.0);
        assert_eq!(p.hit(x + w / 2.0, y + h / 2.0, 2.0), Some(1));
        assert_eq!(p.hit(1.0, 1.0, 2.0), None);
        // Nothing toggled on: room for the hint.
        let none = popup(vec![board(9)], vec![false]);
        assert!(none.size(1.0).1 > 0);
    }

    #[test]
    fn popup_shrinks_with_the_compact_mode() {
        let mut p = popup(vec![board(9), board(6)], vec![true; 2]);
        let (full_w, full_h) = p.size(1.0);
        p.compact = CompactMode::Small;
        let (small_w, small_h) = p.size(1.0);
        p.compact = CompactMode::Text;
        let (text_w, text_h) = p.size(1.0);
        assert!(small_w < full_w && text_w <= full_w, "smaller modes narrow the popup");
        assert!(text_h < small_h && small_h < full_h, "each mode stacks shorter boards");
        // Hit boxes follow the width.
        let (x, y, w, h) = p.chip_box(0).scaled(1.0);
        assert_eq!(p.hit(x + w / 2.0, y + h / 2.0, 1.0), Some(0));
    }

    fn settings() -> SettingsPanel {
        SettingsPanel {
            theme: ThemeName::Dark,
            compact: CompactMode::Full,
            scale: 1.0,
            config_path: "~/.config/hsbg-overlay/config.toml".into(),
            save_error: None,
            hover: None,
        }
    }

    #[test]
    fn settings_rows_pick_themes_compact_modes_and_layout_sizes() {
        let p = settings();
        let (x, y, w, h) = SettingsPanel::row_box(SettingsPanel::theme_row_top(), 1).scaled(2.0);
        assert_eq!(p.hit(x + w / 2.0, y + h / 2.0, 2.0), Some(SettingsHit::Theme(1)));
        let (x, y, w, h) = SettingsPanel::row_box(SettingsPanel::compact_row_top(), 2).scaled(2.0);
        assert_eq!(p.hit(x + w / 2.0, y + h / 2.0, 2.0), Some(SettingsHit::Compact(2)));
        let (x, y, w, h) = SettingsPanel::row_box(SettingsPanel::layout_row_top(), 1).scaled(2.0);
        assert_eq!(p.hit(x + w / 2.0, y + h / 2.0, 2.0), Some(SettingsHit::Layout(1)));
        assert_eq!(p.hit(4.0, 4.0, 2.0), None, "title band is not clickable");
        // All three sections fit above the footer, which fits inside the panel.
        assert_eq!(p.size(1.0, 1000), (252, 494));
        let (_, y, _, h) = SettingsPanel::row_box(SettingsPanel::layout_row_top(), 2).scaled(1.0);
        assert!(y + h < 494.0);
    }

    #[test]
    fn place_colors_follow_theme() {
        assert_eq!(place_color(1, &Theme::DARK), BADGE_GOLD);
        assert_eq!(place_color(2, &Theme::DARK), Theme::DARK.win);
        assert_eq!(place_color(2, &Theme::HIGH_CONTRAST), Theme::HIGH_CONTRAST.win);
        assert_eq!(place_color(8, &Theme::HIGH_CONTRAST), Theme::HIGH_CONTRAST.loss);
        assert_eq!(place_color(8, &Theme::DARK), Theme::DARK.loss);
    }

    #[test]
    fn info_panel_formatting_and_size() {
        assert_eq!(ordinal(1), "1st");
        assert_eq!(ordinal(2), "2nd");
        assert_eq!(ordinal(3), "3rd");
        assert_eq!(ordinal(4), "4th");
        assert_eq!(ordinal(8), "8th");
        assert_eq!(format_thousands(0), "0");
        assert_eq!(format_thousands(999), "999");
        assert_eq!(format_thousands(1000), "1,000");
        assert_eq!(format_thousands(13726), "13,726");
        assert_eq!(format_thousands(-45), "-45");
        assert_eq!(format_thousands(-1250), "-1,250");

        let panel = InfoPanel {
            tribes: vec![Tribe::Beast, Tribe::Mech],
            slots: 5,
            start_mmr: Some(13726),
            current_mmr: Some(13717),
            games: vec![
                GameRow {
                    hero_card_id: "TB_BaconShop_HERO_55".into(),
                    hero_name: "Tickatus".into(),
                    place: 5,
                    mmr_delta: Some(-9),
                },
                GameRow {
                    hero_card_id: "TB_BaconShop_HERO_53".into(),
                    hero_name: "Mutanus".into(),
                    place: 2,
                    mmr_delta: Some(70),
                },
            ],
            width: 190.0,
        };
        // Height: 18 + 56 + 18 + 40 + 18 + 16 + 2 * 24 = 214
        assert_eq!(panel.size(1.0), (190, 214));

        let (w, h) = panel.size(1.0);
        let mut canvas = Canvas::new(w, h);
        let mut fonts = Fonts::load();
        let mut images = Images::new();
        let mut ctx = Ctx { fonts: &mut fonts, images: &mut images, theme: Theme::DARK, compact: CompactMode::Full, s: 1.0, opaque: false };
        panel.render(&mut canvas, &mut ctx);
        canvas.pixmap.save_png("/tmp/test_info_panel.png").unwrap();
    }

    #[test]
    fn top_bar_tools_and_settings_hit() {
        let bar = TopBar {
            turn: 3,
            tavern_tier: 2,
            max_tier: 6,
            selected: None,
            hover: None,
            comps: true,
            comps_open: false,
            search: true,
            search_open: false,
            settings_open: false,
        };
        let tools = bar.tools();
        assert_eq!(tools.len(), 3);
        assert_eq!(tools[0].0, TopHit::Comps);
        assert_eq!(tools[1].0, TopHit::Search);
        assert_eq!(tools[2].0, TopHit::Settings);

        let settings_x = bar.tool_x(2);
        assert_eq!(bar.hit(settings_x + 5.0, 1.0), Some(TopHit::Settings));
        let tier1_x = bar.button_x(1);
        assert_eq!(bar.hit(tier1_x + 5.0, 1.0), Some(TopHit::Tier(1)));
    }
}
