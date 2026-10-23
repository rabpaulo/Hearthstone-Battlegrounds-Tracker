mod app;
mod cards;
mod comps;
mod config;
mod enablers;
mod game;
mod history;
mod images;
mod logfile;
mod memory;
mod net;
mod paths;
mod search;
mod sim;
mod ui;
mod x11;

use std::path::PathBuf;
use std::time::{Duration, Instant};

use anyhow::{bail, Context, Result};

use crate::app::{App, Overlay};
use crate::cards::CardDb;
use crate::config::Config;
use crate::game::{Game, GameEvent};
use crate::history::History;
use crate::sim::{SimClient, SimEvent};

const USAGE: &str = "\
hsbg-overlay — Hearthstone Battlegrounds overlay for X11

USAGE:
    hsbg-overlay [--calibrate] [--logs <dir>]
    hsbg-overlay --replay <Power.log> [--no-sim]
    hsbg-overlay --render-ui <Power.log> <out-dir> [combat-number]
    hsbg-overlay --lobby-tribes

OPTIONS:
    --calibrate    outline the leaderboard hover areas (to tune [leaderboard] in the config)
    --logs <dir>   follow this Logs folder instead of the detected Hearthstone install
    --replay       print every combat of a log with simulated odds and the real outcome
    --render-ui    render the panels to PNG files from a log (no game needed)
    --lobby-tribes print the current game's tribes read from Hearthstone's memory
    -h, --help     show this help

Config: ~/.config/hsbg-overlay/config.toml   Log level: RUST_LOG=debug
";

fn main() {
    env_logger::Builder::from_env(env_logger::Env::default().default_filter_or("info")).init();
    if let Err(e) = run() {
        eprintln!("error: {e:#}");
        std::process::exit(1);
    }
}

fn run() -> Result<()> {
    let args: Vec<String> = std::env::args().skip(1).collect();
    if args.iter().any(|a| a == "-h" || a == "--help") {
        print!("{USAGE}");
        return Ok(());
    }
    let cfg = Config::load(&paths::config_path())?;
    match args.first().map(String::as_str) {
        Some("--replay") => {
            let log = args.get(1).context("--replay needs a Power.log path")?;
            replay(cfg, &PathBuf::from(log), !args.iter().any(|a| a == "--no-sim"))
        }
        Some("--lobby-tribes") => {
            let pid = memory::hearthstone_pid().context("Hearthstone is not running")?;
            match memory::read_once(pid)? {
                Some(l) => println!("{}", l.tribes.iter().map(|t| t.label()).collect::<Vec<_>>().join(", ")),
                None => println!("not in a Battlegrounds game"),
            }
            Ok(())
        }
        Some("--render-ui") => {
            let (Some(log), Some(out)) = (args.get(1), args.get(2)) else {
                bail!("--render-ui needs <Power.log> <out-dir>");
            };
            let combat = args.get(3).map(|n| n.parse()).transpose()?.unwrap_or(usize::MAX);
            let db = CardDb::load_or_fetch()?;
            let ov = Overlay::new(cfg, db, History::in_memory(Vec::new()));
            app::render_offline(ov, &PathBuf::from(log), &PathBuf::from(out), combat)
        }
        _ => {
            let mut calibrate = false;
            let mut logs = None;
            let mut it = args.iter();
            while let Some(a) = it.next() {
                match a.as_str() {
                    "--calibrate" => calibrate = true,
                    "--logs" => logs = Some(PathBuf::from(it.next().context("--logs needs a folder")?)),
                    other => bail!("unknown argument {other}\n\n{USAGE}"),
                }
            }
            overlay(cfg, calibrate, logs)
        }
    }
}

fn overlay(cfg: Config, calibrate: bool, logs: Option<PathBuf>) -> Result<()> {
    let logs = match logs {
        Some(dir) => dir,
        None => {
            let hs_dir = paths::find_hearthstone_dir(cfg.hearthstone_dir.as_deref())?;
            log::info!("Hearthstone: {}", hs_dir.display());
            match paths::ensure_log_config(&hs_dir) {
                Ok(Some(path)) => log::warn!("enabled the Power log in {} — restart Hearthstone", path.display()),
                Ok(None) => {}
                Err(e) => log::warn!("could not check log.config: {e:#}"),
            }
            match paths::ensure_client_config(&hs_dir) {
                Ok(Some(path)) => log::warn!("disabled log truncation in {} — restart Hearthstone", path.display()),
                Ok(None) => {}
                Err(e) => log::warn!("could not check client.config: {e:#}"),
            }
            hs_dir.join("Logs")
        }
    };
    let db = CardDb::load_or_fetch()?;
    let sim = SimClient::new(&cfg, db.path());
    let ov = Overlay::new(cfg, db, History::load());
    let mut app = App::new(ov, &logs, sim, calibrate)?;
    app.run()
}

/// Prints each combat of a log: simulated odds next to what really happened.
fn replay(cfg: Config, log: &std::path::Path, simulate: bool) -> Result<()> {
    let db = CardDb::load_or_fetch()?;
    let mut sim = SimClient::new(&cfg, db.path());
    if simulate {
        if let Some(why) = sim.unavailable() {
            bail!("simulator unavailable: {why}");
        }
    }
    let mut ov = Overlay::new(cfg, db, History::in_memory(Vec::new()));
    let text = std::fs::read_to_string(log).with_context(|| format!("reading {}", log.display()))?;
    ov.game.set_source("replay");
    // Health (+armor) of both sides when the last combat started.
    let mut pending: Option<(i32, i32, i32, i32)> = None;
    let hp = |g: &Game, lobby: i32| {
        g.leaderboard().iter().find(|p| p.lobby_id == lobby).map(|p| p.health + p.armor).unwrap_or(0)
    };
    for line in text.lines() {
        ov.game.process(line);
        for ev in ov.game.take_events() {
            match ev {
                GameEvent::GameStarted => println!("── new game"),
                GameEvent::Combat(snap) => {
                    let opp = ov.db.name(snap.opponent.hero.as_ref().map(|h| h.card_id.as_str()).unwrap_or(""));
                    print!("turn {:>2} vs {:<28}", snap.turn, opp);
                    if simulate {
                        sim.request(ov.sim_input(&snap));
                        let deadline = Instant::now() + Duration::from_secs(30);
                        let result = loop {
                            if let Some(ev) = sim.poll().into_iter().find(|e| !matches!(e, SimEvent::Partial(_))) {
                                break Some(ev);
                            }
                            if Instant::now() > deadline {
                                break None;
                            }
                            std::thread::sleep(Duration::from_millis(20));
                        };
                        match result {
                            Some(SimEvent::Done(o)) => print!(
                                " win {:>5.1}%  tie {:>5.1}%  loss {:>5.1}%  (lethal {:.1}% / {:.1}%)",
                                o.won, o.tied, o.lost, o.won_lethal, o.lost_lethal
                            ),
                            Some(SimEvent::Failed(m)) => print!(" simulation failed: {m}"),
                            _ => print!(" simulation timed out"),
                        }
                    }
                    println!();
                    let me = ov.game.local_lobby_id();
                    pending = Some((snap.turn, snap.opponent.lobby_id, hp(&ov.game, me), hp(&ov.game, snap.opponent.lobby_id)));
                }
                GameEvent::GameEnded(r) => {
                    println!("── finished {} with {}", ordinal(r.place), ov.db.name(&r.hero_card_id))
                }
            }
        }
        if let Some((turn, opp, mine, theirs)) = pending {
            if !ov.game.in_combat() && ov.game.turn() > turn {
                let (lost, dealt) = (mine - hp(&ov.game, ov.game.local_lobby_id()), theirs - hp(&ov.game, opp));
                let outcome = match (dealt > 0, lost > 0) {
                    (true, _) => format!("won, dealt {dealt}"),
                    (_, true) => format!("lost, took {lost}"),
                    _ => "tie (or no damage)".to_string(),
                };
                println!("         actual: {outcome}");
                pending = None;
            }
        }
    }
    let tribes = ov.tribes();
    println!("lobby tribes seen: {}", tribes.iter().map(|t| t.label()).collect::<Vec<_>>().join(", "));
    Ok(())
}

fn ordinal(n: i32) -> String {
    let suffix = match (n % 10, n % 100) {
        (1, 11) | (2, 12) | (3, 13) => "th",
        (1, _) => "st",
        (2, _) => "nd",
        (3, _) => "rd",
        _ => "th",
    };
    format!("{n}{suffix}")
}
