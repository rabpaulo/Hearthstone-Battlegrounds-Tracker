//! Client for the simulator process (`sidecar/sim.mjs`).

use std::io::{BufRead, BufReader, Write};
use std::path::{Path, PathBuf};
use std::process::{Child, ChildStdin, Command, Stdio};
use std::sync::mpsc::{channel, Receiver};
use std::time::{Duration, Instant};

use serde::Deserialize;

use crate::config::Config;
use crate::paths;

#[derive(Debug, Clone, Copy, PartialEq, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Odds {
    pub won: f32,
    pub tied: f32,
    pub lost: f32,
    pub won_lethal: f32,
    pub lost_lethal: f32,
    pub average_damage_won: f32,
    pub average_damage_lost: f32,
    pub simulations: u32,
}

#[derive(Debug, Deserialize)]
struct Message {
    #[serde(rename = "type")]
    kind: String,
    #[serde(default)]
    id: u64,
    result: Option<Odds>,
    message: Option<String>,
}

#[derive(Debug, Clone, PartialEq)]
pub enum SimEvent {
    Partial(Odds),
    Done(Odds),
    Failed(String),
}

enum Incoming {
    Line(String),
    Exited,
}

pub struct SimClient {
    node: String,
    dir: Option<PathBuf>,
    cards: PathBuf,
    sims: u32,
    child: Option<Child>,
    stdin: Option<ChildStdin>,
    rx: Option<Receiver<Incoming>>,
    ready: bool,
    queued: Option<String>,
    next_id: u64,
    current: u64,
    last_spawn: Option<Instant>,
    unavailable: Option<String>,
}

/// Folder with `sim.mjs` and the installed simulator package.
pub fn find_sidecar(cfg: &Config) -> Option<PathBuf> {
    let exe_dir = std::env::current_exe().ok().and_then(|p| p.parent().map(Path::to_path_buf));
    let mut candidates: Vec<PathBuf> = Vec::new();
    candidates.extend(cfg.sidecar_dir.clone());
    candidates.extend(std::env::var_os("HSBG_SIDECAR_DIR").map(PathBuf::from));
    if let Some(d) = &exe_dir {
        candidates.push(d.join("sidecar"));
        candidates.push(d.join("../share/hsbg-overlay/sidecar"));
        candidates.push(d.join("../../sidecar"));
    }
    candidates.push(paths::data_dir().join("sidecar"));
    candidates.push(PathBuf::from(concat!(env!("CARGO_MANIFEST_DIR"), "/sidecar")));
    candidates.into_iter().find(|d| d.join("sim.mjs").is_file())
}

impl SimClient {
    pub fn new(cfg: &Config, cards: &Path) -> SimClient {
        let dir = find_sidecar(cfg);
        let unavailable = match &dir {
            None => Some("simulator not found (sidecar/sim.mjs)".to_string()),
            Some(d) if !d.join("node_modules/@firestone-hs/simulate-bgs-battle").is_dir() => {
                Some(format!("run `npm install` in {}", d.display()))
            }
            _ => None,
        };
        if let Some(why) = &unavailable {
            log::warn!("combat simulation disabled: {why}");
        }
        SimClient {
            node: cfg.node.clone(),
            dir,
            cards: cards.to_path_buf(),
            sims: cfg.simulations.max(100),
            child: None,
            stdin: None,
            rx: None,
            ready: false,
            queued: None,
            next_id: 0,
            current: 0,
            last_spawn: None,
            unavailable,
        }
    }

    /// Stops the process (it holds a few hundred MB of card data).
    pub fn stop(&mut self) {
        if let Some(mut c) = self.child.take() {
            log::info!("simulator stopped");
            let _ = c.kill();
            let _ = c.wait();
        }
        self.stdin = None;
        self.rx = None;
        self.ready = false;
        self.queued = None;
    }

    pub fn running(&self) -> bool {
        self.child.is_some()
    }

    pub fn unavailable(&self) -> Option<&str> {
        self.unavailable.as_deref()
    }

    /// Starts the process ahead of time (loading cards takes ~1.5 s).
    pub fn warm_up(&mut self) {
        if self.child.is_none() && self.unavailable.is_none() {
            if self.last_spawn.is_some_and(|t| t.elapsed() < Duration::from_secs(5)) {
                return;
            }
            self.spawn();
        }
    }

    fn spawn(&mut self) {
        let Some(dir) = self.dir.clone() else { return };
        self.last_spawn = Some(Instant::now());
        let child = Command::new(&self.node)
            .arg(dir.join("sim.mjs"))
            .arg(&self.cards)
            .current_dir(&dir)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn();
        let mut child = match child {
            Ok(c) => c,
            Err(e) => {
                self.unavailable = Some(format!("cannot run `{}`: {e}", self.node));
                log::warn!("combat simulation disabled: {e}");
                return;
            }
        };
        let (tx, rx) = channel();
        let stdout = child.stdout.take().expect("piped");
        std::thread::spawn(move || {
            for line in BufReader::new(stdout).lines() {
                let Ok(line) = line else { break };
                if tx.send(Incoming::Line(line)).is_err() {
                    return;
                }
            }
            let _ = tx.send(Incoming::Exited);
        });
        let stderr = child.stderr.take().expect("piped");
        std::thread::spawn(move || {
            for line in BufReader::new(stderr).lines().map_while(Result::ok) {
                log::debug!("sim: {line}");
            }
        });
        self.stdin = child.stdin.take();
        self.child = Some(child);
        self.rx = Some(rx);
        self.ready = false;
        log::info!("simulator starting");
    }

    /// Simulates a combat; supersedes any simulation in flight.
    pub fn request(&mut self, snapshot: serde_json::Value) {
        if self.unavailable.is_some() {
            return;
        }
        self.next_id += 1;
        self.current = self.next_id;
        let line = serde_json::json!({
            "id": self.current,
            "type": "simulate",
            "sims": self.sims,
            "snapshot": snapshot,
        })
        .to_string();
        self.queued = Some(line);
        self.warm_up();
        self.flush_queue();
    }

    fn flush_queue(&mut self) {
        if !self.ready {
            return;
        }
        if let (Some(line), Some(stdin)) = (self.queued.take(), self.stdin.as_mut()) {
            if writeln!(stdin, "{line}").and_then(|_| stdin.flush()).is_err() {
                log::warn!("simulator pipe closed");
            }
        }
    }

    pub fn poll(&mut self) -> Vec<SimEvent> {
        let mut events = Vec::new();
        let mut exited = false;
        if let Some(rx) = &self.rx {
            while let Ok(msg) = rx.try_recv() {
                let line = match msg {
                    Incoming::Line(l) => l,
                    Incoming::Exited => {
                        exited = true;
                        break;
                    }
                };
                let Ok(m) = serde_json::from_str::<Message>(&line) else { continue };
                match m.kind.as_str() {
                    "ready" => {
                        self.ready = true;
                        log::info!("simulator ready");
                    }
                    _ if m.id != self.current => {}
                    "partial" => events.extend(m.result.map(SimEvent::Partial)),
                    "result" => events.extend(m.result.map(SimEvent::Done)),
                    "error" => events.push(SimEvent::Failed(m.message.unwrap_or_default())),
                    _ => {}
                }
            }
        }
        if exited {
            log::warn!("simulator exited");
            if let Some(mut c) = self.child.take() {
                let _ = c.wait();
            }
            self.stdin = None;
            self.rx = None;
            self.ready = false;
            if self.queued.is_none() && self.current != 0 {
                events.push(SimEvent::Failed("simulator stopped".into()));
            }
        }
        if self.ready {
            self.flush_queue();
        } else if self.queued.is_some() && self.child.is_none() {
            self.warm_up();
        }
        events
    }
}

impl Drop for SimClient {
    fn drop(&mut self) {
        self.stop();
    }
}
