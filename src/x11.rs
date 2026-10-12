//! X11 plumbing: finding the Hearthstone window and drawing overlay windows.
//!
//! Overlay windows are override-redirect, so tiling window managers such as
//! dwm never manage, tile or focus them; the overlay positions them itself
//! and keeps them stacked above the game. With a compositor they use a 32-bit
//! ARGB visual for real transparency; without one, panels are painted opaque.

use anyhow::{Context, Result};
use x11rb::connection::{Connection, RequestConnection};
use x11rb::protocol::shape::{self, ConnectionExt as _};
use x11rb::protocol::xproto::{
    AtomEnum, ClipOrdering, ColormapAlloc, ConfigureWindowAux, ConnectionExt as _, CreateGCAux, CreateWindowAux,
    EventMask, ImageFormat, MapState, PropMode, StackMode, VisualClass, Window, WindowClass,
};
use x11rb::protocol::Event;
use x11rb::rust_connection::RustConnection;
use x11rb::wrapper::ConnectionExt as _;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct Rect {
    pub x: i32,
    pub y: i32,
    pub w: i32,
    pub h: i32,
}

impl Rect {
    pub fn new(x: i32, y: i32, w: i32, h: i32) -> Rect {
        Rect { x, y, w, h }
    }

    pub fn contains(&self, px: i32, py: i32) -> bool {
        px >= self.x && py >= self.y && px < self.x + self.w && py < self.y + self.h
    }

    pub fn intersects(&self, o: &Rect) -> bool {
        self.x < o.x + o.w && o.x < self.x + self.w && self.y < o.y + o.h && o.y < self.y + self.h
    }
}

struct Atoms {
    net_active_window: u32,
    net_wm_window_type: u32,
    net_wm_window_type_notification: u32,
    net_wm_state: u32,
    net_wm_state_above: u32,
    net_wm_name: u32,
    utf8_string: u32,
}

pub struct X11 {
    pub conn: RustConnection,
    root: Window,
    screen_rect: Rect,
    depth: u8,
    visual: u32,
    colormap: u32,
    pub compositing: bool,
    atoms: Atoms,
    max_request_bytes: usize,
}

/// What we know about the game window.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct GameWindow {
    pub id: Window,
    pub rect: Rect,
    /// Mapped and at least partly on screen (dwm parks windows of hidden
    /// tags off screen instead of unmapping them).
    pub visible: bool,
}

impl X11 {
    pub fn connect() -> Result<X11> {
        let (conn, screen_num) = x11rb::connect(None).context("cannot connect to the X server ($DISPLAY)")?;
        let screen = conn.setup().roots[screen_num].clone();
        let intern = |name: &str| -> Result<u32> { Ok(conn.intern_atom(false, name.as_bytes())?.reply()?.atom) };
        let atoms = Atoms {
            net_active_window: intern("_NET_ACTIVE_WINDOW")?,
            net_wm_window_type: intern("_NET_WM_WINDOW_TYPE")?,
            net_wm_window_type_notification: intern("_NET_WM_WINDOW_TYPE_NOTIFICATION")?,
            net_wm_state: intern("_NET_WM_STATE")?,
            net_wm_state_above: intern("_NET_WM_STATE_ABOVE")?,
            net_wm_name: intern("_NET_WM_NAME")?,
            utf8_string: intern("UTF8_STRING")?,
        };
        let cm_owner = conn
            .get_selection_owner(intern(&format!("_NET_WM_CM_S{screen_num}"))?)?
            .reply()?
            .owner;
        let compositing = cm_owner != x11rb::NONE || compositor_process_running();

        let argb = screen
            .allowed_depths
            .iter()
            .filter(|d| d.depth == 32)
            .flat_map(|d| d.visuals.iter())
            .find(|v| v.class == VisualClass::TRUE_COLOR)
            .map(|v| v.visual_id);
        let (depth, visual, colormap) = match argb {
            Some(visual) => {
                let cmap = conn.generate_id()?;
                conn.create_colormap(ColormapAlloc::NONE, cmap, screen.root, visual)?;
                (32, visual, cmap)
            }
            None => (screen.root_depth, screen.root_visual, screen.default_colormap),
        };
        if conn.extension_information(shape::X11_EXTENSION_NAME)?.is_none() {
            anyhow::bail!("the X server lacks the SHAPE extension (needed for click-through panels)");
        }
        let max_request_bytes = conn.maximum_request_bytes();
        Ok(X11 {
            screen_rect: Rect::new(0, 0, screen.width_in_pixels as i32, screen.height_in_pixels as i32),
            root: screen.root,
            depth,
            visual,
            colormap,
            compositing: compositing && depth == 32,
            atoms,
            max_request_bytes,
            conn,
        })
    }

    pub fn flush(&self) {
        let _ = self.conn.flush();
    }

    pub fn poll_event(&self) -> Option<Event> {
        self.conn.poll_for_event().ok().flatten()
    }

    /// Watch the root window so restacking by the window manager is noticed.
    pub fn watch_root(&self) -> Result<()> {
        self.conn.change_window_attributes(
            self.root,
            &x11rb::protocol::xproto::ChangeWindowAttributesAux::new().event_mask(EventMask::SUBSTRUCTURE_NOTIFY),
        )?;
        Ok(())
    }

    fn wm_class(&self, win: Window) -> Option<String> {
        let reply = self
            .conn
            .get_property(false, win, AtomEnum::WM_CLASS, AtomEnum::STRING, 0, 256)
            .ok()?
            .reply()
            .ok()?;
        Some(String::from_utf8_lossy(&reply.value).replace('\0', " ").to_lowercase())
    }

    fn wm_name(&self, win: Window) -> Option<String> {
        let utf8 = self
            .conn
            .get_property(false, win, self.atoms.net_wm_name, self.atoms.utf8_string, 0, 256)
            .ok()?
            .reply()
            .ok()?;
        if !utf8.value.is_empty() {
            return Some(String::from_utf8_lossy(&utf8.value).into_owned());
        }
        let legacy = self
            .conn
            .get_property(false, win, AtomEnum::WM_NAME, AtomEnum::STRING, 0, 256)
            .ok()?
            .reply()
            .ok()?;
        Some(String::from_utf8_lossy(&legacy.value).into_owned())
    }

    fn is_hearthstone(&self, win: Window) -> bool {
        if self.wm_class(win).is_some_and(|c| c.split_whitespace().any(|p| p == "hearthstone.exe")) {
            return true;
        }
        self.wm_name(win).is_some_and(|n| n == "Hearthstone")
    }

    /// Searches top-level windows (and one level of WM frames) for the game.
    pub fn find_game_window(&self) -> Option<Window> {
        let top = self.conn.query_tree(self.root).ok()?.reply().ok()?.children;
        for &w in top.iter().rev() {
            if self.is_hearthstone(w) {
                return Some(w);
            }
        }
        for &w in top.iter().rev() {
            let Ok(Ok(tree)) = self.conn.query_tree(w).map(|c| c.reply()) else { continue };
            if let Some(&c) = tree.children.iter().find(|&&c| self.is_hearthstone(c)) {
                return Some(c);
            }
        }
        None
    }

    pub fn game_window(&self, id: Window) -> Option<GameWindow> {
        let attrs = self.conn.get_window_attributes(id).ok()?.reply().ok()?;
        let geo = self.conn.get_geometry(id).ok()?.reply().ok()?;
        let pos = self.conn.translate_coordinates(id, self.root, 0, 0).ok()?.reply().ok()?;
        let rect = Rect::new(pos.dst_x as i32, pos.dst_y as i32, geo.width as i32, geo.height as i32);
        let visible = attrs.map_state == MapState::VIEWABLE && rect.intersects(&self.screen_rect) && rect.w > 200;
        Some(GameWindow { id, rect, visible })
    }

    pub fn active_window(&self) -> Option<Window> {
        let reply = self
            .conn
            .get_property(false, self.root, self.atoms.net_active_window, AtomEnum::WINDOW, 0, 1)
            .ok()?
            .reply()
            .ok()?;
        let active = reply.value32()?.next();
        active
    }

    pub fn pointer(&self) -> Option<(i32, i32)> {
        let p = self.conn.query_pointer(self.root).ok()?.reply().ok()?;
        Some((p.root_x as i32, p.root_y as i32))
    }

    pub fn create_overlay(&self, name: &str, interactive: bool) -> Result<OverlayWindow> {
        let win = self.conn.generate_id()?;
        let mut events = EventMask::EXPOSURE;
        if interactive {
            events |= EventMask::BUTTON_PRESS | EventMask::POINTER_MOTION | EventMask::LEAVE_WINDOW;
        }
        let aux = CreateWindowAux::new()
            .background_pixel(0)
            .border_pixel(0)
            .colormap(self.colormap)
            .override_redirect(1)
            .event_mask(events);
        self.conn.create_window(
            self.depth,
            win,
            self.root,
            0,
            0,
            1,
            1,
            0,
            WindowClass::INPUT_OUTPUT,
            self.visual,
            &aux,
        )?;
        let class = format!("{name}\0hsbg-overlay\0");
        self.conn.change_property8(PropMode::REPLACE, win, AtomEnum::WM_CLASS, AtomEnum::STRING, class.as_bytes())?;
        self.conn.change_property8(PropMode::REPLACE, win, AtomEnum::WM_NAME, AtomEnum::STRING, name.as_bytes())?;
        self.conn.change_property32(
            PropMode::REPLACE,
            win,
            self.atoms.net_wm_window_type,
            AtomEnum::ATOM,
            &[self.atoms.net_wm_window_type_notification],
        )?;
        self.conn.change_property32(
            PropMode::REPLACE,
            win,
            self.atoms.net_wm_state,
            AtomEnum::ATOM,
            &[self.atoms.net_wm_state_above],
        )?;
        if !interactive {
            // Empty input region: clicks fall through to the game.
            self.conn.shape_rectangles(shape::SO::SET, shape::SK::INPUT, ClipOrdering::UNSORTED, win, 0, 0, &[])?;
        }
        let gc = self.conn.generate_id()?;
        self.conn.create_gc(gc, win, &CreateGCAux::new().graphics_exposures(0))?;
        Ok(OverlayWindow { id: win, gc, rect: Rect::default(), mapped: false, frame: Vec::new() })
    }
}

fn compositor_process_running() -> bool {
    const NAMES: [&str; 5] = ["picom", "xcompmgr", "compton", "compiz", "fastcompmgr"];
    std::fs::read_dir("/proc")
        .map(|it| {
            it.flatten().any(|e| {
                std::fs::read_to_string(e.path().join("comm")).is_ok_and(|c| NAMES.contains(&c.trim()))
            })
        })
        .unwrap_or(false)
}

/// One overlay panel window. Keeps its last frame so exposures can be
/// repainted without re-rendering.
pub struct OverlayWindow {
    pub id: Window,
    gc: u32,
    pub rect: Rect,
    pub mapped: bool,
    /// BGRA, premultiplied, `rect.w * rect.h * 4` bytes.
    frame: Vec<u8>,
}

impl OverlayWindow {
    pub fn set_rect(&mut self, x: &X11, rect: Rect) {
        if rect == self.rect || rect.w <= 0 || rect.h <= 0 {
            return;
        }
        let aux = ConfigureWindowAux::new().x(rect.x).y(rect.y).width(rect.w as u32).height(rect.h as u32);
        let _ = x.conn.configure_window(self.id, &aux);
        self.rect = rect;
    }

    pub fn show(&mut self, x: &X11) {
        if !self.mapped {
            let _ = x.conn.map_window(self.id);
            self.mapped = true;
        }
    }

    pub fn hide(&mut self, x: &X11) {
        if self.mapped {
            let _ = x.conn.unmap_window(self.id);
            self.mapped = false;
        }
    }

    pub fn raise(&self, x: &X11) {
        if self.mapped {
            let _ = x.conn.configure_window(self.id, &ConfigureWindowAux::new().stack_mode(StackMode::ABOVE));
        }
    }

    /// Uploads an RGBA premultiplied frame of the window's current size.
    pub fn present(&mut self, x: &X11, rgba: &[u8]) {
        let (w, h) = (self.rect.w as usize, self.rect.h as usize);
        if rgba.len() != w * h * 4 {
            return;
        }
        self.frame.clear();
        self.frame.reserve(rgba.len());
        for px in rgba.as_chunks::<4>().0 {
            self.frame.extend_from_slice(&[px[2], px[1], px[0], px[3]]);
        }
        self.repaint(x);
    }

    pub fn repaint(&self, x: &X11) {
        let (w, h) = (self.rect.w as usize, self.rect.h as usize);
        if self.frame.len() != w * h * 4 || w == 0 {
            return;
        }
        let rows_per_chunk = ((x.max_request_bytes.saturating_sub(64)) / (w * 4)).max(1);
        for (i, chunk) in self.frame.chunks(rows_per_chunk * w * 4).enumerate() {
            let rows = chunk.len() / (w * 4);
            let _ = x.conn.put_image(
                ImageFormat::Z_PIXMAP,
                self.id,
                self.gc,
                w as u16,
                rows as u16,
                0,
                (i * rows_per_chunk) as i16,
                0,
                x.depth,
                chunk,
            );
        }
    }
}
