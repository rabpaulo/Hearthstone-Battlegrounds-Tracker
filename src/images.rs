//! Card images from HearthstoneJSON, fetched and decoded off the UI thread
//! and cached on disk.
//!
//! - tiles: 256×59 strips used for list rows,
//! - renders: full Battlegrounds card images used for the hover preview.

use std::collections::{HashMap, HashSet, VecDeque};
use std::path::PathBuf;
use std::sync::mpsc::{channel, Receiver, Sender};
use std::sync::{Arc, Condvar, Mutex};

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

enum Job {
    /// Download if needed, decode and hand back to the UI thread.
    Load(Key),
    /// Only make sure the file is in the disk cache.
    Fetch(Key),
}

/// Work shared by the worker threads. Loads go to the front so whatever the
/// UI is waiting on overtakes background prefetches.
#[derive(Default)]
struct Queue {
    jobs: Mutex<VecDeque<Job>>,
    ready: Condvar,
}

impl Queue {
    fn push(&self, job: Job) {
        let mut jobs = self.jobs.lock().unwrap();
        match job {
            Job::Load(_) => jobs.push_front(job),
            Job::Fetch(_) => jobs.push_back(job),
        }
        self.ready.notify_one();
    }

    fn pop(&self) -> Job {
        let mut jobs = self.jobs.lock().unwrap();
        loop {
            if let Some(job) = jobs.pop_front() {
                return job;
            }
            jobs = self.ready.wait(jobs).unwrap();
        }
    }
}

pub struct Images {
    loaded: HashMap<Key, Arc<Pixmap>>,
    requested: HashSet<Key>,
    prefetched: HashSet<Key>,
    queue: Arc<Queue>,
    from_worker: Receiver<(Key, Option<Pixmap>)>,
    revision: u64,
}

impl Images {
    pub fn new() -> Images {
        let queue = Arc::new(Queue::default());
        let (done, from_worker): (Sender<_>, _) = channel();
        for _ in 0..4 {
            let queue = queue.clone();
            let done = done.clone();
            std::thread::spawn(move || loop {
                match queue.pop() {
                    Job::Load(key) => {
                        let img = load(&key);
                        if done.send((key, img)).is_err() {
                            return;
                        }
                    }
                    Job::Fetch(key) => {
                        fetch(&key);
                    }
                }
            });
        }
        Images {
            loaded: HashMap::new(),
            requested: HashSet::new(),
            prefetched: HashSet::new(),
            queue,
            from_worker,
            revision: 0,
        }
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
            self.queue.push(Job::Load(key));
        }
        None
    }

    /// Downloads an image into the disk cache in the background, so a later
    /// `get` only has to decode it.
    pub fn prefetch(&mut self, kind: ImageKind, card_id: &str) {
        if card_id.is_empty() {
            return;
        }
        let key = (kind, card_id.to_string());
        if self.requested.contains(&key) || self.prefetched.contains(&key) {
            return;
        }
        self.prefetched.insert(key.clone());
        self.queue.push(Job::Fetch(key));
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

/// Path of the cached file, downloading it first if needed.
fn fetch(key: &Key) -> Option<PathBuf> {
    let path = cache_path(key);
    if !path.is_file() {
        if let Err(e) = net::download(&key.0.url(&key.1), &path) {
            log::debug!("image {}: {e:#}", key.1);
            return None;
        }
    }
    Some(path)
}

fn load(key: &Key) -> Option<Pixmap> {
    let path = fetch(key)?;
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
