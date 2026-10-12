//! Card images from HearthstoneJSON, fetched and decoded off the UI thread
//! and cached on disk.
//!
//! - tiles: 256×59 strips used for list rows,
//! - renders: full Battlegrounds card images used for the hover preview.

use std::collections::{HashMap, HashSet};
use std::path::PathBuf;
use std::sync::mpsc::{channel, Receiver, Sender};
use std::sync::Arc;

use tiny_skia::Pixmap;

use crate::{net, paths};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum ImageKind {
    Tile,
    Render,
}

impl ImageKind {
    fn url(self, card_id: &str) -> String {
        match self {
            ImageKind::Tile => format!("https://art.hearthstonejson.com/v1/tiles/{card_id}.png"),
            ImageKind::Render => format!("https://art.hearthstonejson.com/v1/bgs/latest/enUS/256x/{card_id}.png"),
        }
    }

    fn dir(self) -> &'static str {
        match self {
            ImageKind::Tile => "tiles",
            ImageKind::Render => "renders",
        }
    }
}

type Key = (ImageKind, String);

pub struct Images {
    loaded: HashMap<Key, Arc<Pixmap>>,
    requested: HashSet<Key>,
    to_worker: Sender<Key>,
    from_worker: Receiver<(Key, Option<Pixmap>)>,
    revision: u64,
}

impl Images {
    pub fn new() -> Images {
        let (to_worker, jobs) = channel::<Key>();
        let (done, from_worker) = channel();
        let jobs = Arc::new(std::sync::Mutex::new(jobs));
        for _ in 0..3 {
            let jobs = jobs.clone();
            let done = done.clone();
            std::thread::spawn(move || loop {
                let job = jobs.lock().unwrap().recv();
                let Ok(key) = job else { return };
                let img = load(&key);
                if done.send((key, img)).is_err() {
                    return;
                }
            });
        }
        Images { loaded: HashMap::new(), requested: HashSet::new(), to_worker, from_worker, revision: 0 }
    }

    /// The image if ready; otherwise schedules it and returns `None`.
    pub fn get(&mut self, kind: ImageKind, card_id: &str) -> Option<Arc<Pixmap>> {
        if card_id.is_empty() {
            return None;
        }
        let key = (kind, card_id.to_string());
        if let Some(img) = self.loaded.get(&key) {
            return Some(img.clone());
        }
        if self.requested.insert(key.clone()) {
            let _ = self.to_worker.send(key);
        }
        None
    }

    /// Collects finished downloads. Returns true when something new arrived.
    pub fn poll(&mut self) -> bool {
        let mut changed = false;
        while let Ok((key, img)) = self.from_worker.try_recv() {
            if let Some(img) = img {
                self.loaded.insert(key, Arc::new(img));
                changed = true;
            }
        }
        if changed {
            self.revision += 1;
        }
        changed
    }

    /// Bumps whenever an image arrives; part of panel models so they redraw.
    pub fn revision(&self) -> u64 {
        self.revision
    }
}

fn cache_path(key: &Key) -> PathBuf {
    paths::cache_dir().join(key.0.dir()).join(format!("{}.png", key.1))
}

fn load(key: &Key) -> Option<Pixmap> {
    let path = cache_path(key);
    if !path.is_file() {
        if let Err(e) = net::download(&key.0.url(&key.1), &path) {
            log::debug!("image {}: {e:#}", key.1);
            return None;
        }
    }
    let img = decode_png(&std::fs::read(&path).ok()?)?;
    match key.0 {
        // Tiles reserve their left part for a deck tracker's mana gem.
        ImageKind::Tile if img.width() > TILE_ART_X * 2 => {
            img.clone_rect(tiny_skia::IntRect::from_xywh(TILE_ART_X as i32, 0, img.width() - TILE_ART_X, img.height())?)
        }
        _ => Some(img),
    }
}

/// First column of actual art in a 256-wide tile.
const TILE_ART_X: u32 = 80;

/// Decodes a PNG into a premultiplied RGBA pixmap.
fn decode_png(bytes: &[u8]) -> Option<Pixmap> {
    let mut decoder = png::Decoder::new(bytes);
    decoder.set_transformations(png::Transformations::EXPAND | png::Transformations::STRIP_16);
    let mut reader = decoder.read_info().ok()?;
    let mut buf = vec![0; reader.output_buffer_size()];
    let info = reader.next_frame(&mut buf).ok()?;
    let (w, h) = (info.width, info.height);
    let mut pixmap = Pixmap::new(w, h)?;
    let src = &buf[..info.buffer_size()];
    let dst = pixmap.data_mut();
    let channels = info.color_type.samples();
    for (i, px) in src.chunks_exact(channels).enumerate() {
        let (r, g, b, a) = match channels {
            4 => (px[0], px[1], px[2], px[3]),
            3 => (px[0], px[1], px[2], 255),
            2 => (px[0], px[0], px[0], px[1]),
            _ => (px[0], px[0], px[0], 255),
        };
        let pm = |c: u8| ((c as u32 * a as u32 + 127) / 255) as u8;
        dst[i * 4..i * 4 + 4].copy_from_slice(&[pm(r), pm(g), pm(b), a]);
    }
    Some(pixmap)
}
