//! ObjAct sync (experimental, `[GAMEPLAY] sync_objact`, NOT verified live; FORK.md
//! "ObjAct sync").
//!
//! Problem: only event flags travel (world.rs). A door / gate / chest whose ObjAct state keeps
//! no flag (`SprjObjActOneState`) is never replayed by doors.rs, and even a replayed object
//! does not let the map's EMEVD script go on: door templates (common_func 20005616 / 20005615,
//! m11_01 11115620) and chests (20005510 / 20005511) wait for `ObjActEventFlag(ObjActEntityID)`
//! before they set the door flag, disable the ObjAct, deactivate the blocker and rebuild the
//! navmesh, or enable the chest's treasure.
//!
//! RE (Sekiro 1.06, dump 2026-11-11, `E:\RE\work_objact`) [VAR]:
//! - `0x6BA7B0(block, index, actor) -> u64` is the local start: `info+0x70/+0x71/+0x72 = 1`,
//!   value = `state->vt[6]()`, request `0x6BCBC0`, then `0x6BC1B0(block, req, 0)`.
//!   `0x6BA910(block, index, actor, value)` is the replay (type 45 receive `0x6BE740`,
//!   doors.rs): `state->vt[5](value)` (flag classes: `SetEventFlag(flag, value != 0)`), then
//!   `+0x70 = ObjActParam.isEventKickSync (row+0x1B)`, `+0x71 = 1`, `+0x72 = 0`, no packet 45.
//! - `ObjActEventFlag(X)` is `EcTestObjActExecuted` (vtable `0x29FDCF8`, test `0x8FCB90`): true
//!   while the list `[[0x143D56058]+0x70]` (`SprjEventObjActExecInfo`, cleared once per event
//!   step by `0x6C76D0` right after the EMEVD update `0x67D180`) holds a node `{+0x10 = 0,
//!   +0x14 = X}`. The only writer is `0x6B2AF0(info)`: if `info+0x70 != 0` and the
//!   ObjActEntityID `info+0x0C != 0` it pushes `{0, info+0x0C}` (`0x6B3CC0`), then `+0x70 = 0`.
//!   `0x6BC1B0` calls it at its end when `ObjActParam.eventKickTiming (row+0x25) == 1`, the
//!   update `0x6B35D0` when the object animation ends and timing is 0.
//! - Every door / chest row (9999xx, 90075/6) has isEventKickSync 0 and eventKickTiming 1, so a
//!   replay (ours, doors.rs, the game's own type 45) never kicks the EMEVD script. The actor
//!   (`req+0x0C`, a chr handle, chr+0x08) only moves / animates that chr; the kick ignores it.
//! - info: `+0x08` index, `+0x0C` ObjActEntityID (= MSB `objActEntityId`, checked against
//!   `objacts_msb.tsv` for 31 infos of m11_01 / m11_02 / m13_00 in the dump), `+0x50` object
//!   handle (not an entity id), `+0x54` run state, `+0x78` ObjActParam id, `+0x80` its row.
//!
//! So the receiver replays with `0x6BA910(block, index, -1, value)` (no state toggle, no
//! packet 45, no detour echo) and then kicks: `info+0x70 = 1; 0x6B2AF0(info)`, the very push
//! the acting player's `0x6BC1B0` makes. `0x6BA7B0` on the receiver would kick too, but it
//! also toggles the door flag (`state->vt[4]` = `SetEventFlag(flag, !flag)`: the friend's
//! flag, already applied here, would go off) and sends packet 45 at stage 4.
//!
//! - **Sender** (`[DEBUG] objact_sender`): `hook` (default) detours `0x6BA7B0` (prologue
//!   checked at install, as the closed 1.0.11 `objsync` does); an execution whose actor is our
//!   player's handle and that started (run state 3) goes out. `poll` (fallback, also used when
//!   the detour is not installed): the run-state walk of the first version (`decide`).
//!   The walk runs in both modes: the receiver needs its idle / executed facts.
//! - **Message v2** (bulk `BULK_OBJACT`, every peer): map id, ObjActEntityID, info index,
//!   sub index (`+0xAC`), ObjActParam id, state class, target value, sequence number.
//! - **Receiver** (game thread, 300 ms after arrival, retried for 5 s): the info by
//!   ObjActEntityID in the loaded block with that map id, by index / sub only when the id is
//!   not there (a non-zero id that differs refuses). `receive_plan`: replay + kick, or only the
//!   kick when the ObjAct already ran here as a replay (`+0x72 = 0`: the game's type 45), or
//!   nothing (see the reasons).
//! - **Journal**: every executed ObjAct (ours and the peers', door states open / one-state;
//!   no one-loop) by (map id, ObjActEntityID) for the session (new session = empty, at most
//!   `MAX_JOURNAL`, never on disk). An info of a journaled ObjAct that is loaded, idle and
//!   watched for `JOURNAL_SETTLE`, has not run since the load and whose object is not at that
//!   animation is replayed and kicked once ("doors close again after a reload", issue #3).
//!
//! **What is synced** (2026-11-12, after the first live test): by `doors::kind` of the
//! ObjActParam id, not by the state class (table in doors.rs: every Sekiro bar door and gate
//! is a one-loop state, the temple lift levers are one-state). `sync_objact = 1`: Door and
//! Chest of any class; 2: everything with a known class.
//!
//! **One-loop doors** (doors.rs header, [VAR] Ghidra): every execution plays the same
//! opening animation and toggles the MSB flag, so "open" = flag ON and there is no closing
//! execution (the door templates disable the ObjAct after it opened). Hence:
//! - the target state of a one-loop door is always 1 (open), whatever the flag did; a
//!   message with target 0 for one is refused;
//! - receiver: replay `0x6BA910(.., 1)` (`vt[5]` SETS the flag to 1, no toggle) unless the
//!   object is already at the opening animation (`reached`); whichever comes first, the
//!   friend's flag (world.rs) or this message, the result is flag ON + door open, and the
//!   other one is a no-op (flag already 1 / doors.rs `replayed_recently` / object reached);
//! - sender: our player using a door whose flag is already ON (the friend's flag came, the
//!   object did not move yet) toggles it OFF; the detour notes it and doors.rs guard 5 writes
//!   it back ON without sending the OFF (live 2026-11-12: 61110602 ended OFF on both and the
//!   gate would load closed with its ObjAct disabled);
//! - never journaled: the engine sets the door up from its flag at every load (20005616 has
//!   no `ReproduceObjectAnimation`) [ARV], so a reload converges by itself and a journal
//!   replay could only fight a script (m11_00 11100600 closes gate 61100610 in phase 8301).

use std::collections::{HashMap, HashSet, VecDeque};
use std::sync::Mutex;
use std::sync::atomic::{AtomicBool, AtomicU8, AtomicUsize, Ordering};
use std::time::{Duration, Instant};

use crate::doors::{
    self, BLOCK_DESC_SLOT, BLOCK_INFO_COUNT, BLOCK_INFOS, BLOCK_LOADED, BLOCK_STRIDE, Class, GameMem, INFO_HANDLE, INFO_INDEX,
    INFO_PARAM_ID, INFO_RUN_STATE, INFO_STATE, INFO_STRIDE, INFO_SUB, Kind, MAN_BLOCK_COUNT, MAN_BLOCKS, MAX_BLOCKS, Mem, REPLAY_RVA,
    RUN_EXECUTING, STATE_FLAG, STATE_VTS, WORLD_OBJACT_MAN, kind, le_ptr, le_u32, loop_door,
};
use crate::game;
use crate::hook;
use crate::log;
use crate::net::{Dest, Net};
use crate::peers::PeerId;

/// `[GAMEPLAY] sync_objact`: 0 off, 1 doors / gates / chests of any state class (default; by
/// ObjActParam, `doors::kind`), 2 also levers, lifts, bells, rotating walls / floors, windows.
pub static MODE: AtomicU8 = AtomicU8::new(1);
/// `[DEBUG] objact_debug`: log every execution, every map flag, raw infos and prologues.
pub static DEBUG: AtomicBool = AtomicBool::new(false);
/// `[DEBUG] objact_sender`: `SENDER_HOOK` (default) or `SENDER_POLL`.
pub static SENDER: AtomicU8 = AtomicU8::new(SENDER_HOOK);
pub const SENDER_HOOK: u8 = 0;
pub const SENDER_POLL: u8 = 1;

/// Bulk kind of one ObjAct execution (taken kinds: world.rs `BULK_PHASE` list, 46 pickups).
/// An older build logs `net: bulk transfer kind 167 received` and leaves it in its 24-entry
/// inbox, where newer transfers push it out.
pub const BULK_OBJACT: u8 = 0xA7;
const MAGIC: u8 = 0x4F;
/// 1: first build (no ObjActEntityID); 2: + ObjActEntityID and ObjActParam id (this build).
const WIRE_VERSION: u8 = 2;
/// Reason byte: an execution by the sender's own player.
const REASON_EXEC: u8 = 1;
const WIRE_LEN: usize = 29;

/// The local start `0x6BA7B0(block, index, actor)`: `mov [rsp+10],rsi; mov [rsp+18],r8d;
/// push rdi; sub rsp,70` (15 bytes, no RIP-relative operand) [VAR, dump].
const OWN_START_RVA: usize = 0x6BA7B0;
const OWN_START_PROLOGUE: [u8; 15] = [0x48, 0x89, 0x74, 0x24, 0x10, 0x44, 0x89, 0x44, 0x24, 0x18, 0x57, 0x48, 0x83, 0xEC, 0x70];
/// `0x6B2AF0(info)`: pushes the ObjActEventFlag record when `info+0x70` [VAR]. Checked before
/// the first call: `mov rax,rsp; push rbp; lea rbp,[rax-5F]; sub rsp,A0`.
const KICK_RVA: usize = 0x6B2AF0;
const KICK_PROLOGUE: [u8; 15] = [0x48, 0x8B, 0xC4, 0x55, 0x48, 0x8D, 0x68, 0xA1, 0x48, 0x81, 0xEC, 0xA0, 0x00, 0x00, 0x00];
/// `FD4Singleton` of the event manager whose `+0x70` is the executed-ObjAct list; `0x6B3CC0`
/// stops the game (assert) when it is null, so it is checked before every kick.
const EVENT_MAN: usize = 0x3D56058;
const EVENT_MAN_OBJACT_LIST: usize = 0x70;

/// Info bytes [VAR]: `+0x70` "kick the EMEVD", `+0x71` (113) set by every execution, `+0x72`
/// (114) 1 after a local start `0x6BA7B0`, 0 after a replay `0x6BA910`.
const INFO_KICK: usize = 0x70;
const INFO_B113: usize = 0x71;
const INFO_B114: usize = 0x72;
const INFO_ENTITY: usize = 0x0C;
const INFO_PARAM_ROW: usize = 0x80;
/// Primary info of a door's sub infos [VAR, §37].
const INFO_PRIMARY: usize = 0xB0;
/// ObjActParam row: isEventKickSync (u8 +0x1B) [VAR, dump vs csv; eventKickTiming is +0x25].
const PARAM_KICK_SYNC: usize = 0x1B;
/// ChrIns handle (`0x6BE740` passes `chr+8` as the actor) [VAR].
const CHR_HANDLE: usize = 0x08;
/// The block descriptor is a `WorldBlockInfo` [ARV]: its `+0x20` is the slot doors.rs and
/// `0x6BA910` use; `+0x08` is `block_id` (area << 24 | block << 16 ...), `+0x18` the
/// `WorldAreaInfo`, whose `+0x0B` area number must equal the id's top byte (checked).
const DESC_BLOCK_ID: usize = 0x08;
const DESC_AREA: usize = 0x18;
const AREA_NUMBER: usize = 0x0B;

/// The actor of our replay: -1, no chr plays the animation (the friend's replica already
/// plays it through the motion sync; the EMEVD kick does not need an actor) [VAR].
const REPLAY_ACTOR: i32 = -1;

/// Infos read per block (a garbage count must not make a poll read megabytes).
const MAX_BLOCK_INFOS: usize = 512;
/// Infos read per poll, all blocks together.
const MAX_POLL_INFOS: usize = 2048;
const POLL_EVERY: Duration = Duration::from_millis(50);
/// Poll sender: an execution is judged this long after its run state turned 3.
const DECIDE_AFTER: Duration = Duration::from_millis(150);
const OWN_ANIM_NEAR: Duration = Duration::from_secs(2);
/// An execution this long after a `0x6BA910` of ours / doors.rs on that ObjAct is a replay.
const REPLAY_MARK_FOR: Duration = Duration::from_secs(3);
/// Another info of the same ObjAct (door sub infos) within this is the same execution.
const SAME_OBJACT_FOR: Duration = Duration::from_secs(2);
const APPLY_AFTER: Duration = Duration::from_millis(300);
const RETRY_EVERY: Duration = Duration::from_millis(250);
const APPLY_WITHIN: Duration = Duration::from_secs(5);
const MAX_INCOMING: usize = 32;
/// Detour events waiting for the frame task (more = dropped, counted).
const MAX_HOOK_EVENTS: usize = 32;
/// Journal: entries per session, how long an info must be watched after its load, how often
/// the loaded infos are compared with it.
const MAX_JOURNAL: usize = 256;
const JOURNAL_SETTLE: Duration = Duration::from_secs(3);
const JOURNAL_EVERY: Duration = Duration::from_secs(1);
/// Map flags logged this long before and after an execution of ours.
const FLAG_WINDOW: Duration = Duration::from_secs(5);
const SENDER_LOOK: Duration = Duration::from_secs(1);
const RECEIVER_LOOKS: [Duration; 2] = [Duration::from_secs(1), Duration::from_secs(3)];
/// Raw info dumps per load (`objact_debug`).
const MAX_INFO_DUMPS: u32 = 8;
const MAX_FLAGS: usize = 256;
/// `0x6BA7B0` calls by another actor logged per load without `objact_debug`.
const MAX_FOREIGN_LOGS: u32 = 5;
/// Objects whose "no ObjActEntityID" line was logged (beyond: silent).
const MAX_NO_ENTITY_LOGGED: usize = 256;

/// One execution on the wire.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct Msg {
    map: u32,
    /// ObjActEntityID (`info+0x0C`); 0 = none (the receiver uses index / sub).
    entity: i32,
    index: i32,
    sub: i32,
    /// ObjActParam id (`info+0x78`).
    param: i32,
    /// The game's state type: 0 one-state, 1 door, 2 one-loop, 4 one-dir door, 0xFF unknown.
    class: u8,
    /// Value for `0x6BA910` (flag classes: the flag after the execution; one-state: 1).
    state: u8,
    seq: u32,
}

fn encode(m: &Msg) -> Vec<u8> {
    let mut d = Vec::with_capacity(WIRE_LEN);
    d.extend_from_slice(&[MAGIC, WIRE_VERSION, REASON_EXEC, m.class, m.state]);
    for v in [m.map, m.entity as u32, m.index as u32, m.sub as u32, m.param as u32, m.seq] {
        d.extend_from_slice(&v.to_le_bytes());
    }
    d
}

fn decode(d: &[u8]) -> Result<Msg, &'static str> {
    if d.len() < 3 || d[0] != MAGIC {
        return Err("not an ObjAct message");
    }
    match d[1] {
        WIRE_VERSION => {}
        1 => return Err("version 1 (an older build of the fork: no ObjActEntityID; both players need the same build)"),
        _ => return Err("another message version (a newer build?)"),
    }
    if d.len() < WIRE_LEN {
        return Err("too short for version 2");
    }
    if d[2] != REASON_EXEC {
        return Err("unknown reason byte");
    }
    let u = |at: usize| u32::from_le_bytes(d[at..at + 4].try_into().unwrap());
    Ok(Msg { class: d[3], state: d[4], map: u(5), entity: u(9) as i32, index: u(13) as i32, sub: u(17) as i32, param: u(21) as i32, seq: u(25) })
}

fn class_code(c: Option<Class>) -> u8 {
    match c {
        Some(Class::One) => 0,
        Some(Class::Door) => 1,
        Some(Class::OneLoop) => 2,
        Some(Class::OneDirDoor) => 4,
        None => 0xFF,
    }
}

fn class_name(code: u8) -> &'static str {
    match code {
        0 => "one-state",
        1 => "door",
        2 => "one-loop",
        4 => "one-dir-door",
        _ => "unknown",
    }
}

/// `m11_00_00_00 (0x0b000000)`, `?` when unknown.
fn map_name(id: Option<u32>) -> String {
    match id {
        Some(id) => format!("m{:02}_{:02}_{:02}_{:02} ({id:#010x})", id >> 24, id >> 16 & 0xFF, id >> 8 & 0xFF, id & 0xFF),
        None => "?".to_owned(),
    }
}

fn hex(b: &[u8]) -> String {
    b.iter().map(|x| format!("{x:02x}")).collect::<Vec<_>>().join(" ")
}

/// Map / block flags (8 digits: every group, the door flags seen live 611xxxxx and the
/// groups 1/2 of doors.rs `map_flag` included).
fn logged_flag(id: u32) -> bool {
    id >= 10_000_000
}

fn signed_ms(t: Instant, from: Instant) -> i64 {
    if t >= from { t.duration_since(from).as_millis() as i64 } else { -(from.duration_since(t).as_millis() as i64) }
}

// ------------------------------------------------------------------ walk

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct Block {
    addr: usize,
    slot: i32,
    map: Option<u32>,
    infos: usize,
    count: usize,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct Info {
    addr: usize,
    index: i32,
    /// ObjActEntityID (`+0x0C`).
    entity: i32,
    /// ObjActParam id (`+0x78`).
    param: i32,
    sub: i32,
    run: u32,
    kick: u8,
    b113: u8,
    b114: u8,
    state: usize,
    primary: usize,
    handle: u32,
}

impl Info {
    /// One key for all infos of one ObjAct (a door's sub infos share their primary).
    fn objact_key(&self) -> usize {
        key_of(self.addr, self.primary)
    }
}

fn key_of(addr: usize, primary: usize) -> usize {
    if primary >= 0x10000 { primary } else { addr }
}

fn parse_info(addr: usize, r: &[u8]) -> Info {
    Info {
        addr,
        index: le_u32(r, INFO_INDEX) as i32,
        entity: le_u32(r, INFO_ENTITY) as i32,
        param: le_u32(r, INFO_PARAM_ID) as i32,
        sub: le_u32(r, INFO_SUB) as i32,
        run: le_u32(r, INFO_RUN_STATE),
        kick: r[INFO_KICK],
        b113: r[INFO_B113],
        b114: r[INFO_B114],
        state: le_ptr(r, INFO_STATE),
        primary: le_ptr(r, INFO_PRIMARY),
        handle: le_u32(r, INFO_HANDLE),
    }
}

/// The block's map id from its descriptor, checked against its area (see `DESC_BLOCK_ID`).
fn map_id(mem: &impl Mem, desc: usize) -> Option<u32> {
    let id = mem.u32(desc + DESC_BLOCK_ID)?;
    let area = mem.ptr(desc + DESC_AREA)?;
    let n = mem.u8(area + AREA_NUMBER)?;
    (n != 0 && id >> 24 == u32::from(n)).then_some(id)
}

/// The loaded blocks of WorldObjActMan (`base` = module base; layout as doors.rs `scan`).
fn blocks(mem: &impl Mem, base: usize) -> Vec<Block> {
    let mut out = Vec::new();
    let Some(man) = mem.ptr(base + WORLD_OBJACT_MAN) else { return out };
    let (Some(arr), Some(count)) = (mem.ptr(man + MAN_BLOCKS), mem.u32(man + MAN_BLOCK_COUNT)) else { return out };
    if count > MAX_BLOCKS {
        return out;
    }
    for b in 0..count as usize {
        let addr = arr + BLOCK_STRIDE * b;
        if mem.u8(addr + BLOCK_LOADED).unwrap_or(0) == 0 {
            continue;
        }
        let Some(desc) = mem.ptr(addr) else { continue };
        let (Some(slot), Some(infos), Some(n)) = (mem.i32(desc + BLOCK_DESC_SLOT), mem.ptr(addr + BLOCK_INFOS), mem.i32(addr + BLOCK_INFO_COUNT)) else {
            continue;
        };
        let count = usize::try_from(n).unwrap_or(0).min(MAX_BLOCK_INFOS);
        out.push(Block { addr, slot, map: map_id(mem, desc), infos, count });
    }
    out
}

/// Every info of `b`, read in one piece (`buf` is reused between calls).
fn infos(mem: &impl Mem, b: &Block, buf: &mut Vec<u8>) -> Vec<Info> {
    buf.clear();
    buf.resize(INFO_STRIDE * b.count, 0);
    if b.count == 0 || !mem.bytes(b.infos, buf) {
        return Vec::new();
    }
    (0..b.count).map(|i| parse_info(b.infos + INFO_STRIDE * i, &buf[INFO_STRIDE * i..INFO_STRIDE * (i + 1)])).collect()
}

/// Info `index` of `b` (array position = index, as `0x6BA7B0` addresses it).
fn info_at(mem: &impl Mem, b: &Block, index: i32) -> Option<Info> {
    let ix = usize::try_from(index).ok().filter(|&ix| ix < b.count)?;
    let mut r = [0u8; INFO_STRIDE];
    let addr = b.infos + INFO_STRIDE * ix;
    mem.bytes(addr, &mut r).then(|| parse_info(addr, &r))
}

fn class_of(mem: &impl Mem, base: usize, state: usize) -> Option<Class> {
    if state < 0x10000 {
        return None;
    }
    let vt = mem.ptr(state)?;
    STATE_VTS.iter().find(|(rva, _)| base + rva == vt).map(|&(_, c)| c)
}

fn flag_of(mem: &impl Mem, state: usize, class: Class) -> Option<u32> {
    if !class.has_flag() {
        return None;
    }
    mem.u32(state + STATE_FLAG).filter(|&f| f != 0 && f != u32::MAX)
}

/// How the receiver found the info.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum How {
    Entity,
    Index,
}

fn how_text(how: How, entity: i32) -> String {
    match how {
        How::Entity => "found by ObjActEntityID".to_owned(),
        How::Index if entity > 0 => format!("found by index (ObjActEntityID {entity} not in that block here)"),
        How::Index => "found by index (no ObjActEntityID sent)".to_owned(),
    }
}

/// The info for (`entity`, `index`, `sub`) among `all` (one block): by ObjActEntityID when it
/// is set and present, else by index / sub; an index hit whose own ObjActEntityID is set and
/// differs is refused (other map data).
fn locate(all: &[Info], entity: i32, index: i32, sub: i32) -> Result<(Info, How), &'static str> {
    if entity > 0
        && let Some(i) = all.iter().find(|i| i.entity == entity)
    {
        return Ok((*i, How::Entity));
    }
    let at = usize::try_from(index).ok().and_then(|i| all.get(i)).copied();
    let hit = at.filter(|i| i.index == index && i.sub == sub).or_else(|| all.iter().find(|i| i.index == index && i.sub == sub).copied());
    match hit {
        Some(i) if entity > 0 && i.entity > 0 && i.entity != entity => Err("the info at that index has another ObjActEntityID here"),
        Some(i) => Ok((i, How::Index)),
        None => Err("no info with that ObjActEntityID or index here"),
    }
}

/// The loaded block with map id `map` and the info in it (`locate`).
fn find(mem: &impl Mem, base: usize, map: u32, entity: i32, index: i32, sub: i32) -> Result<(Block, Info, How), &'static str> {
    let mut buf = Vec::new();
    let mut why = "that map block is not loaded here";
    for b in blocks(mem, base).into_iter().filter(|b| b.map == Some(map)) {
        match locate(&infos(mem, &b, &mut buf), entity, index, sub) {
            Ok((i, how)) => return Ok((b, i, how)),
            Err(e) => why = e,
        }
    }
    Err(why)
}

// ------------------------------------------------------------------ pure decisions

/// A poll saw run state `run` where the poll before saw `prev`: a new execution.
fn is_edge(prev: u32, run: u32) -> bool {
    run == RUN_EXECUTING && prev != RUN_EXECUTING
}

/// Poll sender: what says that a new execution here is our player's.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
struct Evidence {
    /// A `0x6BA910` of ours or doors.rs on this ObjAct a moment ago.
    our_replay: bool,
    /// `+0x72` was not 1 in the poll before the edge and is 1 at the decision.
    b114_rose: bool,
    /// `+0x72` is 1 at the decision (sticky: only a replay resets it).
    b114_now: bool,
    /// Our player started an ObjAct player animation within 2 s (not used up yet).
    own_anim: bool,
}

/// Does `sync_objact = mode` sync an ObjAct of state `class` and ObjActParam `param` (Ok) or
/// not (Err, why)? Mode 1: doors and chests of any class (`doors::kind`); 2: every known class.
fn syncs(class: Option<Class>, param: i32, mode: u8) -> Result<(), &'static str> {
    if class.is_none() {
        return Err("unknown state class");
    }
    if mode >= 2 || kind(param) != Kind::Other {
        return Ok(());
    }
    Err("not a door or chest by its ObjActParam (a lever, lift, bell, rotating wall / floor or window): synced only with sync_objact = 2")
}

/// Poll sender: send this execution (Ok, why) or not (Err, why).
fn send_decision(e: Evidence, class: Option<Class>, param: i32, mode: u8) -> Result<&'static str, &'static str> {
    if e.our_replay {
        return Err("our own replay of a friend's ObjAct (doors.rs / objact.rs)");
    }
    syncs(class, param, mode)?;
    if e.b114_rose {
        return Ok("info+0x72 went 0 -> 1");
    }
    match (e.b114_now, e.own_anim) {
        (true, true) => Ok("info+0x72 = 1 (from before) and our ObjAct animation"),
        (true, false) => Err("info+0x72 = 1 is left from an earlier execution and no ObjAct animation of ours"),
        (false, true) => Err("only our ObjAct animation as evidence (no object position known to check it is this object)"),
        (false, false) => Err("not ours (the friend's type 45, an NPC or a script)"),
    }
}

/// Hook sender: send our `0x6BA7B0` execution (Ok, why) or not (Err, why). `own` = our
/// player's handle, `started` = the info's run state is 3 right after the call.
fn hook_decision(class: Option<Class>, param: i32, mode: u8, actor: i32, own: Option<i32>, started: bool) -> Result<&'static str, &'static str> {
    if mode == 0 {
        return Err("sync_objact = 0");
    }
    match own {
        None => return Err("our player's handle is unknown"),
        Some(h) if h != actor => return Err("the actor is not our player (an NPC or a script's character)"),
        _ => {}
    }
    if !started {
        return Err("it did not start (request refused, or the object already plays that animation)");
    }
    syncs(class, param, mode)?;
    Ok("our player started it (0x6BA7B0)")
}

/// Value for the receiver's `0x6BA910`: a one-loop door 1 (every execution opens it, the
/// flag's toggle is a use parity: header), another flag state toggles (the opposite of the
/// flag before), a one-state takes 1 (its `vt[5]` ignores it).
fn target_state(class: Option<Class>, param: i32, flag_before: Option<bool>) -> u8 {
    if loop_door(class, param) {
        return 1;
    }
    match (class.is_some_and(Class::has_flag), flag_before) {
        (true, Some(v)) => u8::from(!v),
        _ => 1,
    }
}

/// An ObjAct (by `objact_key`) went out within `SAME_OBJACT_FOR`.
fn sent_lately(keys: &[(usize, Instant)], key: usize, now: Instant) -> bool {
    keys.iter().any(|&(k, t)| k == key && now.duration_since(t) < SAME_OBJACT_FOR)
}

/// Receiver facts about the info here (everything but the object).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct Here {
    executing: bool,
    /// Its run state equals the one it had when first seen after the load (None: not seen).
    idle: Option<bool>,
    /// doors.rs or we ran `0x6BA910` on it a moment ago.
    marked: bool,
    /// We replayed it already since the load.
    replayed: bool,
    /// It executed here since the load (our player, type 45, a script).
    executed: bool,
    /// `+0x72 = 1`: its last execution here was a local start (`0x6BA7B0`, its kick ran).
    local_start: bool,
    /// We kicked its EMEVD already since the load.
    kicked: bool,
    /// Our own player started it since the load (hook sender).
    ours: bool,
}

/// What the receiver does with a peer's execution.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Plan {
    /// `0x6BA910` + the EMEVD kick.
    Replay,
    /// It already ran here as a replay (the game's type 45): only the kick is missing.
    KickOnly,
}

/// Receiver, before the object is looked at (`0x6BCBC0` is only called for `Replay`).
fn receive_plan(class: Option<Class>, param: i32, state: u8, mode: u8, h: Here) -> Result<Plan, &'static str> {
    if mode == 0 {
        return Err("sync_objact = 0 here");
    }
    if state > 1 {
        return Err("target state is not 0 / 1 (bad message)");
    }
    syncs(class, param, mode)?;
    match class {
        Some(Class::One) if state != 1 => return Err("one-state with target state 0 (bad message)"),
        Some(Class::OneLoop) if state == 0 && loop_door(class, param) => {
            return Err("a one-loop door with target state 0: it has no closing execution (a flag toggle of a door that was open already)");
        }
        Some(Class::Door | Class::OneDirDoor) if state == 0 => {
            return Err("a door's toggle back to closed: never replayed (doors.rs keeps door flags on)");
        }
        _ => {}
    }
    if h.marked {
        return Err("doors.rs or objact.rs replayed it a moment ago");
    }
    if h.replayed {
        return Err("replayed once already since the load");
    }
    if h.kicked {
        return Err("its EMEVD was kicked here already since the load");
    }
    if h.executing || h.executed {
        if h.ours {
            return Err("our own player used it since the load");
        }
        if h.local_start {
            return Err("it ran here as a local start since the load (its EMEVD kick ran)");
        }
        return Ok(Plan::KickOnly);
    }
    match h.idle {
        None => Err("not watched yet since the load"),
        Some(false) => Err("its run state is not the one it had at the load"),
        Some(true) => Ok(Plan::Replay),
    }
}

/// Receiver, the object: replay only when it is found and not at that animation yet.
fn object_decision(reached: Option<bool>) -> Result<(), &'static str> {
    match reached {
        Some(false) => Ok(()),
        Some(true) => Err("the object is already at that execution's animation here"),
        None => Err("the object or its request was not found here"),
    }
}

/// The kick (`info+0x70 = 1; 0x6B2AF0(info)`) after a replay: `entity` = ObjActEntityID,
/// `kick_sync` = the row's isEventKickSync, `pending` = `info+0x70` now.
fn kick_plan(entity: i32, kick_sync: Option<u8>, pending: u8) -> Result<(), &'static str> {
    if entity == 0 {
        return Err("no ObjActEntityID (no EMEVD can wait for it)");
    }
    match kick_sync {
        None => Err("its ObjActParam row is unreadable"),
        Some(s) if s != 0 => Err("isEventKickSync = 1: the replay kicked it itself"),
        _ if pending != 0 => Err("a kick is pending already (info+0x70 = 1)"),
        _ => Ok(()),
    }
}

/// Journal: the loaded info has been watched long enough to be judged.
fn journal_gate(seen_for: Option<Duration>, tried: bool) -> Result<(), &'static str> {
    if tried {
        return Err("tried once already since its load");
    }
    match seen_for {
        None => Err("not watched yet"),
        Some(d) if d < JOURNAL_SETTLE => Err("not settled yet"),
        _ => Ok(()),
    }
}

fn near(a: Instant, b: Instant, within: Duration) -> bool {
    if a >= b { a.duration_since(b) <= within } else { b.duration_since(a) <= within }
}

// ------------------------------------------------------------------ journal

/// One executed ObjAct of the session.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct Entry {
    map: u32,
    entity: i32,
    index: i32,
    sub: i32,
    param: i32,
    class: u8,
    state: u8,
    /// Our own player's (else a peer's).
    ours: bool,
}

/// The session's executed ObjActs, newest last, keyed by (map id, ObjActEntityID).
#[derive(Default)]
struct Journal {
    entries: Vec<Entry>,
    dropped: u32,
}

impl Journal {
    /// Keeps `e` (replacing the same ObjAct) or says why not; a door closing again leaves.
    fn note(&mut self, e: Entry) -> Result<&'static str, &'static str> {
        if e.entity <= 0 {
            return Err("no ObjActEntityID");
        }
        let pos = self.entries.iter().position(|x| x.map == e.map && x.entity == e.entity);
        let other = kind(e.param) == Kind::Other;
        let keep = matches!(e.class, 0 | 1 | 4) && e.state == 1 && !other;
        if !keep {
            if let Some(p) = pos {
                self.entries.remove(p);
                return Err("closed again: taken out of the journal");
            }
            return Err(match e.class {
                2 if !other => "one-loop door (never journaled: the load sets it up from its flag)",
                2 => "one-loop state (never journaled: a lift must not move after a reload)",
                0 | 1 | 4 if other => "not a door or chest (never journaled: a lever or a rotating wall must not move after a reload)",
                0 | 1 | 4 => "a door closing",
                _ => "unknown state class",
            });
        }
        if let Some(p) = pos {
            self.entries.remove(p);
        } else if self.entries.len() >= MAX_JOURNAL {
            self.entries.remove(0);
            self.dropped += 1;
        }
        self.entries.push(e);
        Ok(if pos.is_some() { "journaled again" } else { "journaled" })
    }

    fn clear(&mut self) -> usize {
        let n = self.entries.len();
        self.entries.clear();
        n
    }
}

// ------------------------------------------------------------------ state

/// Per info since the load: run state and `+0x72` at the last poll, run state when idle,
/// when it was first seen.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct Seen {
    run: u32,
    b114: u8,
    idle: u32,
    first: Instant,
}

/// Poll: a new execution, judged `DECIDE_AFTER` later.
#[derive(Clone, Copy)]
struct Edge {
    at: Instant,
    block: Block,
    info: Info,
    /// `+0x72` in the poll before the edge.
    prev_b114: u8,
    class: Option<Class>,
    flag: Option<u32>,
    flag_at_edge: Option<bool>,
    start: Option<doors::ObjView>,
    replay: bool,
}

#[derive(Clone, Copy)]
struct Incoming {
    from: PeerId,
    msg: Msg,
    at: Instant,
    next_try: Instant,
}

/// A later look at one info (log only), re-resolved through the block walk.
#[derive(Clone)]
struct Look {
    due: Instant,
    what: String,
    block: usize,
    infos: usize,
    map: Option<u32>,
    slot: i32,
    index: i32,
    handle: u32,
    /// Class known: the object may be looked at.
    known: bool,
    flag: Option<u32>,
    state: u32,
}

#[derive(Default)]
struct State {
    seen: HashMap<usize, Seen>,
    alive: HashSet<usize>,
    buf: Vec<u8>,
    last_poll: Option<Instant>,
    last_in_world: Duration,
    edges: Vec<Edge>,
    incoming: Vec<Incoming>,
    looks: Vec<Look>,
    /// ObjActs (`objact_key`) we or doors.rs ran `0x6BA910` on, and when.
    replays: Vec<(usize, Instant)>,
    /// ObjActs we replayed since the load (one replay each).
    replayed_keys: HashSet<usize>,
    /// ObjActs that executed here since the load.
    executed_keys: HashSet<usize>,
    /// ObjActs our own player started since the load (hook sender).
    own_keys: HashSet<usize>,
    /// ObjActs whose EMEVD we kicked since the load.
    kicked_keys: HashSet<usize>,
    /// ObjActs sent lately (`objact_key`, when).
    sent_keys: Vec<(usize, Instant)>,
    journal: Journal,
    /// Infos (address) the journal judged since their load.
    journal_tried: HashSet<usize>,
    journal_at: Option<Instant>,
    /// Map flag writes of the last `FLAG_WINDOW` (when, id, value).
    flags: VecDeque<(Instant, u32, bool)>,
    flag_window_until: Option<Instant>,
    last_own_anim: i32,
    own_objact: Option<(i32, Instant)>,
    seq: u32,
    received: u32,
    replayed: u32,
    kicks: u32,
    dropped: u32,
    hook_dropped_logged: u32,
    dumps: u32,
    logged_blocks: bool,
    logged_cap: bool,
    logged_prologues: bool,
    logged_sender: bool,
    /// `0x6BA7B0` calls with another actor logged since the load (without `objact_debug`).
    foreign_logged: u32,
    /// doors.rs replays of infos without an ObjActEntityID already logged (flag or address).
    no_entity_logged: HashSet<usize>,
}

static STATE: Mutex<Option<State>> = Mutex::new(None);

fn with<R>(f: impl FnOnce(&mut State) -> R) -> R {
    let mut g = STATE.lock().unwrap_or_else(|e| e.into_inner());
    f(g.get_or_insert_with(State::default))
}

fn mark(s: &mut State, key: usize, now: Instant) {
    s.replays.retain(|(_, t)| now.duration_since(*t) < REPLAY_MARK_FOR);
    if s.replays.len() < 64 {
        s.replays.push((key, now));
    }
}

fn replay_marked(s: &State, key: usize, now: Instant) -> bool {
    s.replays.iter().any(|&(k, t)| k == key && now.duration_since(t) < REPLAY_MARK_FOR)
}

/// The info address and `objact_key` of info `index` of `block` (doors.rs passes both).
fn info_in_game(block: usize, index: i32) -> Option<(usize, usize)> {
    let infos = game::read_ptr(block + BLOCK_INFOS)?;
    let addr = infos + INFO_STRIDE * usize::try_from(index).ok()?;
    Some((addr, key_of(addr, game::read_ptr(addr + INFO_PRIMARY).unwrap_or(0))))
}

/// doors.rs, right before its own `0x6BA910(block, index, ...)`: that execution is a
/// replay, not ours (no echo to the friend), and no second replay of ours follows.
pub fn note_replay(block: usize, index: i32) {
    let Some((_, key)) = info_in_game(block, index) else { return };
    let now = Instant::now();
    with(|s| mark(s, key, now));
}

/// doors.rs, before its `0x6BA910`: we replayed that ObjAct a moment ago (the friend's 0xA7).
pub fn replayed_recently(block: usize, index: i32) -> bool {
    let Some((_, key)) = info_in_game(block, index) else { return false };
    let now = Instant::now();
    with(|s| replay_marked(s, key, now))
}

/// doors.rs, right after its `0x6BA910(block, index, ...)`: the EMEVD kick the replay lacks
/// (only while `sync_objact` is on; logged as an `objact:` line).
pub fn kick_after_replay(block: usize, index: i32, who: &str) {
    if MODE.load(Ordering::Relaxed) == 0 {
        return;
    }
    let Some((addr, key)) = info_in_game(block, index) else { return };
    let run = game::read::<u32>(addr + INFO_RUN_STATE);
    let entity = game::read::<i32>(addr + INFO_ENTITY);
    let debug = DEBUG.load(Ordering::Relaxed);
    let text = with(|s| {
        // No ObjActEntityID (the meditation room windows, replayed by doors.rs after every
        // load and rest): nothing to kick, said once per object (by its flag) and session.
        if entity == Some(0) && !debug {
            let flag = game::read_ptr(addr + INFO_STATE).and_then(|st| game::read::<u32>(st + STATE_FLAG));
            let id = flag.filter(|&f| f != 0 && f != u32::MAX).map_or(addr, |f| f as usize);
            if s.no_entity_logged.len() >= MAX_NO_ENTITY_LOGGED || !s.no_entity_logged.insert(id) {
                return None;
            }
            return Some("EMEVD not kicked: no ObjActEntityID (no EMEVD can wait for it; said once per object)".to_owned());
        }
        Some(if run == Some(RUN_EXECUTING) {
            kick_text(s, addr, key)
        } else {
            format!("EMEVD not kicked: run state {run:?} after the replay (it did not start)")
        })
    });
    if let Some(text) = text {
        log::line(&format!("objact: {who}'s replay (block {block:#x}, info index {index}): {text}"));
    }
}

/// world.rs, a new session: the journal of the last one no longer counts.
pub fn new_session() {
    let n = with(|s| {
        s.incoming.clear();
        s.no_entity_logged.clear();
        s.journal.dropped = 0;
        s.journal.clear()
    });
    if n > 0 {
        log::line(&format!("objact: new session: the journal of {n} executed ObjActs of the last one is cleared"));
    }
}

/// world.rs, every flag write the hook ring shows (ours, the friend's applied ones, scripts').
pub fn flag_write(id: u32, on: bool, bank: u8) {
    if bank != 0 || !logged_flag(id) {
        return;
    }
    let debug = DEBUG.load(Ordering::Relaxed);
    if MODE.load(Ordering::Relaxed) == 0 && !debug {
        return;
    }
    let now = Instant::now();
    with(|s| {
        while s.flags.front().is_some_and(|(t, ..)| now.duration_since(*t) > FLAG_WINDOW) || s.flags.len() >= MAX_FLAGS {
            s.flags.pop_front();
        }
        s.flags.push_back((now, id, on));
        if debug {
            log::line(&format!("objact: flag {id} = {on}"));
        } else if s.flag_window_until.is_some_and(|t| now <= t) {
            log::line(&format!("objact: flag {id} = {on} (within {} s after our ObjAct)", FLAG_WINDOW.as_secs()));
        }
    });
}

// ------------------------------------------------------------------ the 0x6BA7B0 detour

static HOOK_TRAMP: AtomicUsize = AtomicUsize::new(0);

/// One call of `0x6BA7B0` (game thread), handed to the frame task.
#[derive(Clone, Copy)]
struct OwnStart {
    block: usize,
    index: i32,
    actor: i32,
    run_before: u32,
    flag_before: Option<bool>,
    caller: usize,
    at: Instant,
}

struct HookQueue {
    events: Vec<OwnStart>,
    dropped: u32,
}

static HOOK_QUEUE: Mutex<HookQueue> = Mutex::new(HookQueue { events: Vec::new(), dropped: 0 });

/// Installs the `0x6BA7B0` detour (from `init`, at the title screen). Not installed = the poll
/// sender, whatever `objact_sender` says.
pub fn install() {
    let d: extern "C" fn(usize, i32, i32) -> u64 = own_start_detour;
    match hook::install_detour(OWN_START_RVA, &OWN_START_PROLOGUE, d as usize, "ObjAct local start (0x6BA7B0)") {
        Some(t) => {
            HOOK_TRAMP.store(t, Ordering::Release);
            log::line("objact: 0x6BA7B0 hooked: our ObjAct executions are sent from the hook (unless objact_sender = poll)");
        }
        None => log::line("objact: 0x6BA7B0 NOT hooked (its bytes are not Sekiro 1.06's, or no memory): our ObjAct executions are found by the poll instead"),
    }
}

/// Info `index` before the local start: run state, the flag value (flag classes) and, for a
/// one-loop door, its flag id.
struct PreStart {
    run: u32,
    flag_before: Option<bool>,
    loop_door_flag: Option<u32>,
}

fn pre_start(block: usize, index: i32) -> Option<PreStart> {
    let mem = GameMem;
    let n = mem.i32(block + BLOCK_INFO_COUNT)?;
    if index < 0 || index >= n {
        return None;
    }
    let addr = mem.ptr(block + BLOCK_INFOS)? + INFO_STRIDE * index as usize;
    let run = mem.u32(addr + INFO_RUN_STATE)?;
    let state = mem.ptr(addr + INFO_STATE);
    let class = state.and_then(|st| class_of(&mem, game::module_base(), st));
    let flag = state.zip(class).and_then(|(st, c)| flag_of(&mem, st, c));
    let param = mem.i32(addr + INFO_PARAM_ID).unwrap_or(-1);
    Some(PreStart { run, flag_before: flag.and_then(game::event_flag), loop_door_flag: flag.filter(|_| loop_door(class, param)) })
}

/// Guard 5 (doors.rs): our player's start switched a one-loop door's flag from ON to OFF.
fn own_loop_toggle_off(actor: i32, own: Option<i32>, before: Option<bool>, after: Option<bool>) -> bool {
    own == Some(actor) && before == Some(true) && after == Some(false)
}

/// `0x6BA7B0(block, index, actor)`: the original runs unchanged; what it was called on goes
/// to the frame task. Nothing is sent or logged here.
extern "C" fn own_start_detour(block: usize, index: i32, actor: i32) -> u64 {
    let t = HOOK_TRAMP.load(Ordering::Acquire);
    if t == 0 {
        // Hooked, trampoline not stored yet (microseconds at install, at the title screen).
        return 0;
    }
    let caller = hook::detour_caller(t);
    let pre = pre_start(block, index);
    let original: extern "C" fn(usize, i32, i32) -> u64 = unsafe { std::mem::transmute(t) };
    let ret = original(block, index, actor);
    if let Some(PreStart { run: run_before, flag_before, loop_door_flag }) = pre {
        // Noted here, before world.rs's flag ring sees the toggle's write (same frame).
        if let Some(f) = loop_door_flag {
            let own = game::player().and_then(|p| game::read::<i32>(p.0 + CHR_HANDLE));
            if own_loop_toggle_off(actor, own, flag_before, game::event_flag(f)) {
                doors::note_own_loop_toggle(f);
            }
        }
        let mut q = HOOK_QUEUE.lock().unwrap_or_else(|e| e.into_inner());
        if q.events.len() < MAX_HOOK_EVENTS {
            q.events.push(OwnStart { block, index, actor, run_before, flag_before, caller, at: Instant::now() });
        } else {
            q.dropped += 1;
        }
    }
    ret
}

fn take_own_starts() -> (Vec<OwnStart>, u32) {
    let mut q = HOOK_QUEUE.lock().unwrap_or_else(|e| e.into_inner());
    (std::mem::take(&mut q.events), q.dropped)
}

// ------------------------------------------------------------------ the kick

/// 0 not checked, 1 `0x6B2AF0` holds the expected bytes, 2 it does not (never called).
static KICK_CODE: AtomicU8 = AtomicU8::new(0);

fn kick_code_ok(base: usize) -> bool {
    match KICK_CODE.load(Ordering::Relaxed) {
        1 => true,
        2 => false,
        _ => {
            let mut b = [0u8; KICK_PROLOGUE.len()];
            let ok = game::read_bytes(base + KICK_RVA, &mut b) && b == KICK_PROLOGUE;
            KICK_CODE.store(if ok { 1 } else { 2 }, Ordering::Relaxed);
            if !ok {
                log::line(&format!("objact: 0x6B2AF0 has unexpected bytes {} (not Sekiro 1.06?): replays are never kicked", hex(&b)));
            }
            ok
        }
    }
}

/// The EMEVD kick of info `info` after its replay (game thread): `info+0x70 = 1;
/// 0x6B2AF0(info)`, so that `ObjActEventFlag(ObjActEntityID)` holds as after the opener's
/// own start. Ok = the ObjActEntityID kicked.
fn kick(info: usize) -> Result<i32, &'static str> {
    let base = game::module_base();
    let entity = game::read::<i32>(info + INFO_ENTITY).ok_or("the info is unreadable")?;
    let sync = game::read_ptr(info + INFO_PARAM_ROW).and_then(|r| game::read::<u8>(r + PARAM_KICK_SYNC));
    let pending = game::read::<u8>(info + INFO_KICK).unwrap_or(1);
    kick_plan(entity, sync, pending)?;
    if !kick_code_ok(base) {
        return Err("0x6B2AF0 has unexpected bytes");
    }
    if game::read_ptr(base + EVENT_MAN).and_then(|m| game::read_ptr(m + EVENT_MAN_OBJACT_LIST)).is_none() {
        return Err("the event manager or its ObjAct list is not there");
    }
    if !game::write::<u8>(info + INFO_KICK, 1) {
        return Err("info+0x70 is not writable");
    }
    let f: extern "C" fn(usize) = unsafe { std::mem::transmute(base + KICK_RVA) };
    f(info);
    Ok(entity)
}

fn kick_text(s: &mut State, info: usize, key: usize) -> String {
    match kick(info) {
        Ok(entity) => {
            s.kicked_keys.insert(key);
            s.kicks += 1;
            format!("EMEVD kicked: ObjActEventFlag({entity}) holds as after the opener's own start ({} kicks so far)", s.kicks)
        }
        Err(why) => format!("EMEVD not kicked: {why}"),
    }
}

/// `0x6BA910(block, index, -1, state)` on info `i`, then its kick (when it started).
fn replay_and_kick(s: &mut State, base: usize, b: &Block, i: &Info, state: u32, now: Instant) -> String {
    let key = i.objact_key();
    mark(s, key, now);
    s.replayed_keys.insert(key);
    let replay: extern "C" fn(usize, i32, i32, u32) -> u8 = unsafe { std::mem::transmute(base + REPLAY_RVA) };
    let al = replay(b.addr, i.index, REPLAY_ACTOR, state);
    s.replayed += 1;
    let run = game::read::<u32>(i.addr + INFO_RUN_STATE);
    let kicked = if run == Some(RUN_EXECUTING) {
        kick_text(s, i.addr, key)
    } else {
        format!("EMEVD not kicked: run state {run:?} after the replay (it did not start)")
    };
    format!(
        "replayed with 0x6BA910(block, {}, actor {REPLAY_ACTOR}, {state}) (al {al}, {} so far), run state now {}; {kicked}",
        i.index,
        s.replayed,
        run.map_or("?".to_owned(), |r| r.to_string())
    )
}

// ------------------------------------------------------------------ frame

/// world.rs, every frame while connected (game thread). `ready`: settled after the load and
/// the flag manager readable (real world bank).
pub fn tick(net: &mut Net, in_world_for: Duration, ready: bool) {
    let now = Instant::now();
    let mode = MODE.load(Ordering::Relaxed);
    let debug = DEBUG.load(Ordering::Relaxed);
    let hooked = HOOK_TRAMP.load(Ordering::Acquire) != 0;
    let by_hook = hooked && SENDER.load(Ordering::Relaxed) != SENDER_POLL;
    // 0. The detour's calls: always taken, used only by the hook sender in the world.
    let (starts, hook_dropped) = take_own_starts();
    // 1. The peers' executions: always read, so nothing stays in the inbox when off. A
    //    client's transfer reaches the other clients through coop-net's host relay.
    while let Some((from, d)) = net.take_bulk_from(BULK_OBJACT) {
        match decode(&d) {
            Ok(m) => with(|s| receive(s, from, m, mode, now)),
            Err(why) => log::line(&format!("objact: message from peer {} ({} bytes) ignored: {why}", from.slot(), d.len())),
        }
    }
    // Never in a Remnant memory (other flag bank, other objects).
    if !ready || game::remnant() || (mode == 0 && !debug) {
        return;
    }
    let base = game::module_base();
    let out = with(|s| {
        if in_world_for < s.last_in_world {
            // A loading screen: the infos are new, our marks and edges are stale.
            s.seen.clear();
            s.edges.clear();
            s.looks.clear();
            s.replays.clear();
            s.replayed_keys.clear();
            s.executed_keys.clear();
            s.own_keys.clear();
            s.kicked_keys.clear();
            s.journal_tried.clear();
            s.journal_at = None;
            s.sent_keys.clear();
            s.own_objact = None;
            s.dumps = 0;
            s.foreign_logged = 0;
            s.logged_blocks = false;
        }
        s.last_in_world = in_world_for;
        if !s.logged_sender {
            s.logged_sender = true;
            let how = match (SENDER.load(Ordering::Relaxed), hooked) {
                (SENDER_HOOK, true) => "hook (the 0x6BA7B0 detour)",
                (SENDER_HOOK, false) => "poll (objact_sender = hook, but 0x6BA7B0 is not hooked)",
                _ => "poll (objact_sender = poll)",
            };
            log::line(&format!("objact: sender: {how}; sync_objact = {mode}; journal: {} ObjActs", s.journal.entries.len()));
        }
        if hook_dropped > s.hook_dropped_logged {
            s.hook_dropped_logged = hook_dropped;
            log::line(&format!("objact: {hook_dropped} calls of 0x6BA7B0 dropped so far ({MAX_HOOK_EVENTS} were waiting)"));
        }
        if debug && !s.logged_prologues {
            s.logged_prologues = true;
            for (rva, name) in [(0x6BC1B0usize, "ObjAct execution 0x6BC1B0"), (OWN_START_RVA, "own ObjAct 0x6BA7B0"), (REPLAY_RVA, "replay 0x6BA910"), (KICK_RVA, "kick 0x6B2AF0")] {
                let mut b = [0u8; 24];
                let text = if game::read_bytes(base + rva, &mut b) { hex(&b) } else { "unreadable".to_owned() };
                log::line(&format!("objact: {name} first bytes {text}"));
            }
        }
        if !by_hook {
            own_anim(s, now);
        }
        if s.last_poll.is_none_or(|t| now.duration_since(t) >= POLL_EVERY) {
            s.last_poll = Some(now);
            poll(s, base, now, debug, by_hook);
        }
        let mut out = decide(s, now, mode, debug, by_hook);
        if by_hook {
            for ev in starts {
                if let Some(m) = own_start(s, base, ev, mode, debug, now) {
                    out.push(m);
                }
            }
        }
        out
    });
    for m in out {
        // Host: to every client; client: to the host, which relays it to the other clients.
        let ok = net.send_bulk_to(Dest::All, BULK_OBJACT, &encode(&m));
        log::line(&format!(
            "objact: sent our ObjAct #{} (map {}, ObjActEntityID {}, info index {}, sub {}, ObjActParam {}, state {}): ok = {ok}",
            m.seq,
            map_name(Some(m.map)),
            m.entity,
            m.index,
            m.sub,
            m.param,
            m.state
        ));
    }
    with(|s| {
        apply(s, base, now, mode);
        journal_check(s, base, now, mode, debug);
        run_looks(s, now);
    });
}

/// Poll sender: our own player's newest animation, evidence for one send.
fn own_anim(s: &mut State, now: Instant) {
    let Some(p) = game::player() else { return };
    let a = crate::anim::Probe::default().current(p).unwrap_or(0);
    if a == s.last_own_anim {
        return;
    }
    s.last_own_anim = a;
    if doors::objact_anims().is_some_and(|set| doors::is_objact_anim(a, set)) {
        s.own_objact = Some((a, now));
    }
}

/// One walk: run state edges -> `edges`; once per load the loaded blocks.
fn poll(s: &mut State, base: usize, now: Instant, debug: bool, by_hook: bool) {
    let mem = GameMem;
    let all = blocks(&mem, base);
    if !s.logged_blocks && !all.is_empty() {
        s.logged_blocks = true;
        let list: Vec<String> = all.iter().map(|b| format!("slot {} {} ({} infos)", b.slot, map_name(b.map), b.count)).collect();
        log::line(&format!("objact: loaded ObjAct blocks: {}", list.join(", ")));
    }
    let mut buf = std::mem::take(&mut s.buf);
    let mut alive = std::mem::take(&mut s.alive);
    alive.clear();
    let mut new = Vec::new();
    let mut budget = MAX_POLL_INFOS;
    for b in &all {
        if b.count > budget {
            if !s.logged_cap {
                s.logged_cap = true;
                log::line(&format!("objact: more than {MAX_POLL_INFOS} ObjAct infos loaded: block slot {} and later ones not watched", b.slot));
            }
            continue;
        }
        budget -= b.count;
        for i in infos(&mem, b, &mut buf) {
            alive.insert(i.addr);
            let prev = s.seen.get(&i.addr).copied();
            // Idle = the run state when first seen, or the first one after an execution seen at the load.
            let idle = match prev {
                Some(p) if p.idle == RUN_EXECUTING && i.run != RUN_EXECUTING => i.run,
                Some(p) => p.idle,
                None => i.run,
            };
            let first = prev.map_or(now, |p| p.first);
            s.seen.insert(i.addr, Seen { run: i.run, b114: i.b114, idle, first });
            if let Some(p) = prev
                && is_edge(p.run, i.run)
            {
                new.push((*b, i, p.b114));
            }
        }
    }
    s.seen.retain(|a, _| alive.contains(a));
    s.journal_tried.retain(|a| alive.contains(a));
    s.buf = buf;
    s.alive = alive;
    for (b, i, prev_b114) in new {
        let key = i.objact_key();
        s.executed_keys.insert(key);
        let class = class_of(&mem, base, i.state);
        let replay = replay_marked(s, key, now);
        let flag = class.and_then(|c| flag_of(&mem, i.state, c));
        let flag_at_edge = flag.and_then(game::event_flag);
        // The object only for a known class (a foreign state must not reach 0x6BCBC0), and
        // only for an execution the poll sender may send (or when debugging).
        let start = if class.is_some() && (debug || (!replay && !by_hook)) { doors::object_view(b.addr, b.slot, i.index, i.handle, 1) } else { None };
        if s.edges.len() < 32 {
            s.edges.push(Edge { at: now, block: b, info: i, prev_b114, class, flag, flag_at_edge, start, replay });
        }
    }
}

/// After one of our executions went out: the map flags around it and a look 1 s later.
fn watch_after_send(s: &mut State, now: Instant, at: Instant, b: &Block, i: &Info, known: bool, flag: Option<u32>, state: u8, debug: bool) {
    if !debug && s.flag_window_until.is_none_or(|t| now > t) {
        let before: Vec<String> =
            s.flags.iter().filter(|&&(t, ..)| t + FLAG_WINDOW >= at).map(|&(t, id, on)| format!("{id} = {on} ({:+} ms)", signed_ms(t, at))).collect();
        log::line(&format!("objact: map flags written from {} s before it until now: [{}]", FLAG_WINDOW.as_secs(), before.join(", ")));
    }
    s.flag_window_until = Some(now + FLAG_WINDOW);
    if s.looks.len() < 64 {
        s.looks.push(Look {
            due: at + SENDER_LOOK,
            what: format!("{} s after our ObjAct (map {}, ObjActEntityID {}, info index {})", SENDER_LOOK.as_secs(), map_name(b.map), i.entity, i.index),
            block: b.addr,
            infos: b.infos,
            map: b.map,
            slot: b.slot,
            index: i.index,
            handle: i.handle,
            known,
            flag,
            state: u32::from(state),
        });
    }
}

/// Journals one execution (ours or a peer's); the result for the log.
fn journal_note(s: &mut State, e: Entry) -> String {
    match s.journal.note(e) {
        Ok(what) => format!("{what} ({} in the journal)", s.journal.entries.len()),
        Err(why) => format!("not journaled: {why}"),
    }
}

/// Hook sender: one call of `0x6BA7B0` -> a message when it is our player's and it started.
fn own_start(s: &mut State, base: usize, ev: OwnStart, mode: u8, debug: bool, now: Instant) -> Option<Msg> {
    let mem = GameMem;
    let Some(b) = blocks(&mem, base).into_iter().find(|b| b.addr == ev.block) else {
        if debug {
            log::line(&format!("objact: 0x6BA7B0 on block {:#x} index {}: the block is not a loaded WorldObjActMan block", ev.block, ev.index));
        }
        return None;
    };
    let Some(i) = info_at(&mem, &b, ev.index) else {
        log::line(&format!("objact: 0x6BA7B0 on block slot {} index {}: the info is unreadable", b.slot, ev.index));
        return None;
    };
    let class = class_of(&mem, base, i.state);
    let own = game::player().and_then(|p| game::read::<i32>(p.0 + CHR_HANDLE));
    let started = i.run == RUN_EXECUTING;
    let key = i.objact_key();
    let verdict = hook_decision(class, i.param, mode, ev.actor, own, started);
    // Another character's execution (NPC, script): the first few lines per load show whether
    // our handle (chr+0x08) is read right; the rest only when debugging.
    if !debug && own != Some(ev.actor) {
        if s.foreign_logged >= MAX_FOREIGN_LOGS {
            return None;
        }
        s.foreign_logged += 1;
    }
    let state = target_state(class, i.param, ev.flag_before);
    let mut msg = None;
    let decision: Result<u32, &str> = match verdict {
        Err(why) => Err(why),
        Ok(_) if sent_lately(&s.sent_keys, key, now) => Err("another info of the same ObjAct (a door sub info) was sent just now"),
        Ok(_) => match b.map {
            None => Err("the block's map id is unreadable (descriptor +0x08 / area check)"),
            Some(map) => {
                s.seq = s.seq.wrapping_add(1);
                s.sent_keys.retain(|&(_, t)| now.duration_since(t) < SAME_OBJACT_FOR);
                s.sent_keys.push((key, now));
                msg = Some(Msg { map, entity: i.entity, index: i.index, sub: i.sub, param: i.param, class: class_code(class), state, seq: s.seq });
                Ok(s.seq)
            }
        },
    };
    let journaled = if verdict.is_ok() {
        s.own_keys.insert(key);
        s.executed_keys.insert(key);
        match b.map {
            Some(map) => journal_note(
                s,
                Entry { map, entity: i.entity, index: i.index, sub: i.sub, param: i.param, class: class_code(class), state, ours: true },
            ),
            None => "not journaled: no map id".to_owned(),
        }
    } else {
        "not journaled".to_owned()
    };
    let flag = class.and_then(|c| flag_of(&mem, i.state, c));
    let fmt_flag = |v: Option<bool>| v.map_or("?".to_owned(), |b| b.to_string());
    let door_note = match (loop_door(class, i.param), ev.flag_before) {
        (true, Some(true)) => "; one-loop door whose flag was on already (the friend's flag came, our object had not moved): doors.rs guard 5 puts the toggled flag back on",
        (true, _) => "; one-loop door",
        _ => "",
    };
    log::line(&format!(
        "objact: we used ObjAct: map {} (block slot {}, info index {}, sub {}), ObjActEntityID {}, ObjActParam {}, {} state, flag {} before {} now {}, run state {} -> {}, actor {} (our player {}), caller {}; {}; {journaled}{door_note}",
        map_name(b.map),
        b.slot,
        i.index,
        i.sub,
        i.entity,
        i.param,
        class_name(class_code(class)),
        flag.map_or("none".to_owned(), |f| f.to_string()),
        fmt_flag(ev.flag_before),
        fmt_flag(flag.and_then(game::event_flag)),
        ev.run_before,
        i.run,
        ev.actor,
        own.map_or("?".to_owned(), |h| h.to_string()),
        if ev.caller == 0 { "?".to_owned() } else { format!("sekiro+{:#x}", ev.caller.wrapping_sub(base)) },
        match (verdict, decision) {
            (Ok(why), Ok(seq)) => format!("{why}: sending as #{seq} (target state {state})"),
            (Ok(why), Err(not)) => format!("{why}, but not sent: {not}"),
            (Err(why), _) => format!("not sent: {why}"),
        }
    ));
    if debug && s.dumps < MAX_INFO_DUMPS {
        let mut raw = [0u8; INFO_STRIDE];
        if game::read_bytes(i.addr, &mut raw) {
            s.dumps += 1;
            log::line(&format!("objact: info {:#x} (index {}) bytes: {}", i.addr, i.index, hex(&raw)));
        }
    }
    if msg.is_some() {
        watch_after_send(s, now, ev.at, &b, &i, class.is_some(), flag, state, debug);
    }
    msg
}

/// Poll: executions `DECIDE_AFTER` old. Poll sender: ours -> messages (sent by the caller),
/// logged. Hook sender: logged with `objact_debug` only, never sent from here.
fn decide(s: &mut State, now: Instant, mode: u8, debug: bool, by_hook: bool) -> Vec<Msg> {
    let mut out = Vec::new();
    let mut due = Vec::new();
    s.edges.retain(|e| {
        if now.duration_since(e.at) < DECIDE_AFTER {
            return true;
        }
        due.push(*e);
        false
    });
    s.sent_keys.retain(|&(_, t)| now.duration_since(t) < SAME_OBJACT_FOR);
    for e in due {
        let b114_now = game::read::<u8>(e.info.addr + INFO_B114).unwrap_or(e.info.b114);
        if by_hook {
            if debug {
                log::line(&format!(
                    "objact: ObjAct ran here (watch): map {} (block slot {}, info index {}), ObjActEntityID {}, ObjActParam {}, {} state, info+0x71/+0x72 = {}/{}{}",
                    map_name(e.block.map),
                    e.block.slot,
                    e.info.index,
                    e.info.entity,
                    e.info.param,
                    class_name(class_code(e.class)),
                    e.info.b113,
                    b114_now,
                    if e.replay { ", our / doors.rs replay" } else { "" }
                ));
            }
            continue;
        }
        let own = s.own_objact.filter(|&(_, t)| near(t, e.at, OWN_ANIM_NEAR));
        let ev = Evidence { our_replay: e.replay, b114_rose: e.prev_b114 != 1 && b114_now == 1, b114_now: b114_now == 1, own_anim: own.is_some() };
        let verdict = send_decision(ev, e.class, e.info.param, mode);
        if !debug && !ev.our_replay && !ev.b114_now && !ev.own_anim {
            continue;
        }
        if !debug && ev.our_replay {
            continue;
        }
        let flag_now = e.flag.and_then(game::event_flag);
        // The toggle's write (if any) is in the ring around the edge: its old value is "before";
        // else the value at the edge (the toggle may come later).
        let flag_before = e
            .flag
            .and_then(|f| s.flags.iter().rev().find(|&&(t, id, _)| id == f && t + Duration::from_millis(500) >= e.at).map(|&(_, _, on)| !on))
            .or(e.flag_at_edge);
        let state = target_state(e.class, e.info.param, flag_before);
        let key = e.info.objact_key();
        let decision: Result<u32, &str> = match verdict {
            Err(why) => Err(why),
            Ok(_) if mode == 0 => Err("sync_objact = 0"),
            Ok(_) if sent_lately(&s.sent_keys, key, now) => Err("another info of the same ObjAct (a door sub info) was sent just now"),
            Ok(_) => match e.block.map {
                None => Err("the block's map id is unreadable (descriptor +0x08 / area check)"),
                Some(map) => {
                    s.seq = s.seq.wrapping_add(1);
                    s.sent_keys.push((key, now));
                    // The animation is evidence for one execution only.
                    s.own_objact = None;
                    out.push(Msg {
                        map,
                        entity: e.info.entity,
                        index: e.info.index,
                        sub: e.info.sub,
                        param: e.info.param,
                        class: class_code(e.class),
                        state,
                        seq: s.seq,
                    });
                    Ok(s.seq)
                }
            },
        };
        let journaled = match (decision, e.block.map) {
            (Ok(_), Some(map)) => journal_note(
                s,
                Entry {
                    map,
                    entity: e.info.entity,
                    index: e.info.index,
                    sub: e.info.sub,
                    param: e.info.param,
                    class: class_code(e.class),
                    state,
                    ours: true,
                },
            ),
            _ => "not journaled".to_owned(),
        };
        let start = e.start.map_or("? (not looked at)".to_owned(), |v| format!("{:#x} anim {} (playing {}, the open execution plays {})", v.obj, v.anim, v.playing, v.want));
        let anim = own.map_or("none within 2 s".to_owned(), |(a, t)| format!("{a} ({:+} ms)", signed_ms(t, e.at)));
        let fmt_flag = |v: Option<bool>| v.map_or("?".to_owned(), |b| b.to_string());
        log::line(&format!(
            "objact: ObjAct ran here: map {} (block slot {}, info index {}, sub {}), ObjActEntityID {}, ObjActParam {}, {} state, flag {} before {} now {}, info+0x71/+0x72 = {}/{} (+0x72 before the edge {}), object handle {:#x}, object at start {start}, our ObjAct animation {anim}; {}; {journaled}",
            map_name(e.block.map),
            e.block.slot,
            e.info.index,
            e.info.sub,
            e.info.entity,
            e.info.param,
            class_name(class_code(e.class)),
            e.flag.map_or("none".to_owned(), |f| f.to_string()),
            fmt_flag(flag_before),
            fmt_flag(flag_now),
            e.info.b113,
            b114_now,
            e.prev_b114,
            e.info.handle,
            match (verdict, decision) {
                (Ok(why), Ok(seq)) => format!("{why}: sending as #{seq} (target state {state})"),
                (Ok(why), Err(not)) => format!("{why}, but not sent: {not}"),
                (Err(why), _) => format!("not sent: {why}"),
            }
        ));
        if debug && s.dumps < MAX_INFO_DUMPS {
            let mut raw = [0u8; INFO_STRIDE];
            if game::read_bytes(e.info.addr, &mut raw) {
                s.dumps += 1;
                log::line(&format!("objact: info {:#x} (index {}) bytes: {}", e.info.addr, e.info.index, hex(&raw)));
            }
        }
        if decision.is_ok() {
            watch_after_send(s, now, e.at, &e.block, &e.info, e.class.is_some(), e.flag, state, debug);
        }
    }
    out
}

fn receive(s: &mut State, from: PeerId, m: Msg, mode: u8, now: Instant) {
    s.received += 1;
    let queued = mode != 0 && s.incoming.len() < MAX_INCOMING;
    if mode != 0 && !queued {
        s.dropped += 1;
    }
    let journaled = if mode == 0 {
        String::new()
    } else {
        format!(
            "; {}",
            journal_note(
                s,
                Entry { map: m.map, entity: m.entity, index: m.index, sub: m.sub, param: m.param, class: m.class, state: m.state, ours: false },
            )
        )
    };
    log::line(&format!(
        "objact: peer {} ran ObjAct #{}: map {}, ObjActEntityID {}, info index {}, sub {}, ObjActParam {}, {} state, target state {} (#{} received){}{journaled}",
        from.slot(),
        m.seq,
        map_name(Some(m.map)),
        m.entity,
        m.index,
        m.sub,
        m.param,
        class_name(m.class),
        m.state,
        s.received,
        if mode == 0 {
            ": ignored, sync_objact = 0 here".to_owned()
        } else if !queued {
            format!(": dropped, {MAX_INCOMING} are waiting already ({} dropped so far)", s.dropped)
        } else {
            String::new()
        }
    ));
    if queued {
        s.incoming.push(Incoming { from, msg: m, at: now, next_try: now + APPLY_AFTER });
    }
}

/// Receiver facts about info `i` (key `key`) now.
fn here_of(s: &State, i: &Info, now: Instant) -> Here {
    let key = i.objact_key();
    Here {
        executing: i.run == RUN_EXECUTING,
        idle: s.seen.get(&i.addr).map(|x| x.idle == i.run),
        marked: replay_marked(s, key, now),
        replayed: s.replayed_keys.contains(&key),
        executed: s.executed_keys.contains(&key),
        local_start: i.b114 == 1,
        kicked: s.kicked_keys.contains(&key),
        ours: s.own_keys.contains(&key),
    }
}

/// The peers' executions that are due: replayed (and kicked) here when loaded, idle and
/// still closed; only kicked when the game replayed them already.
fn apply(s: &mut State, base: usize, now: Instant, mode: u8) {
    let mut due = Vec::new();
    s.incoming.retain(|x| {
        if now < x.next_try {
            return true;
        }
        due.push(*x);
        false
    });
    let mem = GameMem;
    for mut x in due {
        let m = x.msg;
        let (b, i, how) = match find(&mem, base, m.map, m.entity, m.index, m.sub) {
            Ok(found) => found,
            Err(why) => {
                if now.duration_since(x.at) < APPLY_WITHIN {
                    x.next_try = now + RETRY_EVERY;
                    s.incoming.push(x);
                } else {
                    log::line(&format!(
                        "objact: peer {}'s ObjAct #{} (map {}, ObjActEntityID {}, info index {}, sub {}): not replayed within {} s: {why} (the journal replays it when that block is loaded and the object is closed)",
                        x.from.slot(),
                        m.seq,
                        map_name(Some(m.map)),
                        m.entity,
                        m.index,
                        m.sub,
                        APPLY_WITHIN.as_secs()
                    ));
                }
                continue;
            }
        };
        let class = class_of(&mem, base, i.state);
        let key = i.objact_key();
        let here = here_of(s, &i, now);
        let state = u32::from(m.state);
        let plan = receive_plan(class, i.param, m.state, mode, here);
        // 0x6BCBC0 only for a known class and a valid state (the plan checks both).
        let view = if plan == Ok(Plan::Replay) { doors::object_view(b.addr, b.slot, i.index, i.handle, state) } else { None };
        let what = match plan {
            Ok(Plan::Replay) => match object_decision(view.map(|v| v.reached())) {
                Ok(()) => replay_and_kick(s, base, &b, &i, state, now),
                Err(why) => format!("not replayed: {why}"),
            },
            Ok(Plan::KickOnly) => {
                format!("it ran here already as a replay without its EMEVD kick (the game's type 45?): {}", kick_text(s, i.addr, key))
            }
            Err(why) => format!("not replayed: {why}"),
        };
        log::line(&format!(
            "objact: peer {}'s ObjAct #{} here: block slot {}, info index {} ({}), {} state{}, run state {} (idle {:?}), info+0x71/+0x72 = {}/{}, object {}; {what}",
            x.from.slot(),
            m.seq,
            b.slot,
            i.index,
            how_text(how, m.entity),
            class_name(class_code(class)),
            if class_code(class) == m.class { String::new() } else { format!(" (theirs: {})", class_name(m.class)) },
            i.run,
            here.idle,
            i.b113,
            i.b114,
            view.map_or("not looked at".to_owned(), |v| format!("{:#x} anim {} (playing {}, the execution plays {})", v.obj, v.anim, v.playing, v.want)),
        ));
        let flag = class.and_then(|c| flag_of(&mem, i.state, c));
        for after in RECEIVER_LOOKS {
            if s.looks.len() < 64 {
                s.looks.push(Look {
                    due: x.at + after,
                    what: format!("{} s after peer {}'s ObjAct #{} arrived (info index {})", after.as_secs(), x.from.slot(), m.seq, i.index),
                    block: b.addr,
                    infos: b.infos,
                    map: b.map,
                    slot: b.slot,
                    index: i.index,
                    handle: i.handle,
                    known: class.is_some() && m.state <= 1,
                    flag,
                    state,
                });
            }
        }
    }
}

/// The journal against the loaded infos: a journaled ObjAct whose info settled after its
/// load, did not run since and whose object is not at that animation is replayed and kicked.
fn journal_check(s: &mut State, base: usize, now: Instant, mode: u8, debug: bool) {
    if mode == 0 || s.journal.entries.is_empty() || s.journal_at.is_some_and(|t| now.duration_since(t) < JOURNAL_EVERY) {
        return;
    }
    s.journal_at = Some(now);
    let mem = GameMem;
    let entries = s.journal.entries.clone();
    for e in entries {
        let Ok((b, i, how)) = find(&mem, base, e.map, e.entity, e.index, e.sub) else { continue };
        let seen_for = s.seen.get(&i.addr).map(|x| now.duration_since(x.first));
        if journal_gate(seen_for, s.journal_tried.contains(&i.addr)).is_err() {
            continue;
        }
        s.journal_tried.insert(i.addr);
        let class = class_of(&mem, base, i.state);
        let here = here_of(s, &i, now);
        let who = if e.ours { "our" } else { "a peer's" };
        let what = match receive_plan(class, i.param, e.state, mode, here) {
            Ok(Plan::Replay) => {
                let view = doors::object_view(b.addr, b.slot, i.index, i.handle, u32::from(e.state));
                match object_decision(view.map(|v| v.reached())) {
                    Ok(()) => replay_and_kick(s, base, &b, &i, u32::from(e.state), now),
                    Err(why) if debug => format!("nothing to do: {why}"),
                    Err(_) => continue,
                }
            }
            Ok(Plan::KickOnly) | Err(_) if !debug => continue,
            Ok(Plan::KickOnly) => "nothing to do: it ran here since the load".to_owned(),
            Err(why) => format!("nothing to do: {why}"),
        };
        log::line(&format!(
            "objact: journal: {who} ObjAct of this session (map {}, ObjActEntityID {}, ObjActParam {}, block slot {}, info index {}, {}), {} state, after its load: {what}",
            map_name(Some(e.map)),
            e.entity,
            e.param,
            b.slot,
            i.index,
            how_text(how, e.entity),
            class_name(class_code(class)),
        ));
    }
}

fn run_looks(s: &mut State, now: Instant) {
    let mut due = Vec::new();
    s.looks.retain(|l| {
        if now < l.due {
            return true;
        }
        due.push(l.clone());
        false
    });
    for l in due {
        log::line(&format!("objact: {}: {}", l.what, look(&l)));
    }
}

/// The info's state now (game thread): the same block (address, info array, map id) must
/// still be loaded, else a reloaded block in that slot would be read.
fn look(l: &Look) -> String {
    let mem = GameMem;
    let Some(b) = blocks(&mem, game::module_base()).into_iter().find(|b| b.addr == l.block && b.infos == l.infos && b.map == l.map) else {
        return "the block is gone (unloaded or reloaded)".to_owned();
    };
    let Some(i) = info_at(&mem, &b, l.index).filter(|i| i.index == l.index) else {
        return "the info is gone (out of range, unreadable or another index)".to_owned();
    };
    let view = if l.known { doors::object_view(b.addr, l.slot, l.index, l.handle, l.state) } else { None };
    format!(
        "run state {}, info+0x70/+0x71/+0x72 = {}/{}/{}, flag {}, object {}",
        i.run,
        i.kick,
        i.b113,
        i.b114,
        l.flag.map_or("none".to_owned(), |f| format!("{f} = {}", game::event_flag(f).map_or("?".to_owned(), |v| v.to_string()))),
        view.map_or("not looked at".to_owned(), |v| format!(
            "{:#x} anim {} (playing {}, the execution plays {}, reached {})",
            v.obj,
            v.anim,
            v.playing,
            v.want,
            v.reached()
        )),
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::BTreeMap;

    #[derive(Default)]
    struct Snap(BTreeMap<usize, Vec<u8>>);

    impl Snap {
        fn put(&mut self, at: usize, data: Vec<u8>) {
            self.0.insert(at, data);
        }
    }

    impl Mem for Snap {
        fn bytes(&self, addr: usize, out: &mut [u8]) -> bool {
            let Some((&start, data)) = self.0.range(..=addr).next_back() else { return false };
            let off = addr - start;
            if off + out.len() > data.len() {
                return false;
            }
            out.copy_from_slice(&data[off..off + out.len()]);
            true
        }
    }

    /// ObjActParam rows: a HUB door (one-state in the MSB), the m11_01 gate 1113602 (one-loop),
    /// the temple lift lever (one-state), the meditation room window (one-loop).
    const DOOR: i32 = 999_941;
    const GATE: i32 = 999_900;
    const LEVER: i32 = 205_011;
    const WINDOW: i32 = 1_100_000;

    const BASE: usize = 0x1_4000_0000;
    const MAN: usize = 0x10_0000;
    const BLOCKS: usize = 0x20_0000;
    const DESC: usize = 0x30_0000;
    const AREA: usize = 0x31_0000;
    const INFOS: usize = 0x40_0000;
    const STATES: usize = 0x50_0000;

    fn put_u32(v: &mut [u8], at: usize, x: u32) {
        v[at..at + 4].copy_from_slice(&x.to_le_bytes());
    }

    fn put_u64(v: &mut [u8], at: usize, x: u64) {
        v[at..at + 8].copy_from_slice(&x.to_le_bytes());
    }

    /// One loaded block (slot 3, map `id`, area byte `area`, info count `count`) with infos
    /// (vt RVA, run, b114, sub); info i has ObjActEntityID 1_113_700 + i, ObjActParam 999_900.
    fn world(id: u32, area: u8, count: u32, infos: &[(usize, u32, u8, u32)]) -> Snap {
        let mut m = Snap::default();
        m.put(BASE + WORLD_OBJACT_MAN, (MAN as u64).to_le_bytes().to_vec());
        let mut man = vec![0u8; 0x40];
        put_u32(&mut man, MAN_BLOCK_COUNT, 1);
        put_u64(&mut man, MAN_BLOCKS, BLOCKS as u64);
        m.put(MAN, man);
        let mut blk = vec![0u8; BLOCK_STRIDE];
        put_u64(&mut blk, 0, DESC as u64);
        blk[BLOCK_LOADED] = 1;
        put_u64(&mut blk, BLOCK_INFOS, INFOS as u64);
        put_u32(&mut blk, BLOCK_INFO_COUNT, count);
        m.put(BLOCKS, blk);
        let mut desc = vec![0u8; 0x40];
        put_u32(&mut desc, DESC_BLOCK_ID, id);
        put_u64(&mut desc, DESC_AREA, AREA as u64);
        put_u32(&mut desc, BLOCK_DESC_SLOT, 3);
        m.put(DESC, desc);
        let mut a = vec![0u8; 0x40];
        a[AREA_NUMBER] = area;
        m.put(AREA, a);
        let mut arr = vec![0u8; INFO_STRIDE * infos.len()];
        for (i, &(vt, run, b114, sub)) in infos.iter().enumerate() {
            let at = INFO_STRIDE * i;
            put_u32(&mut arr, at + INFO_INDEX, i as u32);
            put_u32(&mut arr, at + INFO_ENTITY, 1_113_700 + i as u32);
            put_u32(&mut arr, at + INFO_PARAM_ID, 999_900);
            put_u32(&mut arr, at + INFO_HANDLE, 0x100 + i as u32);
            put_u32(&mut arr, at + INFO_RUN_STATE, run);
            put_u32(&mut arr, at + INFO_SUB, sub);
            arr[at + INFO_B114] = b114;
            let st = STATES + 0x100 * i;
            put_u64(&mut arr, at + INFO_STATE, st as u64);
            let mut s = vec![0u8; 0x18];
            put_u64(&mut s, 0, (BASE + vt) as u64);
            put_u32(&mut s, STATE_FLAG, 11_000_000 + i as u32);
            m.put(st, s);
        }
        m.put(INFOS, arr);
        m
    }

    fn info(addr: usize, primary: usize) -> Info {
        Info { addr, index: 0, entity: 0, param: 0, sub: 0, run: 0, kick: 0, b113: 0, b114: 0, state: 0, primary, handle: 0 }
    }

    fn info_e(index: i32, entity: i32, sub: i32) -> Info {
        Info { index, entity, sub, ..info(0x1000 * (index as usize + 1), 0) }
    }

    fn here() -> Here {
        Here { executing: false, idle: Some(true), marked: false, replayed: false, executed: false, local_start: false, kicked: false, ours: false }
    }

    fn entry(entity: i32, class: u8, state: u8) -> Entry {
        Entry { map: 0x0B01_0000, entity, index: 3, sub: 0, param: 999_970, class, state, ours: false }
    }

    #[test]
    fn wire_v2_round_trip_and_rejects() {
        let m = Msg { map: 0x0B01_0000, entity: 1_113_602, index: 9, sub: 0, param: 999_900, class: 2, state: 1, seq: 7 };
        let d = encode(&m);
        assert_eq!(d.len(), WIRE_LEN);
        assert_eq!(d[1], 2);
        assert_eq!(decode(&d), Ok(m));
        // Negative ids survive.
        let n = Msg { entity: -1, index: -1, sub: -1, param: -1, ..m };
        assert_eq!(decode(&encode(&n)), Ok(n));
        assert!(decode(&d[..WIRE_LEN - 1]).is_err());
        let mut bad = d.clone();
        bad[0] = 0;
        assert_eq!(decode(&bad), Err("not an ObjAct message"));
        let mut reason = d.clone();
        reason[2] = 9;
        assert!(decode(&reason).is_err());
        // A version 1 message (25 bytes) of the first build is named as such.
        let mut v1 = vec![MAGIC, 1, REASON_EXEC, 0, 1];
        v1.extend_from_slice(&[0u8; 20]);
        assert!(decode(&v1).unwrap_err().starts_with("version 1"));
        let mut v3 = d;
        v3[1] = 3;
        assert!(decode(&v3).is_err());
    }

    #[test]
    fn walk_reads_map_id_entity_and_infos() {
        let m = world(0x0B00_0000, 11, 2, &[(0x298B570, 0, 0, 0), (0x298B628, 3, 1, 1)]);
        let bs = blocks(&m, BASE);
        assert_eq!(bs, vec![Block { addr: BLOCKS, slot: 3, map: Some(0x0B00_0000), infos: INFOS, count: 2 }]);
        let all = infos(&m, &bs[0], &mut Vec::new());
        assert_eq!(all.len(), 2);
        assert_eq!((all[1].index, all[1].sub, all[1].run, all[1].b114, all[1].handle), (1, 1, 3, 1, 0x101));
        assert_eq!((all[1].entity, all[1].param), (1_113_701, 999_900));
        assert_eq!(info_at(&m, &bs[0], 1), Some(all[1]));
        assert_eq!(info_at(&m, &bs[0], 2), None);
        assert_eq!(info_at(&m, &bs[0], -1), None);
        assert_eq!(class_of(&m, BASE, all[0].state), Some(Class::One));
        assert_eq!(class_of(&m, BASE, all[1].state), Some(Class::Door));
        assert_eq!(flag_of(&m, all[0].state, Class::One), None);
        assert_eq!(flag_of(&m, all[1].state, Class::Door), Some(11_000_001));
        // By ObjActEntityID, whatever the index says; by index when the id is not sent.
        assert_eq!(find(&m, BASE, 0x0B00_0000, 1_113_701, 0, 0).map(|(_, i, h)| (i.addr, h)), Ok((INFOS + INFO_STRIDE, How::Entity)));
        assert_eq!(find(&m, BASE, 0x0B00_0000, 0, 1, 1).map(|(_, i, h)| (i.addr, h)), Ok((INFOS + INFO_STRIDE, How::Index)));
        assert!(find(&m, BASE, 0x0B00_0000, 0, 1, 0).is_err());
        assert_eq!(find(&m, BASE, 0x0C00_0000, 1_113_701, 1, 1).map(|_| ()), Err("that map block is not loaded here"));
    }

    #[test]
    fn walk_caps_a_garbage_count_and_needs_the_matching_area() {
        let m = world(0x0B00_0000, 12, 1_000_000, &[]);
        let b = blocks(&m, BASE);
        assert_eq!(b[0].map, None);
        assert_eq!(b[0].count, MAX_BLOCK_INFOS);
        // The array is not there: nothing read, nothing returned.
        assert!(infos(&m, &b[0], &mut Vec::new()).is_empty());
    }

    #[test]
    fn locate_by_entity_with_index_fallback() {
        let all = [info_e(0, 100, 0), info_e(1, 200, 0), info_e(2, 0, 0), info_e(3, 300, 1)];
        // The id wins over a stale index.
        assert_eq!(locate(&all, 200, 0, 0).map(|(i, h)| (i.index, h)), Ok((1, How::Entity)));
        // Not there by id: the index, unless its own id is set and differs.
        assert_eq!(locate(&all, 999, 2, 0).map(|(i, h)| (i.index, h)), Ok((2, How::Index)));
        assert!(locate(&all, 999, 1, 0).is_err());
        // No id sent: the index / sub.
        assert_eq!(locate(&all, 0, 3, 1).map(|(i, h)| (i.index, h)), Ok((3, How::Index)));
        assert_eq!(locate(&all, -1, 1, 0).map(|(i, h)| (i.index, h)), Ok((1, How::Index)));
        assert!(locate(&all, 0, 3, 0).is_err());
        assert!(locate(&all, 0, 7, 0).is_err());
        // An index that moved: found by the index field, not the position.
        let moved = [info_e(5, 0, 0), info_e(4, 0, 0)];
        assert_eq!(locate(&moved, 0, 4, 0).map(|(i, _)| i.addr), Ok(moved[1].addr));
    }

    #[test]
    fn edges() {
        assert!(is_edge(0, 3) && is_edge(4, 3));
        assert!(!is_edge(3, 3) && !is_edge(0, 0) && !is_edge(3, 0));
    }

    #[test]
    fn echo_of_our_replay_is_never_sent() {
        let e = Evidence { our_replay: true, b114_rose: true, b114_now: true, own_anim: true };
        assert!(send_decision(e, Some(Class::One), DOOR, 1).is_err());
        let mut s = State::default();
        let now = Instant::now();
        let sub = info(0x2000_0130, 0x2000_0000);
        mark(&mut s, info(0x2000_0000, 0).objact_key(), now);
        // A sub info of the replayed door is the same ObjAct.
        assert!(replay_marked(&s, sub.objact_key(), now));
        assert!(!replay_marked(&s, 0x3000_0000, now));
        assert!(!replay_marked(&s, sub.objact_key(), now + REPLAY_MARK_FOR));
    }

    #[test]
    fn sub_infos_go_out_once() {
        let now = Instant::now();
        let a = info(0x2000_0130, 0x2000_0000);
        let b = info(0x2000_0260, 0x2000_0000);
        assert_eq!(a.objact_key(), b.objact_key());
        let keys = vec![(a.objact_key(), now)];
        assert!(sent_lately(&keys, b.objact_key(), now + Duration::from_millis(100)));
        assert!(!sent_lately(&keys, b.objact_key(), now + SAME_OBJACT_FOR));
        assert!(!sent_lately(&keys, info(0x2000_0390, 0).objact_key(), now));
    }

    #[test]
    fn poll_send_needs_more_than_our_animation() {
        let ev = |b114_rose, b114_now, own_anim| Evidence { our_replay: false, b114_rose, b114_now, own_anim };
        assert!(send_decision(ev(false, false, true), Some(Class::One), DOOR, 1).is_err());
        assert!(send_decision(ev(false, true, false), Some(Class::One), DOOR, 1).is_err());
        assert!(send_decision(ev(false, true, true), Some(Class::One), DOOR, 1).is_ok());
        assert!(send_decision(ev(true, true, false), Some(Class::Door), DOOR, 1).is_ok());
        assert!(send_decision(ev(false, false, false), Some(Class::One), DOOR, 1).is_err());
        assert!(send_decision(ev(true, true, true), Some(Class::OneLoop), LEVER, 1).is_err());
        assert!(send_decision(ev(true, true, true), Some(Class::OneLoop), LEVER, 2).is_ok());
        assert!(send_decision(ev(true, true, true), None, DOOR, 2).is_err());
    }

    #[test]
    fn hook_sends_only_our_players_started_execution() {
        let me = Some(0x1000_0001);
        assert!(hook_decision(Some(Class::One), DOOR, 1, 0x1000_0001, me, true).is_ok());
        assert!(hook_decision(Some(Class::Door), DOOR, 1, 0x1000_0001, me, true).is_ok());
        // An NPC / script chr, an unknown own handle, a refused start.
        assert!(hook_decision(Some(Class::One), DOOR, 1, 0x1000_0002, me, true).is_err());
        assert!(hook_decision(Some(Class::One), DOOR, 1, -1, me, true).is_err());
        assert!(hook_decision(Some(Class::One), DOOR, 1, 0x1000_0001, None, true).is_err());
        assert!(hook_decision(Some(Class::One), DOOR, 1, 0x1000_0001, me, false).is_err());
        // Off, one-loop below mode 2, unknown class.
        assert!(hook_decision(Some(Class::One), DOOR, 0, 0x1000_0001, me, true).is_err());
        assert!(hook_decision(Some(Class::OneLoop), LEVER, 1, 0x1000_0001, me, true).is_err());
        assert!(hook_decision(Some(Class::OneLoop), LEVER, 2, 0x1000_0001, me, true).is_ok());
        assert!(hook_decision(None, DOOR, 2, 0x1000_0001, me, true).is_err());
    }

    #[test]
    fn target_state_toggles_flag_states_only() {
        assert_eq!(target_state(Some(Class::Door), DOOR, Some(false)), 1);
        assert_eq!(target_state(Some(Class::Door), DOOR, Some(true)), 0);
        assert_eq!(target_state(Some(Class::OneLoop), LEVER, Some(false)), 1);
        assert_eq!(target_state(Some(Class::Door), DOOR, None), 1);
        assert_eq!(target_state(Some(Class::One), DOOR, Some(true)), 1);
        assert_eq!(target_state(None, DOOR, Some(true)), 1);
    }

    #[test]
    fn plan_replays_kicks_or_refuses() {
        assert_eq!(receive_plan(Some(Class::One), DOOR, 1, 1, here()), Ok(Plan::Replay));
        assert_eq!(receive_plan(Some(Class::Door), DOOR, 1, 1, here()), Ok(Plan::Replay));
        // Already ran here as a replay (type 45): only the kick.
        assert_eq!(receive_plan(Some(Class::One), DOOR, 1, 1, Here { executing: true, ..here() }), Ok(Plan::KickOnly));
        assert_eq!(receive_plan(Some(Class::One), DOOR, 1, 1, Here { executed: true, idle: Some(false), ..here() }), Ok(Plan::KickOnly));
        // Ran as a local start (kicked by the game), by our own player, or kicked already.
        assert!(receive_plan(Some(Class::One), DOOR, 1, 1, Here { executed: true, local_start: true, ..here() }).is_err());
        assert!(receive_plan(Some(Class::One), DOOR, 1, 1, Here { executed: true, ours: true, ..here() }).is_err());
        assert!(receive_plan(Some(Class::One), DOOR, 1, 1, Here { executed: true, kicked: true, ..here() }).is_err());
        assert!(receive_plan(Some(Class::One), DOOR, 1, 1, Here { kicked: true, ..here() }).is_err());
        // Marks and one replay per load.
        assert!(receive_plan(Some(Class::Door), DOOR, 1, 1, Here { marked: true, ..here() }).is_err());
        assert!(receive_plan(Some(Class::One), DOOR, 1, 1, Here { replayed: true, executed: true, ..here() }).is_err());
        // Idle facts.
        assert!(receive_plan(Some(Class::One), DOOR, 1, 1, Here { idle: Some(false), ..here() }).is_err());
        assert!(receive_plan(Some(Class::One), DOOR, 1, 1, Here { idle: None, ..here() }).is_err());
    }

    #[test]
    fn plan_validates_state_class_and_mode() {
        assert!(receive_plan(Some(Class::One), DOOR, 1, 0, here()).is_err());
        assert!(receive_plan(Some(Class::One), DOOR, 2, 1, here()).is_err());
        assert!(receive_plan(Some(Class::One), DOOR, 255, 2, here()).is_err());
        assert!(receive_plan(Some(Class::One), DOOR, 0, 1, here()).is_err());
        assert!(receive_plan(Some(Class::Door), DOOR, 0, 2, here()).is_err());
        assert!(receive_plan(Some(Class::OneDirDoor), DOOR, 0, 2, here()).is_err());
        assert!(receive_plan(None, DOOR, 1, 2, here()).is_err());
        assert!(receive_plan(Some(Class::OneLoop), LEVER, 1, 1, here()).is_err());
        assert_eq!(receive_plan(Some(Class::OneLoop), LEVER, 0, 2, here()), Ok(Plan::Replay));
        assert_eq!(object_decision(Some(false)), Ok(()));
        assert!(object_decision(Some(true)).is_err() && object_decision(None).is_err());
    }

    #[test]
    fn kick_only_when_the_replay_did_not() {
        assert_eq!(kick_plan(1_113_602, Some(0), 0), Ok(()));
        assert!(kick_plan(0, Some(0), 0).is_err());
        assert!(kick_plan(1_113_602, None, 0).is_err());
        assert!(kick_plan(1_113_602, Some(1), 0).is_err());
        assert!(kick_plan(1_113_602, Some(0), 1).is_err());
    }

    #[test]
    fn journal_keeps_open_objacts_per_entity() {
        let mut j = Journal::default();
        assert!(j.note(entry(1_113_713, 0, 1)).is_ok());
        assert!(j.note(entry(1_113_602, 1, 1)).is_ok());
        assert_eq!(j.entries.len(), 2);
        // The same ObjAct again: one entry, now the newest.
        assert_eq!(j.note(Entry { ours: true, ..entry(1_113_713, 0, 1) }), Ok("journaled again"));
        assert_eq!(j.entries.len(), 2);
        assert_eq!(j.entries[1], Entry { ours: true, ..entry(1_113_713, 0, 1) });
        // The same id in another map is another ObjAct.
        assert!(j.note(Entry { map: 0x0B02_0000, ..entry(1_113_713, 0, 1) }).is_ok());
        assert_eq!(j.entries.len(), 3);
        // A door closing again leaves; one-loop, no id, unknown class are never kept.
        assert_eq!(j.note(entry(1_113_602, 1, 0)), Err("closed again: taken out of the journal"));
        assert_eq!(j.entries.len(), 2);
        assert!(j.note(entry(1_113_602, 1, 0)).is_err());
        assert!(j.note(entry(2_005_010, 2, 1)).is_err());
        assert!(j.note(entry(0, 0, 1)).is_err());
        assert!(j.note(entry(-1, 0, 1)).is_err());
        assert!(j.note(entry(1_113_999, 0xFF, 1)).is_err());
        assert_eq!(j.entries.len(), 2);
        assert_eq!(j.clear(), 2);
        assert!(j.entries.is_empty());
    }

    #[test]
    fn journal_is_capped_oldest_first() {
        let mut j = Journal::default();
        for e in 1..=(MAX_JOURNAL as i32 + 3) {
            assert!(j.note(entry(e, 0, 1)).is_ok());
        }
        assert_eq!(j.entries.len(), MAX_JOURNAL);
        assert_eq!(j.dropped, 3);
        assert_eq!(j.entries[0].entity, 4);
        assert_eq!(j.entries.last().map(|e| e.entity), Some(MAX_JOURNAL as i32 + 3));
    }

    #[test]
    fn journal_waits_for_a_settled_info_and_tries_once() {
        assert!(journal_gate(None, false).is_err());
        assert!(journal_gate(Some(Duration::from_secs(1)), false).is_err());
        assert_eq!(journal_gate(Some(JOURNAL_SETTLE), false), Ok(()));
        assert!(journal_gate(Some(JOURNAL_SETTLE * 10), true).is_err());
        // What follows is the receiver's plan: replay only an idle info that did not run.
        assert_eq!(receive_plan(Some(Class::One), DOOR, 1, 1, here()), Ok(Plan::Replay));
    }

    #[test]
    fn mode_1_syncs_doors_and_chests_of_any_class() {
        // The live case: gate 1113602 (one-loop, ObjActParam 999900) goes out at mode 1.
        assert!(syncs(Some(Class::OneLoop), GATE, 1).is_ok());
        assert!(syncs(Some(Class::One), DOOR, 1).is_ok());
        assert!(syncs(Some(Class::One), 999_980, 1).is_ok());
        // Levers (one-state!), windows, bells, rotating walls / floors: only at mode 2.
        for (c, p) in [(Class::One, LEVER), (Class::OneLoop, WINDOW), (Class::OneLoop, 252_555), (Class::One, 90_075), (Class::OneLoop, 90_061), (Class::One, -1)] {
            assert!(syncs(Some(c), p, 1).is_err(), "{p}");
            assert!(syncs(Some(c), p, 2).is_ok(), "{p}");
        }
        assert!(syncs(None, GATE, 2).is_err());
        let me = Some(0x1000_0001);
        assert!(hook_decision(Some(Class::OneLoop), GATE, 1, 0x1000_0001, me, true).is_ok());
        assert!(hook_decision(Some(Class::One), LEVER, 1, 0x1000_0001, me, true).is_err());
        let ev = Evidence { our_replay: false, b114_rose: true, b114_now: true, own_anim: true };
        assert!(send_decision(ev, Some(Class::OneLoop), GATE, 1).is_ok());
        assert!(send_decision(ev, Some(Class::OneLoop), WINDOW, 1).is_err());
        assert_eq!(receive_plan(Some(Class::OneLoop), GATE, 1, 1, here()), Ok(Plan::Replay));
        assert!(receive_plan(Some(Class::One), LEVER, 1, 1, here()).is_err());
    }

    #[test]
    fn a_loop_door_always_targets_open() {
        // The live case: the host used the gate whose flag (from the client) was on already.
        assert_eq!(target_state(Some(Class::OneLoop), GATE, Some(true)), 1);
        assert_eq!(target_state(Some(Class::OneLoop), GATE, Some(false)), 1);
        assert_eq!(target_state(Some(Class::OneLoop), GATE, None), 1);
        // A window (one-loop, not a door) keeps the toggle.
        assert_eq!(target_state(Some(Class::OneLoop), WINDOW, Some(true)), 0);
        // Target 0 for a one-loop door is refused at every mode; for a window at mode 2 it is not.
        assert!(receive_plan(Some(Class::OneLoop), GATE, 0, 1, here()).is_err());
        assert!(receive_plan(Some(Class::OneLoop), GATE, 0, 2, here()).is_err());
        assert_eq!(receive_plan(Some(Class::OneLoop), WINDOW, 0, 2, here()), Ok(Plan::Replay));
    }

    #[test]
    fn loop_doors_levers_and_walls_are_never_journaled() {
        let mut j = Journal::default();
        let e = |class, param| Entry { map: 0x0B01_0000, entity: 1_113_602, index: 9, sub: 0, param, class, state: 1, ours: true };
        assert_eq!(j.note(e(2, GATE)), Err("one-loop door (never journaled: the load sets it up from its flag)"));
        assert!(j.note(e(0, LEVER)).unwrap_err().starts_with("not a door or chest"));
        assert!(j.note(e(0, 90_075)).is_err());
        assert!(j.note(e(2, WINDOW)).unwrap_err().starts_with("one-loop state"));
        assert!(j.entries.is_empty());
        assert!(j.note(e(0, 999_980)).is_ok());
        assert!(j.note(Entry { entity: 1_113_712, ..e(0, DOOR) }).is_ok());
        assert_eq!(j.entries.len(), 2);
    }

    /// One player's view of a one-loop door: its MSB flag and whether the object is open.
    #[derive(Clone, Copy, Debug, PartialEq, Eq)]
    struct Side {
        flag: bool,
        open: bool,
        /// The world flag sync's last known value (world.rs `known`).
        known: bool,
    }

    impl Side {
        /// Our player's local start (`0x6BA7B0`): refused when the object is at the
        /// animation; else it opens and toggles the flag; guard 5 (doors.rs) puts an OFF back
        /// ON. Returns the message and the flag write the sync sends (None: none or quiet).
        fn use_it(&mut self) -> Option<(u8, Option<bool>)> {
            if self.open {
                return None;
            }
            let before = self.flag;
            self.open = true;
            self.flag = !self.flag;
            let mut sent = Some(self.flag);
            // doors.rs guard 5: the detour noted ON -> OFF; world.rs asks keep_open (known ON).
            if before && !self.flag && self.known && doors::keep_open_loop_decision(true, true) {
                self.flag = true;
                sent = None;
            }
            if let Some(v) = sent {
                self.known = v;
            }
            Some((target_state(Some(Class::OneLoop), GATE, Some(before)), sent))
        }

        /// world.rs applies the friend's flag (no guard for one-loop door flags).
        fn flag_arrives(&mut self, v: bool) {
            self.flag = v;
            self.known = v;
        }

        /// objact.rs receives the friend's execution (target `state`).
        fn msg_arrives(&mut self, state: u8) {
            let plan = receive_plan(Some(Class::OneLoop), GATE, state, 1, here());
            if plan == Ok(Plan::Replay) && object_decision(Some(self.open)).is_ok() {
                // 0x6BA910: vt[5] SETS the flag, the opening animation plays.
                self.flag = state != 0;
                self.open = true;
            }
        }

        /// A reload: the engine sets the object up from the flag [ARV].
        fn reload(&mut self) {
            self.open = self.flag;
        }
    }

    fn closed() -> Side {
        Side { flag: false, open: false, known: false }
    }

    fn converged(a: Side, b: Side) {
        assert!(a.flag && a.open && b.flag && b.open, "a {a:?} b {b:?}");
        let (mut a, mut b) = (a, b);
        a.reload();
        b.reload();
        assert!(a.open && b.open, "after a reload: a {a:?} b {b:?}");
    }

    #[test]
    fn loop_door_converges_in_every_order() {
        // 1. Flag first, then the message (the usual order: the flag is in the snapshot).
        let (mut a, mut b) = (closed(), closed());
        let (msg, flag) = a.use_it().unwrap();
        assert_eq!((msg, flag), (1, Some(true)));
        b.flag_arrives(true);
        b.msg_arrives(msg);
        converged(a, b);
        // 2. Message first, then the flag.
        let (mut a, mut b) = (closed(), closed());
        let (msg, _) = a.use_it().unwrap();
        b.msg_arrives(msg);
        b.flag_arrives(true);
        converged(a, b);
        // 3. The live failure: B gets the flag, uses its still closed door before the replay.
        let (mut a, mut b) = (closed(), closed());
        let (msg_a, _) = a.use_it().unwrap();
        b.flag_arrives(true);
        let (msg_b, flag_b) = b.use_it().unwrap();
        // B's toggle OFF is put back ON quietly, B sends "open" (1), no OFF goes out.
        assert_eq!((msg_b, flag_b), (1, None));
        assert!(b.flag && b.open);
        b.msg_arrives(msg_a); // already open: nothing
        a.msg_arrives(msg_b); // already open: nothing
        converged(a, b);
        // 4. Both use it at the same time (each before the other's anything).
        let (mut a, mut b) = (closed(), closed());
        let (ma, fa) = a.use_it().unwrap();
        let (mb, fb) = b.use_it().unwrap();
        a.flag_arrives(fb.unwrap());
        b.flag_arrives(fa.unwrap());
        a.msg_arrives(mb);
        b.msg_arrives(ma);
        converged(a, b);
        // 5. B's block is not loaded: only the flag; B loads the area later.
        let (mut a, mut b) = (closed(), closed());
        a.use_it().unwrap();
        b.flag_arrives(true);
        b.reload();
        converged(a, b);
        // 6. A message of the old build (target 0 after a toggle of an open flag) is refused.
        let mut b = Side { flag: true, open: true, known: true };
        b.msg_arrives(0);
        assert!(b.flag && b.open);
    }

    #[test]
    fn guard_5_note_needs_our_player_and_an_on_to_off_toggle() {
        let me = Some(0x1000_0001);
        assert!(own_loop_toggle_off(0x1000_0001, me, Some(true), Some(false)));
        // An NPC / script actor, an unknown own handle, a normal first opening, no toggle.
        assert!(!own_loop_toggle_off(0x1000_0002, me, Some(true), Some(false)));
        assert!(!own_loop_toggle_off(0x1000_0001, None, Some(true), Some(false)));
        assert!(!own_loop_toggle_off(0x1000_0001, me, Some(false), Some(true)));
        assert!(!own_loop_toggle_off(0x1000_0001, me, Some(true), Some(true)));
        assert!(!own_loop_toggle_off(0x1000_0001, me, None, Some(false)));
    }

    #[test]
    fn names() {
        assert_eq!(map_name(Some(0x0B01_0000)), "m11_01_00_00 (0x0b010000)");
        assert_eq!(class_name(class_code(Some(Class::OneDirDoor))), "one-dir-door");
        assert!(logged_flag(61_100_610) && logged_flag(11_000_670) && !logged_flag(9100));
        assert_eq!(how_text(How::Entity, 5), "found by ObjActEntityID");
        assert!(how_text(How::Index, 5).contains("5 not in that block"));
    }
}
