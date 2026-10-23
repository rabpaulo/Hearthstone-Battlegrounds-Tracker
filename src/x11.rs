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
    EventMask, GrabMode, GrabStatus, ImageFormat, KeyButMask, MapState, PropMode, StackMode, VisualClass, Window,
    WindowClass,
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
    /// Marks a window for Xalia (gamepad UI helper) to ignore.
    xalia_ignore: u32,
}

/// A key press, as far as a text field cares.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Key {
    Char(char),
    Backspace,
    /// Ctrl+Backspace / Ctrl+W.
    DeleteWord,
    /// Ctrl+U.
    Clear,
    Enter,
    Escape,
    Up,
    Down,
    PageUp,
    PageDown,
    Tab,
    BackTab,
}

/// Translates a keysym; `None` for keys a text field ignores.
fn key_from_keysym(sym: u32, ctrl: bool) -> Option<Key> {
    Some(match sym {
        0xff08 if ctrl => Key::DeleteWord,
        0xff08 => Key::Backspace,
        0xff0d | 0xff8d => Key::Enter,
        0xff1b => Key::Escape,
        0xff52 => Key::Up,
        0xff54 => Key::Down,
        0xff55 => Key::PageUp,
        0xff56 => Key::PageDown,
        0xff09 => Key::Tab,
        0xfe20 => Key::BackTab,
        0x77 | 0x57 if ctrl => Key::DeleteWord,
        0x75 | 0x55 if ctrl => Key::Clear,
        _ if ctrl => return None,
        // Latin-1 keysyms are their code points; others are offset.
        0x20..=0x7e | 0xa0..=0xff => Key::Char(char::from_u32(sym)?),
        0x0100_00a0..=0x0110_ffff => Key::Char(char::from_u32(sym - 0x0100_0000)?),
        _ => return None,
    })
}

/// Lower- and upper-case forms of a Latin-1 letter keysym.
fn keysym_cases(sym: u32) -> (u32, u32) {
    match char::from_u32(sym) {
        Some(c) if sym <= 0xff && c.is_alphabetic() => {
            let lower = c.to_lowercase().next().unwrap_or(c) as u32;
            let upper = c.to_uppercase().next().filter(|u| (*u as u32) <= 0xff).unwrap_or(c) as u32;
            (lower, upper)
        }
        _ => (sym, sym),
    }
}

/// Core keyboard mapping: keysyms per keycode, as the server reports them.
#[derive(Default)]
struct Keymap {
    min_keycode: u8,
    per_keycode: usize,
    syms: Vec<u32>,
}

impl Keymap {
    /// Keysym for a key with the given modifiers (shift, caps lock and
    /// AltGr, which is Mod5 on common layouts).
    fn keysym(&self, keycode: u8, state: u16) -> u32 {
        let Some(start) = (keycode.checked_sub(self.min_keycode)).map(|k| k as usize * self.per_keycode) else {
            return 0;
        };
        let Some(syms) = self.syms.get(start..start + self.per_keycode) else { return 0 };
        let at = |i: usize| syms.get(i).copied().unwrap_or(0);
        let altgr = state & u16::from(KeyButMask::MOD5) != 0 && at(4) != 0;
        let (mut base, mut shifted) = if altgr { (at(4), at(5)) } else { (at(0), at(1)) };
        if shifted == 0 {
            (base, shifted) = keysym_cases(base);
        }
        let mut shift = state & u16::from(KeyButMask::SHIFT) != 0;
        if state & u16::from(KeyButMask::LOCK) != 0 && keysym_cases(base).0 != keysym_cases(base).1 {
            shift = !shift;
        }
        if shift { shifted } else { base }
    }
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
    keymap: Keymap,
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
            xalia_ignore: intern("__xalia_ignore")?,
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
        let mut x = X11 {
            screen_rect: Rect::new(0, 0, screen.width_in_pixels as i32, screen.height_in_pixels as i32),
            root: screen.root,
            depth,
            visual,
            colormap,
            compositing: compositing && depth == 32,
            atoms,
            max_request_bytes,
            keymap: Keymap::default(),
            conn,
        };
        x.reload_keymap();
        Ok(x)
    }

    /// Reads the keyboard mapping; call again on `MappingNotify`.
    pub fn reload_keymap(&mut self) {
        let setup = self.conn.setup();
        let (min, max) = (setup.min_keycode, setup.max_keycode);
        let reply = self.conn.get_keyboard_mapping(min, max - min + 1).ok().and_then(|c| c.reply().ok());
        match reply {
            Some(r) => {
                self.keymap = Keymap { min_keycode: min, per_keycode: r.keysyms_per_keycode as usize, syms: r.keysyms };
            }
            None => log::warn!("could not read the keyboard mapping"),
        }
    }

    pub fn key(&self, keycode: u8, state: KeyButMask) -> Option<Key> {
        let state = u16::from(state);
        let sym = self.keymap.keysym(keycode, state);
        key_from_keysym(sym, state & u16::from(KeyButMask::CONTROL) != 0)
    }

    /// Sends all key presses to `win` (overlay windows never get the input
    /// focus). False when another client holds the keyboard.
    pub fn grab_keyboard(&self, win: Window) -> bool {
        let reply = self
            .conn
            .grab_keyboard(false, win, x11rb::CURRENT_TIME, GrabMode::ASYNC, GrabMode::ASYNC)
            .ok()
            .and_then(|c| c.reply().ok());
        reply.is_some_and(|r| r.status == GrabStatus::SUCCESS)
    }

    pub fn ungrab_keyboard(&self) {
        let _ = self.conn.ungrab_keyboard(x11rb::CURRENT_TIME);
    }

    pub fn flush(&self) {
        let _ = self.conn.flush();
    }

    /// Places `win` directly below `sibling` in the stacking order.
    pub fn restack_below(&self, win: Window, sibling: Window) {
        let _ = self
            .conn
            .configure_window(win, &ConfigureWindowAux::new().sibling(sibling).stack_mode(StackMode::BELOW));
    }

    /// Demotes every foreign, mapped top-level window that overlaps `rects`
    /// to just below `bottom`. Wine/Proton (and tools from Steam or GE-Proton
    /// such as Xalia) restack fullscreen helper windows above the panels at
    /// arbitrary moments; pinning only the game window left a 1920x1080 ghost
    /// window floated above the panels inside the game prefix, swallowing
    /// every panel click. The dwm bar is left alone.
    pub fn demote_above(&self, rects: &[Rect], ours: &[Window], bottom: Window) {
        let reply = self.conn.query_tree(self.root).ok().and_then(|c| c.reply().ok());
        let Some(reply) = reply else { return };
        self.demote_children(&reply.children, rects, ours, bottom);
    }

    /// Children are listed bottom of the stack first; everything after
    /// `bottom` covers it. Only mapped, foreign windows overlapping a rect
    /// are sent back below it.
    fn demote_children(&self, children: &[Window], rects: &[Rect], ours: &[Window], bottom: Window) {
        let Some(bottom_pos) = children.iter().position(|w| *w == bottom) else {
            return;
        };
        for win in &children[bottom_pos + 1..] {
            if ours.contains(win) {
                continue;
            }
            let attrs = self.conn.get_window_attributes(*win).ok().and_then(|c| c.reply().ok());
            let Some(attrs) = attrs else {
                continue;
            };
            if attrs.map_state != MapState::VIEWABLE {
                continue;
            }
            if self.wm_class(*win).is_some_and(|c| c.split_whitespace().any(|p| p == "dwm")) {
                continue;
            }
            let geo = self.conn.get_geometry(*win).ok().and_then(|c| c.reply().ok());
            let pos = self.conn.translate_coordinates(*win, self.root, 0, 0).ok().and_then(|c| c.reply().ok());
            let (Some(geo), Some(pos)) = (geo, pos) else {
                continue;
            };
            let r = Rect::new(pos.dst_x as i32, pos.dst_y as i32, geo.width as i32, geo.height as i32);
            if rects.iter().any(|p| p.intersects(&r)) {
                self.restack_below(*win, bottom);
            }
        }
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

    pub fn is_hearthstone(&self, win: Window) -> bool {
        if self.wm_class(win).is_some_and(|c| c.split_whitespace().any(|p| p == "hearthstone.exe")) {
            return true;
        }
        self.wm_name(win).is_some_and(|n| n == "Hearthstone")
    }

    /// Climbs the window tree from `win` until reaching a direct child of `root`.
    pub fn top_level_window(&self, mut win: Window) -> Option<Window> {
        if win == 0 || win == 1 || win == self.root {
            return None;
        }
        loop {
            let tree = self.conn.query_tree(win).ok()?.reply().ok()?;
            if tree.parent == self.root || tree.parent == 0 {
                return Some(win);
            }
            win = tree.parent;
        }
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
        // First try X11 input focus: works reliably even when tiling WMs like dwm
        // do not update _NET_ACTIVE_WINDOW for Wine/Proton windows (due to
        // neverfocus / WM_TAKE_FOCUS).
        if let Some(reply) = self.conn.get_input_focus().ok().and_then(|c| c.reply().ok()) {
            let focus = reply.focus;
            if focus == 1 {
                // PointerRoot: focus is with whatever window contains the pointer.
                if let Some(p) = self.conn.query_pointer(self.root).ok().and_then(|c| c.reply().ok()) {
                    if p.child != 0 && p.child != self.root {
                        if let Some(top) = self.top_level_window(p.child) {
                            return Some(top);
                        }
                    }
                }
            } else if focus != 0 && focus != self.root {
                if let Some(top) = self.top_level_window(focus) {
                    return Some(top);
                }
                return Some(focus);
            }
        }

        let reply = self
            .conn
            .get_property(false, self.root, self.atoms.net_active_window, AtomEnum::WINDOW, 0, 1)
            .ok()?
            .reply()
            .ok()?;
        let active = reply.value32()?.next();
        active
    }

    /// Pointer position, and whether a mouse button is held.
    pub fn pointer(&self) -> Option<(i32, i32, bool)> {
        let p = self.conn.query_pointer(self.root).ok()?.reply().ok()?;
        let buttons = KeyButMask::BUTTON1 | KeyButMask::BUTTON2 | KeyButMask::BUTTON3;
        Some((p.root_x as i32, p.root_y as i32, u16::from(p.mask) & u16::from(buttons) != 0))
    }

    pub fn create_overlay(&self, name: &str, interactive: bool) -> Result<OverlayWindow> {
        let win = self.conn.generate_id()?;
        let mut events = EventMask::EXPOSURE;
        if interactive {
            events |= EventMask::BUTTON_PRESS | EventMask::POINTER_MOTION | EventMask::LEAVE_WINDOW | EventMask::KEY_PRESS;
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
        // Xalia (the gamepad UI helper GE-Proton attaches every Wine process)
        // scans the active window for controls and can grab clicks; it skips
        // any window with this property (see xalia's __xalia_ignore).
        self.conn
            .change_property8(PropMode::REPLACE, win, self.atoms.xalia_ignore, AtomEnum::STRING, b"1")?;
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn keysyms_follow_shift_caps_lock_and_altgr() {
        // Keycode 10: a/A; 11: 1/!, AltGr ¹; 12: Return.
        let km = Keymap {
            min_keycode: 10,
            per_keycode: 6,
            syms: vec![0x61, 0, 0, 0, 0, 0, 0x31, 0x21, 0, 0, 0xb9, 0, 0xff0d, 0, 0, 0, 0, 0],
        };
        let (shift, lock, altgr) = (u16::from(KeyButMask::SHIFT), u16::from(KeyButMask::LOCK), u16::from(KeyButMask::MOD5));
        assert_eq!(km.keysym(10, 0), 0x61);
        assert_eq!(km.keysym(10, shift), 0x41);
        assert_eq!(km.keysym(10, lock), 0x41);
        assert_eq!(km.keysym(11, lock), 0x31, "caps lock leaves digits alone");
        assert_eq!(km.keysym(11, shift), 0x21);
        assert_eq!(km.keysym(11, altgr), 0xb9);
        assert_eq!(km.keysym(9, 0), 0, "below the first keycode");
        assert_eq!(key_from_keysym(km.keysym(12, 0), false), Some(Key::Enter));
    }

    #[test]
    fn control_keys_edit_and_others_are_ignored() {
        assert_eq!(key_from_keysym(0x61, false), Some(Key::Char('a')));
        assert_eq!(key_from_keysym(0xe9, false), Some(Key::Char('é')));
        assert_eq!(key_from_keysym(0x75, true), Some(Key::Clear));
        assert_eq!(key_from_keysym(0xff08, true), Some(Key::DeleteWord));
        assert_eq!(key_from_keysym(0x61, true), None);
        assert_eq!(key_from_keysym(0xffe1, false), None, "Shift alone");
    }
}
