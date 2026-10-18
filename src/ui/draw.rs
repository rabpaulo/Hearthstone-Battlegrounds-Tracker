//! Software rendering: shapes and images through tiny-skia, text through
//! fontdue with a glyph cache.

use std::collections::HashMap;

use tiny_skia::{
    Color, FillRule, FilterQuality, GradientStop, LinearGradient, Paint, Path, PathBuilder, Pattern, Pixmap, Point,
    Rect as SkRect, SpreadMode, Transform,
};

static BOLD: &[u8] = include_bytes!("../../assets/fonts/NotoSans-Bold.ttf");
static MEDIUM: &[u8] = include_bytes!("../../assets/fonts/NotoSans-Medium.ttf");

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Weight {
    Medium,
    Bold,
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Rgba(pub u8, pub u8, pub u8, pub u8);

impl Rgba {
    pub const fn hex(v: u32) -> Rgba {
        Rgba((v >> 16) as u8, (v >> 8) as u8, v as u8, 255)
    }

    pub const fn alpha(self, a: u8) -> Rgba {
        Rgba(self.0, self.1, self.2, a)
    }

    fn sk(self) -> Color {
        Color::from_rgba8(self.0, self.1, self.2, self.3)
    }
}

pub struct Fonts {
    fonts: [fontdue::Font; 2],
    cache: HashMap<(Weight, char, u32), (fontdue::Metrics, Vec<u8>)>,
}

impl Fonts {
    pub fn load() -> Fonts {
        let settings = fontdue::FontSettings::default();
        let load = |b: &[u8]| fontdue::Font::from_bytes(b, settings).expect("bundled font is valid");
        Fonts { fonts: [load(MEDIUM), load(BOLD)], cache: HashMap::new() }
    }

    fn font(&self, w: Weight) -> &fontdue::Font {
        &self.fonts[w as usize]
    }

    fn glyph(&mut self, w: Weight, c: char, px: f32) -> &(fontdue::Metrics, Vec<u8>) {
        let key = (w, c, (px * 4.0) as u32);
        if !self.cache.contains_key(&key) {
            let g = self.fonts[w as usize].rasterize(c, px);
            if self.cache.len() > 8192 {
                self.cache.clear();
            }
            self.cache.insert(key, g);
        }
        &self.cache[&key]
    }

    pub fn measure(&self, text: &str, px: f32, w: Weight) -> f32 {
        let font = self.font(w);
        let mut width = 0.0;
        let mut prev = None;
        for c in text.chars() {
            if let Some(p) = prev {
                width += font.horizontal_kern(p, c, px).unwrap_or(0.0);
            }
            width += font.metrics(c, px).advance_width;
            prev = Some(c);
        }
        width
    }

    /// Distance from the top of a line box to its baseline.
    pub fn ascent(&self, px: f32, w: Weight) -> f32 {
        self.font(w).horizontal_line_metrics(px).map(|m| m.ascent).unwrap_or(px * 0.8)
    }

    /// Shortens `text` with an ellipsis so it fits in `max` pixels.
    pub fn fit(&self, text: &str, px: f32, w: Weight, max: f32) -> String {
        if self.measure(text, px, w) <= max {
            return text.to_string();
        }
        let mut chars: Vec<char> = text.chars().collect();
        while !chars.is_empty() {
            chars.pop();
            let s: String = chars.iter().collect::<String>().trim_end().to_string() + "…";
            if self.measure(&s, px, w) <= max {
                return s;
            }
        }
        String::new()
    }

    /// Breaks `text` into lines no wider than `max` pixels, keeping its own
    /// line breaks. A word too long for a line is shortened with an ellipsis.
    pub fn wrap(&self, text: &str, px: f32, w: Weight, max: f32) -> Vec<String> {
        let mut lines = Vec::new();
        for para in text.lines().map(str::trim).filter(|p| !p.is_empty()) {
            let mut line = String::new();
            for word in para.split_whitespace() {
                let candidate = if line.is_empty() { word.to_string() } else { format!("{line} {word}") };
                if line.is_empty() || self.measure(&candidate, px, w) <= max {
                    line = candidate;
                } else {
                    lines.push(self.fit(&line, px, w, max));
                    line = word.to_string();
                }
            }
            if !line.is_empty() {
                lines.push(self.fit(&line, px, w, max));
            }
        }
        lines
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Align {
    Left,
    Center,
    Right,
}

pub struct Canvas {
    pub pixmap: Pixmap,
}

fn rrect_path(x: f32, y: f32, w: f32, h: f32, r: f32) -> Option<Path> {
    let r = r.min(w / 2.0).min(h / 2.0).max(0.0);
    let mut pb = PathBuilder::new();
    if r < 0.5 {
        pb.push_rect(SkRect::from_xywh(x, y, w, h)?);
        return pb.finish();
    }
    // Cubic approximation of quarter circles.
    let k = r * 0.5523;
    pb.move_to(x + r, y);
    pb.line_to(x + w - r, y);
    pb.cubic_to(x + w - r + k, y, x + w, y + r - k, x + w, y + r);
    pb.line_to(x + w, y + h - r);
    pb.cubic_to(x + w, y + h - r + k, x + w - r + k, y + h, x + w - r, y + h);
    pb.line_to(x + r, y + h);
    pb.cubic_to(x + r - k, y + h, x, y + h - r + k, x, y + h - r);
    pb.line_to(x, y + r);
    pb.cubic_to(x, y + r - k, x + r - k, y, x + r, y);
    pb.close();
    pb.finish()
}

impl Canvas {
    pub fn new(w: i32, h: i32) -> Canvas {
        Canvas { pixmap: Pixmap::new(w.max(1) as u32, h.max(1) as u32).expect("non-zero size") }
    }

    pub fn width(&self) -> f32 {
        self.pixmap.width() as f32
    }

    pub fn height(&self) -> f32 {
        self.pixmap.height() as f32
    }

    pub fn rgba(&self) -> &[u8] {
        self.pixmap.data()
    }

    pub fn rrect(&mut self, x: f32, y: f32, w: f32, h: f32, r: f32, color: Rgba) {
        let Some(path) = rrect_path(x, y, w, h, r) else { return };
        let mut paint = Paint::default();
        paint.set_color(color.sk());
        paint.anti_alias = true;
        self.pixmap.fill_path(&path, &paint, FillRule::Winding, Transform::identity(), None);
    }

    pub fn rect(&mut self, x: f32, y: f32, w: f32, h: f32, color: Rgba) {
        self.rrect(x, y, w, h, 0.0, color);
    }

    #[allow(clippy::too_many_arguments)]
    pub fn stroke_rrect(&mut self, x: f32, y: f32, w: f32, h: f32, r: f32, width: f32, color: Rgba) {
        let Some(path) = rrect_path(x + width / 2.0, y + width / 2.0, w - width, h - width, r) else { return };
        let mut paint = Paint::default();
        paint.set_color(color.sk());
        paint.anti_alias = true;
        let stroke = tiny_skia::Stroke { width, ..Default::default() };
        self.pixmap.stroke_path(&path, &paint, &stroke, Transform::identity(), None);
    }

    pub fn triangle(&mut self, points: [(f32, f32); 3], color: Rgba) {
        let mut pb = PathBuilder::new();
        pb.move_to(points[0].0, points[0].1);
        pb.line_to(points[1].0, points[1].1);
        pb.line_to(points[2].0, points[2].1);
        pb.close();
        let Some(path) = pb.finish() else { return };
        let mut paint = Paint::default();
        paint.set_color(color.sk());
        paint.anti_alias = true;
        self.pixmap.fill_path(&path, &paint, FillRule::Winding, Transform::identity(), None);
    }

    pub fn circle(&mut self, cx: f32, cy: f32, r: f32, color: Rgba) {
        self.rrect(cx - r, cy - r, r * 2.0, r * 2.0, r, color);
    }

    /// Horizontal gradient over a rectangle.
    pub fn hgradient(&mut self, x: f32, y: f32, w: f32, h: f32, from: Rgba, to: Rgba) {
        let Some(rect) = SkRect::from_xywh(x, y, w, h) else { return };
        let Some(shader) = LinearGradient::new(
            Point::from_xy(x, y),
            Point::from_xy(x + w, y),
            vec![GradientStop::new(0.0, from.sk()), GradientStop::new(1.0, to.sk())],
            SpreadMode::Pad,
            Transform::identity(),
        ) else {
            return;
        };
        let paint = Paint { shader, anti_alias: false, ..Default::default() };
        self.pixmap.fill_rect(rect, &paint, Transform::identity(), None);
    }

    /// Draws `img` scaled to cover the destination box (centre-cropped),
    /// clipped to a rounded rectangle.
    #[allow(clippy::too_many_arguments)]
    pub fn image_cover(&mut self, img: &Pixmap, x: f32, y: f32, w: f32, h: f32, r: f32, opacity: f32) {
        let Some(path) = rrect_path(x, y, w, h, r) else { return };
        let (iw, ih) = (img.width() as f32, img.height() as f32);
        let scale = (w / iw).max(h / ih);
        let (dw, dh) = (iw * scale, ih * scale);
        let tf = Transform::from_scale(scale, scale).post_translate(x + (w - dw) / 2.0, y + (h - dh) / 2.0);
        let shader = Pattern::new(img.as_ref(), SpreadMode::Pad, FilterQuality::Bilinear, opacity, tf);
        let paint = Paint { shader, anti_alias: true, ..Default::default() };
        self.pixmap.fill_path(&path, &paint, FillRule::Winding, Transform::identity(), None);
    }

    /// Draws text with its line box top at `y`. Returns the advance width.
    #[allow(clippy::too_many_arguments)]
    pub fn text(
        &mut self,
        fonts: &mut Fonts,
        text: &str,
        x: f32,
        y: f32,
        px: f32,
        weight: Weight,
        color: Rgba,
        align: Align,
    ) -> f32 {
        let width = fonts.measure(text, px, weight);
        let mut pen = match align {
            Align::Left => x,
            Align::Center => x - width / 2.0,
            Align::Right => x - width,
        };
        let baseline = y + fonts.ascent(px, weight);
        let mut prev = None;
        for c in text.chars() {
            if let Some(p) = prev {
                pen += fonts.font(weight).horizontal_kern(p, c, px).unwrap_or(0.0);
            }
            let (m, bitmap) = fonts.glyph(weight, c, px);
            let gx = (pen + m.xmin as f32).round() as i32;
            let gy = (baseline - m.height as f32 - m.ymin as f32).round() as i32;
            let m = *m;
            self.blend_mask(gx, gy, m.width, m.height, bitmap, color);
            pen += m.advance_width;
            prev = Some(c);
        }
        width
    }

    /// Text with a drop shadow, for legibility over card art.
    #[allow(clippy::too_many_arguments)]
    pub fn text_shadow(
        &mut self,
        fonts: &mut Fonts,
        text: &str,
        x: f32,
        y: f32,
        px: f32,
        weight: Weight,
        color: Rgba,
        align: Align,
        shadow: Rgba,
    ) -> f32 {
        let off = (px / 14.0).max(1.0);
        self.text(fonts, text, x + off, y + off, px, weight, shadow, align);
        self.text(fonts, text, x, y, px, weight, color, align)
    }

    fn blend_mask(&mut self, x0: i32, y0: i32, w: usize, h: usize, mask: &[u8], color: Rgba) {
        let (pw, ph) = (self.pixmap.width() as i32, self.pixmap.height() as i32);
        let data = self.pixmap.data_mut();
        for row in 0..h as i32 {
            let y = y0 + row;
            if y < 0 || y >= ph {
                continue;
            }
            for col in 0..w as i32 {
                let x = x0 + col;
                if x < 0 || x >= pw {
                    continue;
                }
                let cov = mask[(row as usize) * w + col as usize] as u32;
                if cov == 0 {
                    continue;
                }
                let a = cov * color.3 as u32 / 255;
                let i = ((y * pw + x) * 4) as usize;
                let inv = 255 - a;
                // Source-over with premultiplied destination.
                data[i] = ((color.0 as u32 * a + data[i] as u32 * inv) / 255) as u8;
                data[i + 1] = ((color.1 as u32 * a + data[i + 1] as u32 * inv) / 255) as u8;
                data[i + 2] = ((color.2 as u32 * a + data[i + 2] as u32 * inv) / 255) as u8;
                data[i + 3] = (a + data[i + 3] as u32 * inv / 255) as u8;
            }
        }
    }
}
