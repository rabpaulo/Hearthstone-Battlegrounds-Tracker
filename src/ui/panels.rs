//! Panel models and their rendering. Models are plain data compared with
//! `PartialEq`; a panel is only re-rendered when its model changes.
//!
//! Layout constants are in unscaled pixels at a 1080p game window; `Ctx::s`
//! scales them to the actual window.

use super::draw::{Align, Canvas, Fonts, Rgba, Weight};
use crate::cards::{Tribe, KEYWORDS};
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

#[derive(Debug, Clone, PartialEq)]
pub struct TopBar {
    pub turn: i32,
    pub tavern_tier: i32,
    pub max_tier: i32,
    pub selected: Option<i32>,
    pub hover: Option<i32>,
}

impl TopBar {
    const H: f32 = 38.0;
    const TURN_W: f32 = 84.0;
    const BTN: f32 = 30.0;
    const GAP: f32 = 4.0;
    const PAD: f32 = 4.0;

    pub fn size(&self, s: f32) -> (i32, i32) {
        let w = Self::PAD * 2.0 + Self::TURN_W + Self::GAP * 2.0 + self.max_tier as f32 * (Self::BTN + Self::GAP)
            - Self::GAP;
        (s_ceil(w, s), s_ceil(Self::H, s))
    }

    fn button_x(tier: i32) -> f32 {
        Self::PAD + Self::TURN_W + Self::GAP * 2.0 + (tier - 1) as f32 * (Self::BTN + Self::GAP)
    }

    pub fn hit(&self, x: f32, s: f32) -> Option<i32> {
        let x = x / s;
        (1..=self.max_tier).find(|&t| {
            let bx = Self::button_x(t);
            x >= bx && x < bx + Self::BTN
        })
    }

    pub fn render(&self, c: &mut Canvas, ctx: &mut Ctx) {
        let s = ctx.s;
        ctx.panel(c);
        let label = if self.turn > 0 { format!("Turn {}", self.turn) } else { "Hero pick".to_string() };
        ctx.label(c, &label, (Self::PAD + Self::TURN_W / 2.0) * s, Self::H / 2.0 * s, 15.0, Weight::Bold, TEXT, Align::Center);
        let sep_x = (Self::PAD + Self::TURN_W + Self::GAP) * s;
        c.rect(sep_x, 7.0 * s, 1.0, (Self::H - 14.0) * s, BORDER);
        let by = (Self::H - Self::BTN) / 2.0 * s;
        let b = Self::BTN * s;
        for t in 1..=self.max_tier {
            let bx = Self::button_x(t) * s;
            let fill = if self.selected == Some(t) {
                ACCENT
            } else if self.hover == Some(t) {
                BUTTON_HOVER
            } else {
                BUTTON
            };
            c.rrect(bx, by, b, b, 5.0 * s, fill);
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

#[derive(Debug, Clone, PartialEq)]
pub struct OpponentPopup {
    pub hero: String,
    pub hero_card_id: String,
    pub place: i32,
    pub health: i32,
    pub tier: i32,
    pub seen_turn: Option<i32>,
    pub turn: i32,
    pub minions: Vec<MinionView>,
    pub images_rev: u64,
}

impl OpponentPopup {
    const W: f32 = 300.0;
    const HEAD: f32 = 52.0;
    const ROW: f32 = 32.0;
    const PAD: f32 = 6.0;

    pub fn size(&self, s: f32) -> (i32, i32) {
        let rows = self.minions.len().max(1) as f32;
        (s_ceil(Self::W, s), s_ceil(Self::HEAD + rows * Self::ROW + Self::PAD, s))
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
        let seen = match self.seen_turn {
            Some(t) if t == self.turn => format!("Tier {} · board from this turn", self.tier),
            Some(t) => {
                let ago = self.turn - t;
                format!("Tier {} · board from turn {t} ({ago} turn{} ago)", self.tier, if ago == 1 { "" } else { "s" })
            }
            None => "Not faced yet".to_string(),
        };
        let seen = ctx.fonts.fit(&seen, 11.0 * s, Weight::Medium, iw - 20.0 * s);
        ctx.label_shadow(c, &seen, x + 10.0 * s, x + 34.0 * s, 11.0, Weight::Medium, DIM);
        let mut y = Self::HEAD * s;
        if self.minions.is_empty() {
            let msg = if self.seen_turn.is_some() { "Empty board" } else { "No board seen yet" };
            ctx.label(c, msg, w / 2.0, y + Self::ROW * s / 2.0, 12.0, Weight::Medium, DIM, Align::Center);
            return;
        }
        let base = if ctx.opaque { BG_OPAQUE } else { Rgba(16, 20, 34, 255) };
        for m in &self.minions {
            let rh = Self::ROW * s;
            ctx.tile_row(c, &m.card_id, x, y, iw, rh - 2.0 * s, base);
            let cy = y + (rh - 2.0 * s) / 2.0;
            // Attack / health gems.
            let gw = 26.0 * s;
            let gh = 20.0 * s;
            c.rrect(x + 4.0 * s, cy - gh / 2.0, gw, gh, 4.0 * s, ATTACK);
            ctx.label(c, &m.attack.to_string(), x + 4.0 * s + gw / 2.0, cy, 12.0, Weight::Bold, Rgba(30, 20, 0, 255), Align::Center);
            c.rrect(x + 8.0 * s + gw, cy - gh / 2.0, gw, gh, 4.0 * s, HEALTH);
            ctx.label(c, &m.health.to_string(), x + 8.0 * s + gw * 1.5, cy, 12.0, Weight::Bold, TEXT, Align::Center);
            let tx = x + 14.0 * s + gw * 2.0;
            let name_color = if m.golden { GOLD } else { TEXT };
            let has_kw = !m.keywords.is_empty();
            let name_cy = if has_kw { cy - 6.0 * s } else { cy };
            let name = ctx.fonts.fit(&m.name, 12.0 * s, Weight::Bold, x + iw - tx - 6.0 * s);
            ctx.label_shadow(c, &name, tx, name_cy, 12.0, Weight::Bold, name_color);
            if has_kw {
                let kw = m.keywords.join(" · ");
                let kw = ctx.fonts.fit(&kw, 9.5 * s, Weight::Medium, x + iw - tx - 6.0 * s);
                ctx.label_shadow(c, &kw, tx, cy + 7.0 * s, 9.5, Weight::Medium, Rgba(190, 200, 225, 255));
            }
            y += rh;
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
