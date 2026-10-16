//! Panel models and their rendering. Models are plain data compared with
//! `PartialEq`; a panel is only re-rendered when its model changes.
//!
//! Layout constants are in unscaled pixels at a 1080p game window; `Ctx::s`
//! scales them to the actual window.

use super::draw::{Align, Canvas, Fonts, Rgba, Weight};
use crate::cards::{Category, Tribe, KEYWORDS};
use crate::images::{ImageKind, Images};

pub mod theme {
    use super::Rgba;
    pub const BG: Rgba = Rgba(12, 15, 26, 228);
    pub const BG_OPAQUE: Rgba = Rgba(12, 15, 26, 255);
    pub const BORDER: Rgba = Rgba(74, 96, 150, 150);
    pub const HEADER: Rgba = Rgba(26, 36, 64, 255);
    pub const BUTTON: Rgba = Rgba(28, 34, 54, 255);
    pub const BUTTON_HOVER: Rgba = Rgba(45, 58, 96, 255);
    pub const ACCENT: Rgba = Rgba(64, 98, 180, 255);
    pub const TEXT: Rgba = Rgba(238, 240, 246, 255);
    pub const DIM: Rgba = Rgba(146, 156, 182, 255);
    pub const GOLD: Rgba = Rgba(242, 193, 78, 255);
    pub const WIN: Rgba = Rgba(102, 187, 106, 255);
    pub const TIE: Rgba = Rgba(189, 189, 189, 255);
    pub const LOSS: Rgba = Rgba(239, 83, 80, 255);
    pub const WIN_LETHAL: Rgba = Rgba(46, 160, 67, 255);
    pub const LOSS_LETHAL: Rgba = Rgba(198, 40, 40, 255);
    pub const ATTACK: Rgba = Rgba(224, 165, 38, 255);
    pub const HEALTH: Rgba = Rgba(192, 57, 43, 255);
}

use theme::*;

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
        let px = px * self.s;
        let y = self.mid(cy, px, w);
        c.text_shadow(self.fonts, text, x, y, px, w, color, Align::Left);
    }

    fn panel(&self, c: &mut Canvas) {
        let (w, h) = (c.width(), c.height());
        if self.opaque {
            c.rect(0.0, 0.0, w, h, BG_OPAQUE);
            c.stroke_rrect(0.0, 0.0, w, h, 0.0, 1.0, BORDER);
        } else {
            let r = 8.0 * self.s;
            c.rrect(0.0, 0.0, w, h, r, BG);
            c.stroke_rrect(0.0, 0.0, w, h, r, 1.0, BORDER);
        }
    }

    /// A list row with the card's tile art fading in from the right.
    #[allow(clippy::too_many_arguments)]
    fn tile_row(&mut self, c: &mut Canvas, card_id: &str, x: f32, y: f32, w: f32, h: f32, base: Rgba) {
        c.rect(x, y, w, h, base);
        if let Some(img) = self.images.get(ImageKind::Tile, card_id) {
            let tw = (h * img.width() as f32 / img.height() as f32).min(w * 0.7);
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
        tools
    }

    fn tool_x(i: usize) -> f32 {
        Self::PAD + Self::TURN_W + Self::GAP * 2.0 + i as f32 * (Self::TOOL_W + Self::GAP)
    }

    /// Right edge of the last tool button.
    fn tools_end(&self) -> f32 {
        match self.tools().len() {
            0 => Self::tool_x(0),
            n => Self::tool_x(n - 1) + Self::TOOL_W,
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
            if x >= Self::tool_x(i) && x < Self::tool_x(i) + Self::TOOL_W {
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
        ctx.panel(c);
        let label = if self.turn > 0 { format!("Turn {}", self.turn) } else { "Hero pick".to_string() };
        ctx.label(c, &label, (Self::PAD + Self::TURN_W / 2.0) * s, Self::H / 2.0 * s, 15.0, Weight::Bold, TEXT, Align::Center);
        let sep = |c: &mut Canvas, x: f32| c.rect(x * s, 7.0 * s, 1.0, (Self::H - 14.0) * s, BORDER);
        sep(c, Self::PAD + Self::TURN_W + Self::GAP);
        let by = (Self::H - Self::BTN) / 2.0 * s;
        let b = Self::BTN * s;
        let fill = |selected: bool, hover: bool| if selected {
            ACCENT
        } else if hover {
            BUTTON_HOVER
        } else {
            BUTTON
        };
        let tools = self.tools();
        for (i, (hit, label, open)) in tools.iter().enumerate() {
            let (bx, bw) = (Self::tool_x(i) * s, Self::TOOL_W * s);
            c.rrect(bx, by, bw, b, 5.0 * s, fill(*open, self.hover == Some(*hit)));
            ctx.label(c, label, bx + bw / 2.0, by + b / 2.0, 13.0, Weight::Bold, TEXT, Align::Center);
        }
        if !tools.is_empty() && self.max_tier > 0 {
            sep(c, self.tools_end() + Self::GAP * 1.5);
        }
        for t in 1..=self.max_tier {
            let bx = self.button_x(t) * s;
            c.rrect(bx, by, b, b, 5.0 * s, fill(self.selected == Some(t), self.hover == Some(TopHit::Tier(t))));
            if t == self.tavern_tier {
                c.stroke_rrect(bx, by, b, b, 5.0 * s, 2.0 * s, GOLD);
            }
            let color = if t <= self.tavern_tier || self.selected == Some(t) { TEXT } else { DIM };
            ctx.label(c, &t.to_string(), bx + b / 2.0, by + b / 2.0, 15.0, Weight::Bold, color, Align::Center);
        }
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
        let w = c.width();
        ctx.panel(c);
        self.render_rows(c, ctx);
        // Header drawn last so scrolled rows slide under it.
        let hh = self.header_h() * s;
        if ctx.opaque {
            c.rect(0.0, 0.0, w, hh, BG_OPAQUE);
        } else {
            c.rrect(0.0, 0.0, w, hh.min(16.0 * s), 8.0 * s, BG_OPAQUE);
            c.rect(0.0, 8.0 * s, w, hh - 8.0 * s, BG_OPAQUE);
        }
        self.render_title(c, ctx);
        self.render_filter(c, ctx);
        c.rect(0.0, hh - 1.0, w, 1.0, BORDER);
        let max_scroll = self.max_scroll(s, c.height() as i32);
        if max_scroll > 0.0 {
            let track = c.height() - hh - 8.0 * s;
            let thumb = (track * c.height() / (self.content_h() * s)).max(20.0 * s);
            let ty = hh + 4.0 * s + (track - thumb) * (self.scroll / max_scroll);
            c.rrect(w - 4.0 * s, ty, 2.5 * s, thumb, 1.5 * s, Rgba(255, 255, 255, 90));
        }
    }

    fn render_rows(&self, c: &mut Canvas, ctx: &mut Ctx) {
        let s = ctx.s;
        let inner_x = Self::PAD * s;
        let inner_w = c.width() - 2.0 * Self::PAD * s;
        let base = if ctx.opaque { BG_OPAQUE } else { Rgba(16, 20, 34, 255) };
        let mut y = self.header_h() * s - self.scroll;
        if self.sections.is_empty() {
            let msg = format!("No {} cards in tier {}", self.keyword_label(), self.tier);
            ctx.label(c, &msg, c.width() / 2.0, y + Self::ROW * s / 2.0, 11.5, Weight::Medium, DIM, Align::Center);
            return;
        }
        let mut index = 0;
        for sec in &self.sections {
            let hh = Self::HEAD * s;
            if y + hh > 0.0 && y < c.height() {
                c.rect(inner_x, y, inner_w, hh, HEADER);
                c.rect(inner_x, y, 4.0 * s, hh, if sec.dim { sec.color.alpha(110) } else { sec.color });
                let color = if sec.dim { DIM } else { TEXT };
                ctx.label(c, &sec.title, inner_x + 11.0 * s, y + hh / 2.0, 13.0, Weight::Bold, color, Align::Left);
            }
            y += hh;
            for row in &sec.rows {
                let rh = Self::ROW * s;
                if y + rh > 0.0 && y < c.height() {
                    ctx.tile_row(c, &row.card_id, inner_x, y + 1.0, inner_w, rh - 1.0, base);
                    if self.hover == Some(TierHit::Row(index)) {
                        c.rect(inner_x, y + 1.0, inner_w, rh - 1.0, Rgba(255, 255, 255, 28));
                        c.rect(inner_x, y + 1.0, 3.0 * s, rh - 1.0, GOLD);
                    }
                    let max = inner_w - if row.cost.is_some() { 40.0 * s } else { 14.0 * s };
                    let name = ctx.fonts.fit(&row.name, 12.5 * s, Weight::Medium, max);
                    let color = if sec.dim { DIM } else { TEXT };
                    ctx.label_shadow(c, &name, inner_x + 9.0 * s, y + rh / 2.0, 12.5, Weight::Medium, color);
                    if let Some(cost) = row.cost {
                        let cx = inner_x + inner_w - 14.0 * s;
                        c.circle(cx, y + rh / 2.0, 9.5 * s, Rgba(0, 0, 0, 160));
                        c.circle(cx, y + rh / 2.0, 8.5 * s, GOLD);
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
        let (tx, ty, tw, th) = Self::toggle_box().scaled(s);
        ctx.label(c, &format!("Tavern Tier {}", self.tier), 12.0 * s, 15.0 * s, 14.0, Weight::Bold, GOLD, Align::Left);
        let subtitle = ctx.fonts.fit(&self.subtitle, 10.0 * s, Weight::Medium, tx - 18.0 * s);
        ctx.label(c, &subtitle, 12.0 * s, 31.0 * s, 10.0, Weight::Medium, DIM, Align::Left);
        // "Lobby only" toggle: hides tribes that are not in this lobby.
        let hover = self.hover == Some(TierHit::LobbyToggle);
        c.rrect(tx, ty, tw, th, 5.0 * s, if hover { BUTTON_HOVER } else { BUTTON });
        let bx = tx + 7.0 * s;
        let b = 12.0 * s;
        let by = ty + (th - b) / 2.0;
        if self.hide_unavailable {
            c.rrect(bx, by, b, b, 3.0 * s, ACCENT);
            c.rrect(bx + 3.0 * s, by + 3.0 * s, b - 6.0 * s, b - 6.0 * s, 1.5 * s, TEXT);
        } else {
            c.stroke_rrect(bx, by, b, b, 3.0 * s, 1.5 * s, DIM);
        }
        let color = if self.hide_unavailable { TEXT } else { DIM };
        ctx.label(c, "Lobby only", bx + b + 6.0 * s, ty + th / 2.0, 10.5, Weight::Bold, color, Align::Left);
    }

    fn render_filter(&self, c: &mut Canvas, ctx: &mut Ctx) {
        let s = ctx.s;
        let (x, y, w, h) = Self::filter_box().scaled(s);
        let active = self.keyword.is_some();
        let hover = matches!(self.hover, Some(TierHit::FilterBar | TierHit::FilterClear));
        let fill = match (active, hover) {
            (true, _) => ACCENT,
            (false, true) => BUTTON_HOVER,
            (false, false) => BUTTON,
        };
        c.rrect(x, y, w, h, 5.0 * s, fill);
        let cy = y + h / 2.0;
        ctx.label(c, "Keyword", x + 9.0 * s, cy, 10.5, Weight::Bold, if active { TEXT } else { DIM }, Align::Left);
        let value_x = x + 66.0 * s;
        ctx.label(c, self.keyword_label(), value_x, cy, 12.0, Weight::Bold, TEXT, Align::Left);
        if active {
            let clear_hover = self.hover == Some(TierHit::FilterClear);
            let cx = x + w - Self::CLEAR_W * s / 2.0;
            if clear_hover {
                c.circle(cx, cy, 8.5 * s, Rgba(255, 255, 255, 50));
            }
            ctx.label(c, "×", cx, cy, 15.0, Weight::Bold, TEXT, Align::Center);
        } else {
            // Disclosure arrow: down when closed, up when open.
            let (ax, aw, ah) = (x + w - 16.0 * s, 8.0 * s, 4.5 * s);
            let (tip, base) = if self.filter_open { (cy - ah / 2.0, cy + ah / 2.0) } else { (cy + ah / 2.0, cy - ah / 2.0) };
            c.triangle([(ax - aw / 2.0, base), (ax + aw / 2.0, base), (ax, tip)], DIM);
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
                ACCENT
            } else if hover {
                BUTTON_HOVER
            } else {
                BUTTON
            };
            c.rrect(cx, cy, cw, ch, 4.0 * s, fill);
            let label = keyword.map(|k| KEYWORDS[k].label).unwrap_or("Any");
            // Shrink long labels a little before resorting to an ellipsis.
            let mut px = 10.0;
            while px > 8.5 && ctx.fonts.measure(label, px * s, Weight::Bold) > cw - 6.0 * s {
                px -= 0.5;
            }
            let label = ctx.fonts.fit(label, px * s, Weight::Bold, cw - 6.0 * s);
            let color = if selected || hover { TEXT } else { Rgba(200, 208, 228, 255) };
            ctx.label(c, &label, cx + cw / 2.0, cy + ch / 2.0, px, Weight::Bold, color, Align::Center);
        }
    }
}

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
        let w = c.width();
        ctx.panel(c);
        self.render_items(c, ctx);
        // Header drawn last so scrolled items slide under it.
        let hh = Self::TITLE * s;
        if ctx.opaque {
            c.rect(0.0, 0.0, w, hh, BG_OPAQUE);
        } else {
            c.rrect(0.0, 0.0, w, 16.0 * s, 8.0 * s, BG_OPAQUE);
            c.rect(0.0, 8.0 * s, w, hh - 8.0 * s, BG_OPAQUE);
        }
        self.render_header(c, ctx);
        c.rect(0.0, hh - 1.0, w, 1.0, BORDER);
        let max_scroll = self.max_scroll(s, c.height() as i32);
        if max_scroll > 0.0 {
            let track = c.height() - hh - 8.0 * s;
            let thumb = (track * c.height() / (self.content_h() * s)).max(20.0 * s);
            let ty = hh + 4.0 * s + (track - thumb) * (self.scroll / max_scroll);
            c.rrect(w - 4.0 * s, ty, 2.5 * s, thumb, 1.5 * s, Rgba(255, 255, 255, 90));
        }
    }

    fn render_header(&self, c: &mut Canvas, ctx: &mut Ctx) {
        let s = ctx.s;
        // Left edge of the title and right edge of the room it has.
        let (tx, right) = match self.detail {
            Some(letter) => {
                let (bx, by, bw, bh) = Self::back_box().scaled(s);
                c.rrect(bx, by, bw, bh, 5.0 * s, if self.hover == Some(CompHit::Back) { BUTTON_HOVER } else { BUTTON });
                let (cx, cy, a) = (bx + bw / 2.0, by + bh / 2.0, 5.0 * s);
                c.triangle([(cx + a * 0.6, cy - a), (cx + a * 0.6, cy + a), (cx - a * 0.7, cy)], TEXT);
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
                c.rrect(bx, by, bw, bh, 5.0 * s, if hover { BUTTON_HOVER } else { BUTTON });
                let b = 12.0 * s;
                let (cx, cy) = (bx + 7.0 * s, by + (bh - b) / 2.0);
                if self.hide_unavailable {
                    c.rrect(cx, cy, b, b, 3.0 * s, ACCENT);
                    c.rrect(cx + 3.0 * s, cy + 3.0 * s, b - 6.0 * s, b - 6.0 * s, 1.5 * s, TEXT);
                } else {
                    c.stroke_rrect(cx, cy, b, b, 3.0 * s, 1.5 * s, DIM);
                }
                let color = if self.hide_unavailable { TEXT } else { DIM };
                ctx.label(c, "Lobby only", cx + b + 6.0 * s, by + bh / 2.0, 10.5, Weight::Bold, color, Align::Left);
                (12.0 * s, bx - 8.0 * s)
            }
        };
        let title = ctx.fonts.fit(&self.title, 14.0 * s, Weight::Bold, right - tx);
        ctx.label(c, &title, tx, 16.0 * s, 14.0, Weight::Bold, GOLD, Align::Left);
        let subtitle = ctx.fonts.fit(&self.subtitle, 10.0 * s, Weight::Medium, right - tx);
        ctx.label(c, &subtitle, tx, 33.0 * s, 10.0, Weight::Medium, DIM, Align::Left);
    }

    fn render_items(&self, c: &mut Canvas, ctx: &mut Ctx) {
        let s = ctx.s;
        let x = Self::PAD * s;
        let iw = c.width() - 2.0 * Self::PAD * s;
        let base = if ctx.opaque { BG_OPAQUE } else { Rgba(16, 20, 34, 255) };
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
                    c.rect(x, y + 2.0 * s, iw, h - 2.0 * s, HEADER);
                    let b = 18.0 * s;
                    let cy = cy + 1.0 * s;
                    c.rrect(x + 6.0 * s, cy - b / 2.0, b, b, 4.0 * s, color);
                    ctx.label(c, letter, x + 6.0 * s + b / 2.0, cy, 12.0, Weight::Bold, Rgba(30, 20, 10, 255), Align::Center);
                    ctx.label(c, &format!("{letter} tier"), x + 32.0 * s, cy, 12.5, Weight::Bold, TEXT, Align::Left);
                    let n = format!("{count} comp{}", if *count == 1 { "" } else { "s" });
                    let nw = ctx.fonts.measure(&n, 10.0 * s, Weight::Medium);
                    ctx.label(c, &n, x + iw - nw - 10.0 * s, cy, 10.0, Weight::Medium, DIM, Align::Left);
                }
                CompItem::Comp(row) => {
                    let (ry, rh) = (y + 1.0 * s, h - 1.0 * s);
                    ctx.tile_row(c, &row.card_id, x, ry, iw, rh, base);
                    if row.dim {
                        c.rect(x, ry, iw, rh, base.alpha(130));
                    }
                    c.rect(x, ry, 4.0 * s, rh, if row.dim { row.color.alpha(110) } else { row.color });
                    if hover {
                        c.rect(x, ry, iw, rh, Rgba(255, 255, 255, 28));
                        c.rect(x, ry, 3.0 * s, rh, GOLD);
                    }
                    if row.your_hero {
                        c.rect(x + iw - 3.0 * s, ry, 3.0 * s, rh, GOLD);
                    }
                    let max = iw - 40.0 * s;
                    let name = ctx.fonts.fit(&row.name, 12.5 * s, Weight::Bold, max);
                    ctx.label_shadow(c, &name, x + 11.0 * s, cy - 7.0 * s, 12.5, Weight::Bold, if row.dim { DIM } else { TEXT });
                    let detail = ctx.fonts.fit(&row.detail, 10.0 * s, Weight::Medium, max);
                    ctx.label_shadow(c, &detail, x + 11.0 * s, cy + 8.0 * s, 10.0, Weight::Medium, Rgba(180, 190, 215, 255));
                    if row.trend != 0 {
                        let (ax, a) = (x + iw - 14.0 * s, 4.5 * s);
                        c.circle(ax, cy, 8.5 * s, Rgba(0, 0, 0, 170));
                        let (tip, foot, color) =
                            if row.trend > 0 { (cy - a, cy + a * 0.8, WIN) } else { (cy + a, cy - a * 0.8, LOSS) };
                        c.triangle([(ax - a, foot), (ax + a, foot), (ax, tip)], color);
                    }
                }
                CompItem::Heading(text) => {
                    ctx.label(c, text, x + 8.0 * s, cy + 3.0 * s, 10.0, Weight::Bold, GOLD.alpha(220), Align::Left);
                }
                CompItem::Text { text, strong } => {
                    let color = if *strong { TEXT } else { Rgba(205, 212, 230, 255) };
                    let weight = if *strong { Weight::Bold } else { Weight::Medium };
                    ctx.label(c, text, x + 8.0 * s, cy, Self::TEXT_PX, weight, color, Align::Left);
                }
                CompItem::Card { card_id, name, tier } => {
                    let (ry, rh) = (y + 1.0 * s, h - 1.0 * s);
                    ctx.tile_row(c, card_id, x, ry, iw, rh, base);
                    if hover {
                        c.rect(x, ry, iw, rh, Rgba(255, 255, 255, 28));
                        c.rect(x, ry, 3.0 * s, rh, GOLD);
                    }
                    let name = ctx.fonts.fit(name, 12.0 * s, Weight::Medium, iw - 44.0 * s);
                    ctx.label_shadow(c, &name, x + 9.0 * s, cy, 12.0, Weight::Medium, TEXT);
                    if *tier > 0 {
                        let tx = x + iw - 14.0 * s;
                        c.circle(tx, cy, 9.5 * s, Rgba(0, 0, 0, 170));
                        c.circle(tx, cy, 8.5 * s, ACCENT);
                        ctx.label(c, &tier.to_string(), tx, cy, 11.0, Weight::Bold, TEXT, Align::Center);
                    }
                }
                CompItem::Hero { card_id, name, detail, yours } => {
                    let (ry, rh) = (y + 1.0 * s, h - 1.0 * s);
                    ctx.tile_row(c, card_id, x, ry, iw, rh, base);
                    if *yours {
                        c.rect(x, ry, 3.0 * s, rh, GOLD);
                    }
                    if hover {
                        c.rect(x, ry, iw, rh, Rgba(255, 255, 255, 28));
                        c.rect(x, ry, 3.0 * s, rh, GOLD);
                    }
                    let dw = ctx.fonts.measure(detail, 10.5 * s, Weight::Bold);
                    let dx = x + iw - dw - 12.0 * s;
                    c.rrect(dx - 6.0 * s, cy - 8.5 * s, dw + 12.0 * s, 17.0 * s, 8.5 * s, Rgba(0, 0, 0, 185));
                    let name = ctx.fonts.fit(name, 12.0 * s, Weight::Medium, dx - x - 18.0 * s);
                    ctx.label_shadow(c, &name, x + 9.0 * s, cy, 12.0, Weight::Medium, if *yours { GOLD } else { TEXT });
                    ctx.label(c, detail, dx, cy, 10.5, Weight::Bold, TEXT, Align::Left);
                }
                CompItem::Note(text) => {
                    let text = ctx.fonts.fit(text, 11.5 * s, Weight::Medium, iw - 16.0 * s);
                    ctx.label(c, &text, c.width() / 2.0, cy, 11.5, Weight::Medium, DIM, Align::Center);
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
        let w = c.width();
        ctx.panel(c);
        self.render_rows(c, ctx);
        // Header drawn last so scrolled rows slide under it.
        let hh = Self::header_h() * s;
        if ctx.opaque {
            c.rect(0.0, 0.0, w, hh, BG_OPAQUE);
        } else {
            c.rrect(0.0, 0.0, w, 16.0 * s, 8.0 * s, BG_OPAQUE);
            c.rect(0.0, 8.0 * s, w, hh - 8.0 * s, BG_OPAQUE);
        }
        ctx.label(c, "Card Search", 12.0 * s, 15.0 * s, 14.0, Weight::Bold, GOLD, Align::Left);
        let subtitle = ctx.fonts.fit(&self.subtitle, 10.0 * s, Weight::Medium, w - 24.0 * s);
        ctx.label(c, &subtitle, 12.0 * s, 31.0 * s, 10.0, Weight::Medium, DIM, Align::Left);
        self.render_field(c, ctx);
        self.render_chips(c, ctx);
        c.rect(0.0, hh - 1.0, w, 1.0, BORDER);
        let max_scroll = self.max_scroll(s, c.height() as i32);
        if max_scroll > 0.0 {
            let track = c.height() - hh - 8.0 * s;
            let thumb = (track * c.height() / (self.content_h() * s)).max(20.0 * s);
            let ty = hh + 4.0 * s + (track - thumb) * (self.scroll / max_scroll);
            c.rrect(w - 4.0 * s, ty, 2.5 * s, thumb, 1.5 * s, Rgba(255, 255, 255, 90));
        }
    }

    fn render_field(&self, c: &mut Canvas, ctx: &mut Ctx) {
        let s = ctx.s;
        let (x, y, w, h) = Self::field_box().scaled(s);
        let cy = y + h / 2.0;
        let hover = matches!(self.hover, Some(SearchHit::Field | SearchHit::Clear));
        c.rrect(x, y, w, h, 5.0 * s, if hover && !self.focused { BUTTON_HOVER } else { Rgba(8, 10, 18, 255) });
        let border = if self.focused { ACCENT } else { BORDER };
        c.stroke_rrect(x, y, w, h, 5.0 * s, if self.focused { 1.5 * s } else { 1.0 }, border);
        // Magnifier: a ring and its handle.
        let (mx, my, r) = (x + 13.0 * s, cy - 1.5 * s, 4.5 * s);
        c.stroke_rrect(mx - r, my - r, 2.0 * r, 2.0 * r, r, 1.6 * s, DIM);
        let (hx, hy, t) = (mx + r * 0.7, my + r * 0.7, 1.1 * s);
        c.triangle([(hx - t, hy + t), (hx + t, hy - t), (hx + 4.0 * s, hy + 4.0 * s)], DIM);
        c.triangle([(hx - t, hy + t), (hx + 4.0 * s, hy + 4.0 * s), (hx + 4.0 * s - t, hy + 4.0 * s + t)], DIM);
        let tx = x + 26.0 * s;
        let room = w - 26.0 * s - if self.query.is_empty() { 8.0 * s } else { Self::CLEAR_W * s };
        let px = 12.5 * s;
        if self.query.is_empty() {
            let hint = if self.focused { "Type a card name…" } else { "Click to search cards" };
            ctx.label(c, hint, tx, cy, 12.0, Weight::Medium, DIM.alpha(200), Align::Left);
        } else {
            // Keep the end of a long query, where the caret is.
            let mut shown = self.query.clone();
            while ctx.fonts.measure(&shown, px, Weight::Medium) > room - 4.0 * s && !shown.is_empty() {
                shown.remove(0);
            }
            let text = if shown.len() < self.query.len() { format!("…{shown}") } else { shown };
            let tw = ctx.fonts.measure(&text, px, Weight::Medium);
            ctx.label(c, &text, tx, cy, 12.5, Weight::Medium, TEXT, Align::Left);
            if self.focused {
                c.rect(tx + tw + 1.5 * s, cy - 8.0 * s, 1.5 * s, 16.0 * s, TEXT);
            }
            let cx = x + w - Self::CLEAR_W * s / 2.0;
            if self.hover == Some(SearchHit::Clear) {
                c.circle(cx, cy, 8.5 * s, Rgba(255, 255, 255, 50));
            }
            ctx.label(c, "×", cx, cy, 15.0, Weight::Bold, TEXT, Align::Center);
            return;
        }
        if self.focused {
            c.rect(tx - 1.0 * s, cy - 8.0 * s, 1.5 * s, 16.0 * s, TEXT);
        }
    }

    fn render_chips(&self, c: &mut Canvas, ctx: &mut Ctx) {
        let s = ctx.s;
        for (i, k) in Self::chips().enumerate() {
            let (cx, cy, cw, ch) = Self::chip_box(i).scaled(s);
            let selected = k == self.category;
            let hover = self.hover == Some(SearchHit::Category(k));
            let fill = if selected {
                ACCENT
            } else if hover {
                BUTTON_HOVER
            } else {
                BUTTON
            };
            c.rrect(cx, cy, cw, ch, 4.0 * s, fill);
            let label = k.map(Category::label).unwrap_or("All");
            let color = if selected || hover { TEXT } else { Rgba(200, 208, 228, 255) };
            ctx.label(c, label, cx + cw / 2.0, cy + ch / 2.0, 10.0, Weight::Bold, color, Align::Center);
        }
    }

    fn render_rows(&self, c: &mut Canvas, ctx: &mut Ctx) {
        let s = ctx.s;
        let x = Self::PAD * s;
        let iw = c.width() - 2.0 * Self::PAD * s;
        let base = if ctx.opaque { BG_OPAQUE } else { Rgba(16, 20, 34, 255) };
        let top = Self::header_h() * s - self.scroll;
        if self.rows.is_empty() {
            let note = ctx.fonts.fit(&self.note, 11.5 * s, Weight::Medium, iw - 16.0 * s);
            ctx.label(c, &note, c.width() / 2.0, top + Self::ROW * s / 2.0, 11.5, Weight::Medium, DIM, Align::Center);
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
                c.rect(x, ry, iw, h, Rgba(255, 255, 255, 28));
                c.rect(x, ry, 3.0 * s, h, GOLD);
            }
            let max = iw - if row.tier > 0 { 40.0 * s } else { 18.0 * s };
            let name = ctx.fonts.fit(&row.name, 12.5 * s, Weight::Bold, max);
            ctx.label_shadow(c, &name, x + 11.0 * s, cy - 7.0 * s, 12.5, Weight::Bold, TEXT);
            let detail = ctx.fonts.fit(&row.detail, 10.0 * s, Weight::Medium, max);
            ctx.label_shadow(c, &detail, x + 11.0 * s, cy + 8.0 * s, 10.0, Weight::Medium, Rgba(180, 190, 215, 255));
            if row.tier > 0 {
                let tx = x + iw - 14.0 * s;
                c.circle(tx, cy, 9.5 * s, Rgba(0, 0, 0, 170));
                c.circle(tx, cy, 8.5 * s, ACCENT);
                ctx.label(c, &row.tier.to_string(), tx, cy, 11.0, Weight::Bold, TEXT, Align::Center);
            }
        }
        if self.more > 0 {
            let y = top + self.rows.len() as f32 * rh + Self::FOOT * s / 2.0;
            let msg = format!("+{} more · keep typing to narrow down", self.more);
            let msg = ctx.fonts.fit(&msg, 10.5 * s, Weight::Medium, iw - 16.0 * s);
            ctx.label(c, &msg, c.width() / 2.0, y, 10.5, Weight::Medium, DIM, Align::Center);
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
        ctx.panel(c);
        let col_w = Self::W * s / 5.0;
        let fade = |c: Rgba, dim: bool| if dim { c.alpha(150) } else { c };
        let cols: [(&str, Option<f32>, Rgba); 5] = [
            ("LETHAL", self.odds.map(|o| o.won_lethal), WIN_LETHAL),
            ("WIN", self.odds.map(|o| o.won), WIN),
            ("TIE", self.odds.map(|o| o.tied), TIE),
            ("LOSS", self.odds.map(|o| o.lost), LOSS),
            ("LETHAL", self.odds.map(|o| o.lost_lethal), LOSS_LETHAL),
        ];
        for (i, (label, value, color)) in cols.iter().enumerate() {
            let cx = col_w * (i as f32 + 0.5);
            let lethal = i == 0 || i == 4;
            let label_color = if lethal { DIM.alpha(170) } else { DIM };
            ctx.label(c, label, cx, 13.0 * s, 9.5, Weight::Bold, fade(label_color, self.dim), Align::Center);
            let text = value.map(percent).unwrap_or_else(|| "–".into());
            let zero = value.is_none_or(|v| v <= 0.0);
            let vc = if lethal && zero { DIM } else { *color };
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
        let fc = if self.note.is_some() { LOSS.alpha(220) } else { DIM };
        ctx.label(c, &footer, Self::W * s / 2.0, 52.0 * s, 10.5, Weight::Medium, fc, Align::Center);
        if let Some(o) = self.odds {
            let (x0, y, w, h) = (10.0 * s, (Self::H - 9.0) * s, (Self::W - 20.0) * s, 3.0 * s);
            let total = (o.won + o.tied + o.lost).max(0.001);
            let mut x = x0;
            for (v, col) in [(o.won, WIN), (o.tied, TIE), (o.lost, LOSS)] {
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
pub struct InfoPanel {
    pub tribes: Vec<Tribe>,
    pub slots: usize,
    pub games: Vec<(String, i32)>,
    /// Games this session and their average place.
    pub session: Option<(usize, f32)>,
    /// Unscaled width chosen by the layout.
    pub width: f32,
}

pub fn place_color(place: i32) -> Rgba {
    match place {
        1 => GOLD,
        2..=4 => WIN,
        _ => Rgba(120, 128, 150, 255),
    }
}

impl InfoPanel {
    const PAD: f32 = 8.0;
    const TITLE: f32 = 20.0;
    const CHIP: f32 = 21.0;
    const GAME: f32 = 20.0;

    fn height(&self) -> f32 {
        let games = self.games.len().max(1) as f32;
        Self::PAD
            + Self::TITLE
            + self.slots as f32 * (Self::CHIP + 3.0)
            + 6.0
            + Self::TITLE
            + games * Self::GAME
            + if self.session.is_some() { Self::GAME } else { 0.0 }
            + Self::PAD
    }

    pub fn size(&self, s: f32) -> (i32, i32) {
        (s_ceil(self.width, s), s_ceil(self.height(), s))
    }

    pub fn render(&self, c: &mut Canvas, ctx: &mut Ctx) {
        let s = ctx.s;
        ctx.panel(c);
        let x = Self::PAD * s;
        let w = (self.width - 2.0 * Self::PAD) * s;
        let mut y = Self::PAD * s;
        ctx.label(c, "AVAILABLE TRIBES", x, y + Self::TITLE * s / 2.0, 10.0, Weight::Bold, DIM, Align::Left);
        y += Self::TITLE * s;
        for i in 0..self.slots {
            let h = Self::CHIP * s;
            match self.tribes.get(i) {
                Some(&t) => {
                    c.rrect(x, y, w, h, 4.0 * s, tribe_color(t));
                    ctx.label_shadow(c, t.label(), x + 8.0 * s, y + h / 2.0, 12.0, Weight::Bold, TEXT);
                }
                None => {
                    c.rrect(x, y, w, h, 4.0 * s, BUTTON);
                    ctx.label(c, "?", x + w / 2.0, y + h / 2.0, 12.0, Weight::Bold, DIM, Align::Center);
                }
            }
            y += h + 3.0 * s;
        }
        y += 6.0 * s;
        ctx.label(c, "LATEST GAMES", x, y + Self::TITLE * s / 2.0, 10.0, Weight::Bold, DIM, Align::Left);
        y += Self::TITLE * s;
        if self.games.is_empty() {
            ctx.label(c, "No games yet", x, y + Self::GAME * s / 2.0, 11.0, Weight::Medium, DIM, Align::Left);
            y += Self::GAME * s;
        }
        for (hero, place) in &self.games {
            let cy = y + Self::GAME * s / 2.0;
            let r = 8.0 * s;
            c.circle(x + r, cy, r, place_color(*place));
            ctx.label(c, &place.to_string(), x + r, cy, 10.5, Weight::Bold, Rgba(15, 18, 28, 255), Align::Center);
            let name = ctx.fonts.fit(hero, 11.5 * s, Weight::Medium, w - 2.0 * r - 6.0 * s);
            ctx.label(c, &name, x + 2.0 * r + 6.0 * s, cy, 11.5, Weight::Medium, TEXT, Align::Left);
            y += Self::GAME * s;
        }
        if let Some((n, avg)) = self.session {
            let text = format!("Session: {n} · avg {avg:.1}");
            let text = ctx.fonts.fit(&text, 11.0 * s, Weight::Medium, w);
            ctx.label(c, &text, x, y + Self::GAME * s / 2.0, 11.0, Weight::Medium, DIM, Align::Left);
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
}

impl OpponentPopup {
    const W: f32 = Self::PAD * 2.0 + Self::SLOTS as f32 * Self::MINION_W + (Self::SLOTS - 1) as f32 * Self::MINION_GAP;
    const HEAD: f32 = 52.0;
    const CHIP_COLS: usize = 10;
    const CHIP_H: f32 = 20.0;
    const GAP: f32 = 4.0;
    const BOARD_HEAD: f32 = 22.0;
    /// A board laid out like in game: up to seven minions side by side.
    const SLOTS: usize = 7;
    const MINION_W: f32 = 64.0;
    const MINION_GAP: f32 = 6.0;
    const PORTRAIT_H: f32 = 82.0;
    /// Portrait, the stat gems hanging below it and a two-line name.
    const MINIONS_H: f32 = 8.0 + Self::PORTRAIT_H + 8.0 + 24.0;
    const ROW: f32 = 32.0;
    const MORE: f32 = 20.0;
    const PAD: f32 = 6.0;

    fn chips_h(&self) -> f32 {
        if self.boards.is_empty() {
            return 0.0;
        }
        self.boards.len().div_ceil(Self::CHIP_COLS) as f32 * (Self::CHIP_H + Self::GAP) + Self::GAP
    }

    fn chip_box(i: usize) -> Box2 {
        let inner = Self::W - 2.0 * Self::PAD;
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

    fn board_h(b: &BoardView) -> f32 {
        Self::BOARD_HEAD + if b.minions.is_empty() { Self::ROW } else { Self::MINIONS_H }
    }

    fn content_h(&self) -> f32 {
        let boards = self.visible().map(|(_, b)| Self::board_h(b)).sum::<f32>();
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
        (s_ceil(Self::W, s), s_ceil(self.content_h(), s))
    }

    /// Turn chip at a point.
    pub fn hit(&self, x: f32, y: f32, s: f32) -> Option<usize> {
        (0..self.boards.len()).find(|&i| Self::chip_box(i).contains(x / s, y / s))
    }

    pub fn render(&self, c: &mut Canvas, ctx: &mut Ctx) {
        let s = ctx.s;
        ctx.panel(c);
        let w = c.width();
        let x = Self::PAD * s;
        let iw = w - 2.0 * Self::PAD * s;
        // Header: hero art strip behind the name.
        let hh = (Self::HEAD - Self::PAD) * s;
        ctx.tile_row(c, &self.hero_card_id, x, x, iw, hh, HEADER);
        let name = ctx.fonts.fit(&self.hero, 14.0 * s, Weight::Bold, iw - 20.0 * s);
        ctx.label_shadow(c, &name, x + 10.0 * s, x + 15.0 * s, 14.0, Weight::Bold, TEXT);
        let faced = self.boards.len();
        let sub = match self.boards.first() {
            Some(b) => format!("Tier {} · faced {faced} time{}", b.tier, if faced == 1 { "" } else { "s" }),
            None => "Not faced yet".to_string(),
        };
        let sub = ctx.fonts.fit(&sub, 11.0 * s, Weight::Medium, iw - 20.0 * s);
        ctx.label_shadow(c, &sub, x + 10.0 * s, x + 34.0 * s, 11.0, Weight::Medium, DIM);
        let mut y = Self::HEAD * s;
        if self.boards.is_empty() {
            ctx.label(c, "No board seen yet", w / 2.0, y + Self::ROW * s / 2.0, 12.0, Weight::Medium, DIM, Align::Center);
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
            ctx.label(c, "Click a turn to show its board", w / 2.0, y + Self::ROW * s / 2.0, 11.5, Weight::Medium, DIM, Align::Center);
        }
        if self.hidden > 0 {
            let msg = format!("+{} older board{} not shown", self.hidden, if self.hidden == 1 { "" } else { "s" });
            ctx.label(c, &msg, w / 2.0, y + Self::MORE * s / 2.0, 10.5, Weight::Medium, DIM, Align::Center);
        }
    }

    fn render_chips(&self, c: &mut Canvas, ctx: &mut Ctx) {
        let s = ctx.s;
        for (i, b) in self.boards.iter().enumerate() {
            let (cx, cy, cw, ch) = Self::chip_box(i).scaled(s);
            let on = self.shown[i];
            let hover = self.hover == Some(i);
            let fill = match (on, hover) {
                (true, _) => ACCENT,
                (false, true) => BUTTON_HOVER,
                (false, false) => BUTTON,
            };
            c.rrect(cx, cy, cw, ch, 4.0 * s, fill);
            if hover && on {
                c.rrect(cx, cy, cw, ch, 4.0 * s, Rgba(255, 255, 255, 30));
            }
            // The latest board gets the same gold mark as its header.
            if i == 0 {
                c.rect(cx + 3.0 * s, cy + ch - 3.0 * s, cw - 6.0 * s, 1.5 * s, GOLD);
            }
            let color = if on || hover { TEXT } else { Rgba(160, 170, 195, 255) };
            ctx.label(c, &format!("T{}", b.turn), cx + cw / 2.0, cy + ch / 2.0, 10.5, Weight::Bold, color, Align::Center);
        }
    }

    fn render_board(&self, c: &mut Canvas, ctx: &mut Ctx, b: &BoardView, latest: bool, mut y: f32) -> f32 {
        let s = ctx.s;
        let x = Self::PAD * s;
        let iw = c.width() - 2.0 * Self::PAD * s;
        let w = c.width();
        // Board header: turn, tier and how long ago.
        let bh = (Self::BOARD_HEAD - 2.0) * s;
        let by = y + 1.0 * s;
        c.rect(x, by, iw, bh, HEADER);
        c.rect(x, by, 3.0 * s, bh, if latest { GOLD } else { DIM });
        let ago = self.turn - b.turn;
        let when = match ago {
            0 => "this turn".to_string(),
            1 => "1 turn ago".to_string(),
            n => format!("{n} turns ago"),
        };
        let title = format!("Turn {} · Tier {}", b.turn, b.tier);
        ctx.label(c, &title, x + 9.0 * s, by + bh / 2.0, 11.5, Weight::Bold, if latest { TEXT } else { Rgba(200, 208, 228, 255) }, Align::Left);
        ctx.label(c, &when, x + iw - 7.0 * s, by + bh / 2.0, 10.5, Weight::Medium, DIM, Align::Right);
        y += Self::BOARD_HEAD * s;
        if b.minions.is_empty() {
            ctx.label(c, "Empty board", w / 2.0, y + Self::ROW * s / 2.0, 12.0, Weight::Medium, DIM, Align::Center);
            return y + Self::ROW * s;
        }
        // Centred like the game centres a board that is not full.
        let n = b.minions.len().min(Self::SLOTS) as f32;
        let row_w = n * Self::MINION_W + (n - 1.0) * Self::MINION_GAP;
        let x0 = (Self::W - row_w) / 2.0;
        for (i, m) in b.minions.iter().take(Self::SLOTS).enumerate() {
            let mx = (x0 + i as f32 * (Self::MINION_W + Self::MINION_GAP)) * s;
            Self::render_minion(c, ctx, m, mx, y + 8.0 * s);
        }
        y + Self::MINIONS_H * s
    }

    /// One minion as on the game board: portrait, attack bottom left,
    /// health bottom right, keyword effects around it, name below.
    fn render_minion(c: &mut Canvas, ctx: &mut Ctx, m: &MinionView, x: f32, y: f32) {
        let s = ctx.s;
        let (w, h) = (Self::MINION_W * s, Self::PORTRAIT_H * s);
        let r = w / 2.0;
        let has = |k: &str| m.keywords.contains(&k);
        if has("Taunt") {
            // Stone shield behind the portrait.
            let (tx, ty, tw, th) = (x - 4.0 * s, y - 3.0 * s, w + 8.0 * s, h + 5.0 * s);
            c.rrect(tx, ty, tw, th, 12.0 * s, Rgba(132, 136, 148, 255));
            c.stroke_rrect(tx, ty, tw, th, 12.0 * s, 1.5 * s, Rgba(60, 62, 72, 255));
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
        let frame = if m.golden { GOLD } else { Rgba(96, 84, 70, 255) };
        c.stroke_rrect(x, y, w, h, r, 2.0 * s, frame);
        if has("Shield") {
            c.stroke_rrect(x - 2.5 * s, y - 2.5 * s, w + 5.0 * s, h + 5.0 * s, r + 2.5 * s, 3.0 * s, Rgba(255, 214, 90, 210));
        }
        // Small badges along the top for keywords without a look of their own.
        let badges: Vec<(&str, Rgba)> = [
            ("Venomous", "V", Rgba(70, 170, 60, 255)),
            ("Windfury", "W", Rgba(70, 150, 220, 255)),
            ("Mega-Windfury", "M", Rgba(70, 110, 230, 255)),
        ]
        .into_iter()
        .filter(|(k, ..)| has(k))
        .map(|(_, l, col)| (l, col))
        .collect();
        let bw = 14.0 * s;
        let bx0 = x + w / 2.0 - (badges.len() as f32 * bw) / 2.0 + bw / 2.0;
        for (i, (label, col)) in badges.iter().enumerate() {
            let (cx, cy) = (bx0 + i as f32 * bw, y + 4.0 * s);
            c.circle(cx, cy, 6.5 * s, Rgba(0, 0, 0, 200));
            c.circle(cx, cy, 5.5 * s, *col);
            ctx.label(c, label, cx, cy, 8.0, Weight::Bold, TEXT, Align::Center);
        }
        // Stats where the game puts them.
        let gy = y + h - 6.0 * s;
        for (cx, value, color) in [(x + 9.0 * s, m.attack, ATTACK), (x + w - 9.0 * s, m.health, HEALTH)] {
            c.circle(cx, gy, 11.5 * s, Rgba(0, 0, 0, 220));
            c.circle(cx, gy, 10.0 * s, color);
            let text = value.to_string();
            let px = match text.len() {
                1 | 2 => 12.5,
                3 => 10.0,
                _ => 8.5,
            };
            c.text_shadow(ctx.fonts, &text, cx, ctx.mid(gy, px * s, Weight::Bold), px * s, Weight::Bold, TEXT, Align::Center);
        }
        let lines = ctx.fonts.wrap(&m.name, 9.5 * s, Weight::Bold, w + Self::MINION_GAP * s - 2.0 * s);
        let name_color = if m.golden { GOLD } else { Rgba(220, 224, 236, 255) };
        for (i, line) in lines.iter().take(2).enumerate() {
            let cy = y + h + 15.0 * s + i as f32 * 11.0 * s;
            ctx.label(c, line, x + w / 2.0, cy, 9.5, Weight::Bold, name_color, Align::Center);
        }
    }
}

// ---- card preview --------------------------------------------------------

#[derive(Debug, Clone, PartialEq)]
pub struct CardPreview {
    pub card_id: String,
    pub images_rev: u64,
}

impl CardPreview {
    const W: f32 = 230.0;

    /// Size once the image has loaded.
    pub fn size(&self, s: f32, images: &mut Images) -> Option<(i32, i32)> {
        let img = images.get(ImageKind::Render, &self.card_id)?;
        let w = Self::W * s;
        Some((w.ceil() as i32, (w * img.height() as f32 / img.width() as f32).ceil() as i32))
    }

    pub fn render(&self, c: &mut Canvas, ctx: &mut Ctx) {
        if let Some(img) = ctx.images.get(ImageKind::Render, &self.card_id) {
            if ctx.opaque {
                c.rect(0.0, 0.0, c.width(), c.height(), BG_OPAQUE);
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
        for (i, &(x, y, w, h)) in self.slots.iter().enumerate() {
            let color = if self.hover == Some(i) { GOLD } else { Rgba(80, 220, 255, 255) };
            c.stroke_rrect(x, y, w, h, 4.0, 2.0, color);
            ctx.label(c, &format!("{}", i + 1), x + 6.0, y + 12.0, 12.0, Weight::Bold, color, Align::Left);
        }
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
        let (x, y, w, h) = OpponentPopup::chip_box(1).scaled(2.0);
        assert_eq!(p.hit(x + w / 2.0, y + h / 2.0, 2.0), Some(1));
        assert_eq!(p.hit(1.0, 1.0, 2.0), None);
        // Nothing toggled on: room for the hint.
        let none = popup(vec![board(9)], vec![false]);
        assert!(none.size(1.0).1 > 0);
    }
}
