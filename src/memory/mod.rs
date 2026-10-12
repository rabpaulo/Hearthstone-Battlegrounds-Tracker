//! Lobby tribes read from the game's memory.
//!
//! The server tells the client which tribes are in the lobby before the hero
//! pick, but the client never writes them to a log; Hearthstone keeps them in
//! `GameState.s_instance.m_availableRacesInBattlegroundsExcludingAmalgam`
//! (a `List<TAG_RACE>`). This is the same read-only lookup HDT and Firestone
//! do. Reading needs ptrace-level access to the game process: Proton/umu
//! containers grant it to the user who started them; plain Wine under
//! `kernel.yama.ptrace_scope=1` does not (see the README).

pub mod mono;

use std::fs::File;
use std::os::unix::fs::FileExt;
use std::path::Path;
use std::sync::mpsc::{channel, Receiver, Sender};
use std::time::Duration;

use anyhow::{Context, Result};

use crate::cards::Tribe;
use mono::{Field, Memory, Mono};

const MONO_DLL: &str = "mono-2.0-bdwgc.dll";

struct ProcMem {
    file: File,
}

impl Memory for ProcMem {
    fn read_into(&self, addr: u64, buf: &mut [u8]) -> std::io::Result<()> {
        self.file.read_exact_at(buf, addr)
    }
}

/// Pid of the running `Hearthstone.exe`.
pub fn hearthstone_pid() -> Option<i32> {
    for entry in std::fs::read_dir("/proc").ok()?.flatten() {
        let Ok(pid) = entry.file_name().to_string_lossy().parse::<i32>() else { continue };
        let Ok(cmdline) = std::fs::read(entry.path().join("cmdline")) else { continue };
        let exe = cmdline.split(|&b| b == 0).next().unwrap_or_default();
        let exe = String::from_utf8_lossy(exe);
        let name = exe.rsplit(['\\', '/']).next().unwrap_or("");
        if name.eq_ignore_ascii_case("Hearthstone.exe") {
            return Some(pid);
        }
    }
    None
}

/// Load address of a module (the mapping of its first page).
fn module_base(pid: i32, file_name: &str) -> Result<u64> {
    let maps = std::fs::read_to_string(format!("/proc/{pid}/maps"))?;
    maps.lines()
        .filter(|l| l.ends_with(file_name))
        .filter(|l| l.split_whitespace().nth(2) == Some("00000000"))
        .filter_map(|l| u64::from_str_radix(l.split('-').next()?, 16).ok())
        .min()
        .with_context(|| format!("{file_name} is not loaded"))
}

/// Mono TAG_RACE values.
pub fn tribe_from_race(race: i32) -> Option<Tribe> {
    Some(match race {
        11 => Tribe::Undead,
        14 => Tribe::Murloc,
        15 => Tribe::Demon,
        17 => Tribe::Mech,
        18 => Tribe::Elemental,
        20 => Tribe::Beast,
        23 => Tribe::Pirate,
        24 => Tribe::Dragon,
        43 => Tribe::Quilboar,
        92 => Tribe::Naga,
        126 => Tribe::Aberration,
        _ => return None,
    })
}

/// Resolved once per game process; class pointers do not move.
pub struct GameStateReader<M> {
    mono: Mono<M>,
    class: u64,
    instance: Field,
    races: Field,
}

/// What the client holds for the current game.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LobbyRaces {
    /// Address of the `GameState`, which identifies the game.
    pub game: u64,
    pub tribes: Vec<Tribe>,
}

impl<M: Memory> GameStateReader<M> {
    pub fn new(mono: Mono<M>) -> Result<GameStateReader<M>> {
        let image = mono.image("Assembly-CSharp")?;
        let class = mono.class(image, "", "GameState")?;
        let instance = mono.field(class, "s_instance")?;
        let races = mono.field(class, "m_availableRacesInBattlegroundsExcludingAmalgam")?;
        anyhow::ensure!(instance.is_static && !races.is_static, "unexpected GameState layout");
        Ok(GameStateReader { mono, class, instance, races })
    }

    /// `None` outside a game (no `GameState`) or before the server sent the
    /// lobby tribes.
    pub fn read(&self) -> Result<Option<LobbyRaces>> {
        let statics = self.mono.static_data(self.class)?;
        if statics == 0 {
            return Ok(None);
        }
        let game = self.mono.mem.u64(statics + self.instance.offset)?;
        if game == 0 {
            return Ok(None);
        }
        let list = self.mono.mem.u64(game + self.races.offset)?;
        if list == 0 {
            return Ok(None);
        }
        let mut tribes: Vec<Tribe> = self.mono.int_list(list)?.into_iter().filter_map(tribe_from_race).collect();
        tribes.sort();
        tribes.dedup();
        Ok((!tribes.is_empty()).then_some(LobbyRaces { game, tribes }))
    }
}

fn attach(pid: i32) -> Result<GameStateReader<ProcMem>> {
    let file = File::open(format!("/proc/{pid}/mem")).context("opening the game's memory")?;
    let base = module_base(pid, MONO_DLL)?;
    GameStateReader::new(Mono::attach(ProcMem { file }, base)?)
}

/// One-off reading, for diagnostics.
pub fn read_once(pid: i32) -> Result<Option<LobbyRaces>> {
    let reader = attach(pid).map_err(|e| {
        if is_permission_error(&e) {
            e.context("no permission to read the game's memory (see README: ptrace)")
        } else {
            e
        }
    })?;
    reader.read()
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum MemoryEvent {
    /// Current lobby tribes, or `None` while not in a game.
    Lobby(Option<LobbyRaces>),
    /// Reading is impossible (e.g. permissions); shop inference stays in use.
    Unavailable(String),
}

/// Sends `ev` unless it repeats the previous event. `Err` when the receiver
/// is gone; `Ok(true)` when the event was new.
fn send_changed(tx: &Sender<MemoryEvent>, last: &mut Option<MemoryEvent>, ev: MemoryEvent) -> Result<bool, ()> {
    if last.as_ref() == Some(&ev) {
        return Ok(false);
    }
    *last = Some(ev.clone());
    tx.send(ev).map(|_| true).map_err(|_| ())
}

fn is_permission_error(e: &anyhow::Error) -> bool {
    e.chain().any(|c| {
        c.downcast_ref::<std::io::Error>().is_some_and(|io| io.kind() == std::io::ErrorKind::PermissionDenied)
    })
}

/// Polls the game process in the background; sends changes only.
pub fn spawn_reader() -> Receiver<MemoryEvent> {
    let (tx, rx) = channel();
    std::thread::spawn(move || {
        let mut reader: Option<(i32, GameStateReader<ProcMem>)> = None;
        let mut last = None;
        loop {
            let alive = reader.as_ref().is_some_and(|(pid, _)| Path::new(&format!("/proc/{pid}")).exists());
            if !alive {
                reader = None;
                match hearthstone_pid() {
                    None => {
                        if send_changed(&tx, &mut last, MemoryEvent::Lobby(None)).is_err() {
                            return;
                        }
                    }
                    Some(pid) => match attach(pid) {
                        Ok(r) => {
                            log::info!("reading lobby tribes from the game (pid {pid})");
                            reader = Some((pid, r));
                        }
                        Err(e) => {
                            let permission = is_permission_error(&e);
                            let why = if permission {
                                "no permission to read the game's memory".to_string()
                            } else {
                                format!("{e:#}")
                            };
                            match send_changed(&tx, &mut last, MemoryEvent::Unavailable(why.clone())) {
                                Err(()) => return,
                                Ok(true) => log::warn!("lobby tribes from memory unavailable: {why}"),
                                Ok(false) => {}
                            }
                            // The runtime may still be loading; try again later.
                            std::thread::sleep(Duration::from_secs(if permission { 30 } else { 5 }));
                            continue;
                        }
                    },
                }
            }
            if let Some((_, r)) = &reader {
                match r.read() {
                    Ok(lobby) => {
                        if send_changed(&tx, &mut last, MemoryEvent::Lobby(lobby)).is_err() {
                            return;
                        }
                    }
                    Err(e) => {
                        log::debug!("memory read failed: {e:#}");
                        reader = None;
                    }
                }
            }
            std::thread::sleep(Duration::from_millis(700));
        }
    });
    rx
}

#[cfg(test)]
mod tests {
    use super::mono::fake::FakeMemory;
    use super::*;

    /// Lays out a miniature Mono runtime: a PE export for
    /// `mono_get_root_domain`, one assembly with a `GameState` class whose
    /// static `s_instance` holds a list of races.
    fn fake_runtime(races: &[i32]) -> (FakeMemory, u64) {
        let mut m = FakeMemory::new();
        // --- PE image with one export.
        let base = m.alloc(0x2000);
        m.write(base, b"MZ");
        m.put_u32(base + 0x3c, 0x80);
        m.write(base + 0x80, b"PE\0\0");
        let opt = base + 0x80 + 24;
        m.write(opt, &0x20bu16.to_le_bytes());
        m.put_u32(opt + 112, 0x200); // export directory RVA
        let exp = base + 0x200;
        m.put_u32(exp + 24, 1);
        m.put_u32(exp + 28, 0x300); // functions
        m.put_u32(exp + 32, 0x310); // names
        m.put_u32(exp + 36, 0x320); // ordinals
        m.put_u32(base + 0x300, 0x400);
        m.put_u32(base + 0x310, 0x330);
        m.write(base + 0x330, b"mono_get_root_domain\0");
        // mov rax, [rip+disp]; ret  -> global at base+0x800
        let func = base + 0x400;
        let disp = (0x800i64 - (0x400 + 7)) as i32;
        m.write(func, &[0x48, 0x8b, 0x05]);
        m.write(func + 3, &disp.to_le_bytes());
        m.write(func + 7, &[0xc3]);

        // --- domain → assemblies.
        let domain = m.alloc(0x200);
        m.put_u64(base + 0x800, domain);
        let other_asm = m.alloc(0x100);
        let other_name = m.string("mscorlib");
        m.put_u64(other_asm + 0x10, other_name);
        let asm = m.alloc(0x100);
        let asm_name = m.string("Assembly-CSharp");
        m.put_u64(asm + 0x10, asm_name);
        let image = m.alloc(0x600);
        m.put_u64(asm + 0x60, image);
        let node2 = m.alloc(16);
        m.put_u64(node2, asm);
        let node1 = m.alloc(16);
        m.put_u64(node1, other_asm);
        m.put_u64(node1 + 8, node2);
        m.put_u64(domain + 0xa0, node1);

        // --- classes: a decoy chained before GameState in one bucket.
        let empty_ns = m.string("");
        let mk_class = |m: &mut FakeMemory, name: &str| {
            let k = m.alloc(0x200);
            let n = m.string(name);
            m.put_u64(k + 0x48, n);
            m.put_u64(k + 0x50, empty_ns);
            k
        };
        let decoy = mk_class(&mut m, "Decoy");
        let game_state = mk_class(&mut m, "GameState");
        m.put_u64(decoy + 0x108, game_state);
        let table = m.alloc(8 * 4);
        m.put_u64(table + 8 * 2, decoy);
        m.put_u32(image + 0x4d0 + 0x18, 4);
        m.put_u64(image + 0x4d0 + 0x20, table);

        // Fields: s_instance (static, offset 0) and the races list (offset 16).
        let mk_type = |m: &mut FakeMemory, attrs: u32| {
            let t = m.alloc(16);
            m.put_u32(t + 8, attrs);
            t
        };
        let fields = m.alloc(0x20 * 2);
        let static_type = mk_type(&mut m, 0x10 | (0x12 << 16));
        let inst_type = mk_type(&mut m, 0x15 << 16);
        let n_inst = m.string("s_instance");
        let n_races = m.string("m_availableRacesInBattlegroundsExcludingAmalgam");
        m.put_u64(fields, static_type);
        m.put_u64(fields + 8, n_inst);
        m.put_u32(fields + 0x18, 0);
        m.put_u64(fields + 0x20, inst_type);
        m.put_u64(fields + 0x28, n_races);
        m.put_u32(fields + 0x38, 16);
        m.put_u64(game_state + 0x98, fields);
        m.put_u32(game_state + 0x100, 2);
        m.put_u32(game_state + 0x5c, 3); // vtable slots

        // Static storage via runtime_info → vtable → after 3 method slots.
        let statics = m.alloc(16);
        let vtable = m.alloc(0x48 + 8 * 4);
        m.put_u64(vtable + 0x48 + 8 * 3, statics);
        let runtime_info = m.alloc(16);
        m.put_u64(runtime_info + 8, vtable);
        m.put_u64(game_state + 0xd0, runtime_info);

        // The GameState instance and its List<TAG_RACE>.
        let list_class = mk_class(&mut m, "List`1");
        let list_fields = m.alloc(0x20 * 2);
        let n_items = m.string("_items");
        let n_size = m.string("_size");
        m.put_u64(list_fields, inst_type);
        m.put_u64(list_fields + 8, n_items);
        m.put_u32(list_fields + 0x18, 0x10);
        m.put_u64(list_fields + 0x20, inst_type);
        m.put_u64(list_fields + 0x28, n_size);
        m.put_u32(list_fields + 0x38, 0x18);
        m.put_u64(list_class + 0x98, list_fields);
        m.put_u32(list_class + 0x100, 2);
        let list_vtable = m.alloc(16);
        m.put_u64(list_vtable, list_class);
        let array = m.alloc(0x20 + 4 * races.len().max(1) + 8);
        for (i, r) in races.iter().enumerate() {
            m.put_u32(array + 0x20 + 4 * i as u64, *r as u32);
        }
        let list = m.alloc(0x20);
        m.put_u64(list, list_vtable);
        m.put_u64(list + 0x10, array);
        m.put_u32(list + 0x18, races.len() as u32);
        let instance = m.alloc(0x40);
        m.put_u64(instance + 16, list);
        m.put_u64(statics, instance);
        (m, base)
    }

    #[test]
    fn reads_lobby_tribes_through_mono_structures() {
        // Beast, Mech, Murloc, Pirate, Undead (+ an unknown race id).
        let (mem, base) = fake_runtime(&[20, 17, 14, 23, 11, 999]);
        let reader = GameStateReader::new(Mono::attach(mem, base).unwrap()).unwrap();
        let lobby = reader.read().unwrap().unwrap();
        assert_eq!(lobby.tribes, vec![Tribe::Beast, Tribe::Mech, Tribe::Murloc, Tribe::Pirate, Tribe::Undead]);
        assert_ne!(lobby.game, 0);
    }

    #[test]
    fn empty_list_means_not_known_yet() {
        let (mem, base) = fake_runtime(&[]);
        let reader = GameStateReader::new(Mono::attach(mem, base).unwrap()).unwrap();
        assert_eq!(reader.read().unwrap(), None);
    }
}
