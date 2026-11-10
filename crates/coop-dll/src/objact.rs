//! ObjAct sync (experimental, `[GAMEPLAY] sync_objact`, NOT verified live; FORK.md
//! "ObjAct sync").
//!
//! Problem: only event flags travel (world.rs). A door / gate / lever whose ObjAct state
//! keeps no flag (`SprjObjActOneState`, most of the doors seen live: `one-state 24, door 0,
//! one-loop 9`, docs/JATKA-TASTA-MULTIPLAYER.md TODO 6) is never replayed by doors.rs, so
//! the object the friend opened stays closed here until a reload (IMPLEMENTATION-NOTES
//! §37 "Risks"). The game's own replay of a friend's ObjAct (type 45 -> `0x6BE740` ->
//! `0x6BA910(block, index, actor, state)`) only runs while the native session is engaged
//! at stage 4.
//!
//! - **Sender (poll, not a detour).** Every 50 ms the WorldObjActMan walk of doors.rs reads
//!   each loaded block's info array in one read (at most `MAX_BLOCK_INFOS` per block and
//!   `MAX_POLL_INFOS` per poll). An info whose run state (`+0x54`) turns 3 (executing,
//!   `0x6B31E0`) is a new execution. It is judged 150 ms later (`send_decision`): ours when
//!   `info+114` went 0 -> 1 between the poll before the edge and that moment (the player's
//!   own path `0x6BA7B0` -> `0x6BC1B0` sets it; the replay `0x6BA910` sets `+113 = 1,
//!   +114 = 0`; IMPLEMENTATION-NOTES §30), or `+114` is still 1 from an earlier execution AND
//!   our player started an `ObjActParam.playerAnimId` animation within 2 s (that evidence is
//!   used for one send only). Our animation alone is not enough: every execution in every
//!   loaded block would match (an EMEVD gate, the friend's type 45), and no object position
//!   is known to check that the object is near us. Never ours: one of our own `0x6BA910`
//!   replays (doors.rs and this module mark them per ObjAct, `Info::objact_key`), a one-loop
//!   state unless `sync_objact = 2`, an unknown state class. All infos of one ObjAct (a
//!   door's sub infos share their primary `+0xB0`) are sent once per 2 s.
//!   The message (bulk `BULK_OBJACT`, every peer: coop-net's host relays a client's transfer
//!   to the other clients with its sender) carries the block's map id (WorldBlockInfo
//!   `+0x08`, checked against its area; self-describing, unlike a slot number), info index,
//!   sub index, state class, target state, sequence number.
//!   Why not a detour on `0x6BC1B0`: its signature and prologue bytes are in no note of this
//!   repo, and a wrong guess crashes the game. With `objact_debug = 1` the first bytes of
//!   `0x6BC1B0`, `0x6BA7B0`, `0x6BA910` and the raw info of each execution are logged.
//! - **Receiver** (game thread, 300 ms after arrival, retried for 5 s): the loaded block with
//!   that map id, the info by index / sub, then `replay_gate` (state must be 0/1, a one-state
//!   only 1; not marked as replayed by doors.rs or us; at most one replay per ObjAct per load;
//!   not executed here since the load; idle = the run state it had when first seen after the
//!   load; class known; one-loop only with `sync_objact = 2`; never a door toggling closed)
//!   and only then the object (`doors::object_view`, never for an unknown class): already at
//!   that execution's animation = nothing. Else `0x6BA910(block, index, -1, state)`.
//! - **Logging** (`objact:` lines): our executions (and every one with `+114 = 1` or our
//!   animation near, sent or not), the info 1 s later, the map flags 5 s around ours; on the
//!   receiver what arrived, what was done, the info 1 s and 3 s later.
//!
//! Open: the ObjActParam id is not located in the info (sent as -1); the actor is -1 (no
//! animation on our copy of the friend); a player standing on a lift is not checked.

use std::collections::{HashMap, HashSet, VecDeque};
use std::sync::Mutex;
use std::sync::atomic::{AtomicBool, AtomicU8, Ordering};
use std::time::{Duration, Instant};

use crate::doors::{
    self, BLOCK_DESC_SLOT, BLOCK_INFO_COUNT, BLOCK_INFOS, BLOCK_LOADED, BLOCK_STRIDE, Class, GameMem, INFO_HANDLE, INFO_INDEX,
    INFO_RUN_STATE, INFO_STATE, INFO_STRIDE, INFO_SUB, MAN_BLOCK_COUNT, MAN_BLOCKS, MAX_BLOCKS, Mem, REPLAY_RVA, RUN_EXECUTING,
    STATE_FLAG, STATE_VTS, WORLD_OBJACT_MAN, le_ptr, le_u32,
};
use crate::game;
use crate::log;
use crate::net::{Dest, Net};
use crate::peers::PeerId;

/// `[GAMEPLAY] sync_objact`: 0 off, 1 doors / gates / levers (default), 2 also one-loop states.
pub static MODE: AtomicU8 = AtomicU8::new(1);
/// `[DEBUG] objact_debug`: log every execution, every map flag, raw infos and prologues.
pub static DEBUG: AtomicBool = AtomicBool::new(false);

/// Bulk kind of one ObjAct execution (taken kinds: world.rs `BULK_PHASE` list, 46 pickups).
/// An older build logs `net: bulk transfer kind 167 received` and leaves it in its 24-entry
/// inbox, where newer transfers push it out.
pub const BULK_OBJACT: u8 = 0xA7;
const MAGIC: u8 = 0x4F;
const WIRE_VERSION: u8 = 1;
/// Reason byte: an execution by the sender's own player.
const REASON_EXEC: u8 = 1;
const WIRE_LEN: usize = 25;

/// Info bytes (decimal, as in the IDA output, IMPLEMENTATION-NOTES §30): `+113` set by the
/// replay `0x6BA910`, `+114` set to 1 by the player's own execution `0x6BC1B0` [VAR].
const INFO_B113: usize = 113;
const INFO_B114: usize = 114;
/// Primary info of a door's sub infos [VAR, §37].
const INFO_PRIMARY: usize = 0xB0;
/// The block descriptor is a `WorldBlockInfo` [ARV]: its `+0x20` is the slot doors.rs and
/// `0x6BA910` use (`world_block_index` in vendor/sekiro `WorldBlockInfo`); `+0x08` is
/// `block_id` (area << 24 | block << 16 ...), `+0x18` the `WorldAreaInfo`, whose `+0x0B`
/// area number must equal the id's top byte (checked, else no map id).
const DESC_BLOCK_ID: usize = 0x08;
const DESC_AREA: usize = 0x18;
const AREA_NUMBER: usize = 0x0B;

/// TODO(live test): the actor of our replay. -1 = no chr plays the animation (as doors.rs).
/// If the first test shows the object animating but still blocking the way, try the
/// friend's native replica handle here (chr+0x78 net form, nsession.rs).
const REPLAY_ACTOR: i32 = -1;

/// Infos read per block (a garbage count must not make a poll read megabytes).
const MAX_BLOCK_INFOS: usize = 512;
/// Infos read per poll, all blocks together.
const MAX_POLL_INFOS: usize = 2048;
const POLL_EVERY: Duration = Duration::from_millis(50);
/// An execution is judged this long after its run state turned 3 (`+114`, our animation).
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
/// Map flags logged this long before and after an execution of ours.
const FLAG_WINDOW: Duration = Duration::from_secs(5);
const SENDER_LOOK: Duration = Duration::from_secs(1);
const RECEIVER_LOOKS: [Duration; 2] = [Duration::from_secs(1), Duration::from_secs(3)];
/// Raw info dumps per load (`objact_debug`).
const MAX_INFO_DUMPS: u32 = 8;
const MAX_FLAGS: usize = 256;

/// One execution on the wire.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct Msg {
    map: u32,
    index: i32,
    sub: i32,
    /// ObjActParam id; -1 = not located yet.
    param: i32,
    /// The game's state type: 0 one-state, 1 door, 2 one-loop, 4 one-dir door, 0xFF unknown.
    class: u8,
    /// Target state for `0x6BA910` (0 / 1).
    state: u8,
    seq: u32,
}

fn encode(m: &Msg) -> Vec<u8> {
    let mut d = Vec::with_capacity(WIRE_LEN);
    d.extend_from_slice(&[MAGIC, WIRE_VERSION, REASON_EXEC, m.class, m.state]);
    d.extend_from_slice(&m.map.to_le_bytes());
    d.extend_from_slice(&m.index.to_le_bytes());
    d.extend_from_slice(&m.sub.to_le_bytes());
    d.extend_from_slice(&m.param.to_le_bytes());
    d.extend_from_slice(&m.seq.to_le_bytes());
    d
}

fn decode(d: &[u8]) -> Option<Msg> {
    if d.len() < WIRE_LEN || d[0] != MAGIC || d[1] != WIRE_VERSION || d[2] != REASON_EXEC {
        return None;
    }
    let u = |at: usize| u32::from_le_bytes(d[at..at + 4].try_into().unwrap());
    Some(Msg { class: d[3], state: d[4], map: u(5), index: u(9) as i32, sub: u(13) as i32, param: u(17) as i32, seq: u(21) })
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
    sub: i32,
    run: u32,
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
    (0..b.count)
        .map(|i| {
            let r = &buf[INFO_STRIDE * i..INFO_STRIDE * (i + 1)];
            Info {
                addr: b.infos + INFO_STRIDE * i,
                index: le_u32(r, INFO_INDEX) as i32,
                sub: le_u32(r, INFO_SUB) as i32,
                run: le_u32(r, INFO_RUN_STATE),
                b113: r[INFO_B113],
                b114: r[INFO_B114],
                state: le_ptr(r, INFO_STATE),
                primary: le_ptr(r, INFO_PRIMARY),
                handle: le_u32(r, INFO_HANDLE),
            }
        })
        .collect()
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

/// The info `index` / `sub` of the loaded block with map id `map`.
fn find(mem: &impl Mem, base: usize, map: u32, index: i32, sub: i32) -> Option<(Block, Info)> {
    let mut buf = Vec::new();
    for b in blocks(mem, base).into_iter().filter(|b| b.map == Some(map)) {
        let all = infos(mem, &b, &mut buf);
        let at = usize::try_from(index).ok().and_then(|i| all.get(i)).copied();
        let hit = at.filter(|i| i.index == index && i.sub == sub).or_else(|| all.iter().find(|i| i.index == index && i.sub == sub).copied());
        if let Some(i) = hit {
            return Some((b, i));
        }
    }
    None
}

// ------------------------------------------------------------------ pure decisions

/// A poll saw run state `run` where the poll before saw `prev`: a new execution.
fn is_edge(prev: u32, run: u32) -> bool {
    run == RUN_EXECUTING && prev != RUN_EXECUTING
}

/// What says that a new execution here is our player's.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
struct Evidence {
    /// A `0x6BA910` of ours or doors.rs on this ObjAct a moment ago.
    our_replay: bool,
    /// `+114` was not 1 in the poll before the edge and is 1 at the decision.
    b114_rose: bool,
    /// `+114` is 1 at the decision (sticky: only a replay resets it).
    b114_now: bool,
    /// Our player started an ObjAct player animation within 2 s (not used up yet).
    own_anim: bool,
}

/// Sender: send this execution (Ok, why) or not (Err, why).
fn send_decision(e: Evidence, class: Option<Class>, mode: u8) -> Result<&'static str, &'static str> {
    if e.our_replay {
        return Err("our own replay of a friend's ObjAct (doors.rs / objact.rs)");
    }
    match class {
        None => return Err("unknown state class"),
        Some(Class::OneLoop) if mode < 2 => return Err("one-loop state (a lift or a lever loop?): sent only with sync_objact = 2"),
        _ => {}
    }
    if e.b114_rose {
        return Ok("info+114 went 0 -> 1");
    }
    match (e.b114_now, e.own_anim) {
        (true, true) => Ok("info+114 = 1 (from before) and our ObjAct animation"),
        (true, false) => Err("info+114 = 1 is left from an earlier execution and no ObjAct animation of ours"),
        (false, true) => Err("only our ObjAct animation as evidence (no object position known to check it is this object)"),
        (false, false) => Err("not ours (the friend's type 45, an NPC or a script)"),
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
}

/// Receiver, before the object is looked at (`0x6BCBC0` is only called when this is Ok).
fn replay_gate(class: Option<Class>, state: u8, mode: u8, h: Here) -> Result<(), &'static str> {
    if mode == 0 {
        return Err("sync_objact = 0 here");
    }
    if state > 1 {
        return Err("target state is not 0 / 1 (bad message)");
    }
    match class {
        None => return Err("unknown state class"),
        Some(Class::One) if state != 1 => return Err("one-state with target state 0 (bad message)"),
        Some(Class::OneLoop) if mode < 2 => return Err("one-loop state (a lift or a lever loop?): replayed only with sync_objact = 2"),
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
    if h.executing {
        return Err("it is executing here now (the game's type 45, or our own)");
    }
    if h.executed {
        return Err("it executed here since the load (opened here already)");
    }
    match h.idle {
        None => Err("not watched yet since the load"),
        Some(false) => Err("its run state is not the one it had at the load"),
        Some(true) => Ok(()),
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

fn near(a: Instant, b: Instant, within: Duration) -> bool {
    if a >= b { a.duration_since(b) <= within } else { b.duration_since(a) <= within }
}

// ------------------------------------------------------------------ state

/// Per info since the load: run state and `+114` at the last poll, run state when idle.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct Seen {
    run: u32,
    b114: u8,
    idle: u32,
}

/// A new execution, judged `DECIDE_AFTER` later.
#[derive(Clone, Copy)]
struct Edge {
    at: Instant,
    block: Block,
    info: Info,
    /// `+114` in the poll before the edge.
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
    /// ObjActs sent lately (`objact_key`, when).
    sent_keys: Vec<(usize, Instant)>,
    /// Map flag writes of the last `FLAG_WINDOW` (when, id, value).
    flags: VecDeque<(Instant, u32, bool)>,
    flag_window_until: Option<Instant>,
    last_own_anim: i32,
    own_objact: Option<(i32, Instant)>,
    seq: u32,
    received: u32,
    replayed: u32,
    dropped: u32,
    dumps: u32,
    logged_blocks: bool,
    logged_cap: bool,
    logged_prologues: bool,
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

/// The `objact_key` of info `index` of `block` (doors.rs passes block and index).
fn key_in_game(block: usize, index: i32) -> Option<usize> {
    let infos = game::read_ptr(block + BLOCK_INFOS)?;
    let addr = infos + INFO_STRIDE * usize::try_from(index).ok()?;
    Some(key_of(addr, game::read_ptr(addr + INFO_PRIMARY).unwrap_or(0)))
}

/// doors.rs, right before its own `0x6BA910(block, index, ...)`: that execution is a
/// replay, not ours (no echo to the friend), and no second replay of ours follows.
pub fn note_replay(block: usize, index: i32) {
    let Some(key) = key_in_game(block, index) else { return };
    let now = Instant::now();
    with(|s| mark(s, key, now));
}

/// doors.rs, before its `0x6BA910`: we replayed that ObjAct a moment ago (the friend's 0xA7).
pub fn replayed_recently(block: usize, index: i32) -> bool {
    let Some(key) = key_in_game(block, index) else { return false };
    let now = Instant::now();
    with(|s| replay_marked(s, key, now))
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

// ------------------------------------------------------------------ frame

/// world.rs, every frame while connected (game thread). `ready`: settled after the load and
/// the flag manager readable (real world bank).
pub fn tick(net: &mut Net, in_world_for: Duration, ready: bool) {
    let now = Instant::now();
    let mode = MODE.load(Ordering::Relaxed);
    let debug = DEBUG.load(Ordering::Relaxed);
    // 1. The peers' executions: always read, so nothing stays in the inbox when off. A
    //    client's transfer reaches the other clients through coop-net's host relay.
    while let Some((from, d)) = net.take_bulk_from(BULK_OBJACT) {
        match decode(&d) {
            Some(m) => with(|s| receive(s, from, m, mode, now)),
            None => log::line(&format!("objact: unreadable message from peer {} ({} bytes), ignored", from.slot(), d.len())),
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
            s.sent_keys.clear();
            s.own_objact = None;
            s.dumps = 0;
            s.logged_blocks = false;
        }
        s.last_in_world = in_world_for;
        if debug && !s.logged_prologues {
            s.logged_prologues = true;
            for (rva, name) in [(0x6BC1B0usize, "ObjAct execution 0x6BC1B0"), (0x6BA7B0, "own ObjAct 0x6BA7B0"), (REPLAY_RVA, "replay 0x6BA910")] {
                let mut b = [0u8; 24];
                let text = if game::read_bytes(base + rva, &mut b) { hex(&b) } else { "unreadable".to_owned() };
                log::line(&format!("objact: {name} first bytes {text}"));
            }
        }
        own_anim(s, now);
        if s.last_poll.is_none_or(|t| now.duration_since(t) >= POLL_EVERY) {
            s.last_poll = Some(now);
            poll(s, base, now, debug);
        }
        decide(s, now, mode, debug)
    });
    for m in out {
        // Host: to every client; client: to the host, which relays it to the other clients.
        let ok = net.send_bulk_to(Dest::All, BULK_OBJACT, &encode(&m));
        log::line(&format!(
            "objact: sent our ObjAct #{} (map {}, info index {}, sub {}, state {}): ok = {ok}",
            m.seq,
            map_name(Some(m.map)),
            m.index,
            m.sub,
            m.state
        ));
    }
    with(|s| {
        apply(s, base, now, mode);
        run_looks(s, now);
    });
}

/// Our own player's newest animation: an ObjAct player animation is evidence for one send.
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
fn poll(s: &mut State, base: usize, now: Instant, debug: bool) {
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
            s.seen.insert(i.addr, Seen { run: i.run, b114: i.b114, idle });
            if let Some(p) = prev
                && is_edge(p.run, i.run)
            {
                new.push((*b, i, p.b114));
            }
        }
    }
    s.seen.retain(|a, _| alive.contains(a));
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
        // only for an execution that may be sent (or when debugging).
        let start = if class.is_some() && (debug || !replay) { doors::object_view(b.addr, b.slot, i.index, i.handle, 1) } else { None };
        if s.edges.len() < 32 {
            s.edges.push(Edge { at: now, block: b, info: i, prev_b114, class, flag, flag_at_edge, start, replay });
        }
    }
}

/// Executions `DECIDE_AFTER` old: ours -> messages (sent by the caller), logged.
fn decide(s: &mut State, now: Instant, mode: u8, debug: bool) -> Vec<Msg> {
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
        let own = s.own_objact.filter(|&(_, t)| near(t, e.at, OWN_ANIM_NEAR));
        let ev = Evidence { our_replay: e.replay, b114_rose: e.prev_b114 != 1 && b114_now == 1, b114_now: b114_now == 1, own_anim: own.is_some() };
        let verdict = send_decision(ev, e.class, mode);
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
        // Flag states toggle (state slot 4): the target is the opposite of "before".
        let state = match (e.class.is_some_and(Class::has_flag), flag_before) {
            (true, Some(v)) => u8::from(!v),
            _ => 1,
        };
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
                    out.push(Msg { map, index: e.info.index, sub: e.info.sub, param: -1, class: class_code(e.class), state, seq: s.seq });
                    Ok(s.seq)
                }
            },
        };
        let start = e.start.map_or("? (not looked at)".to_owned(), |v| format!("{:#x} anim {} (playing {}, the open execution plays {})", v.obj, v.anim, v.playing, v.want));
        let anim = own.map_or("none within 2 s".to_owned(), |(a, t)| format!("{a} ({:+} ms)", signed_ms(t, e.at)));
        let fmt_flag = |v: Option<bool>| v.map_or("?".to_owned(), |b| b.to_string());
        log::line(&format!(
            "objact: ObjAct ran here: map {} (block slot {}, info index {}, sub {}), {} state, flag {} before {} now {}, info+113/+114 = {}/{} (+114 before the edge {}), ObjActParam id ? (not located), object handle {:#x}, object at start {start}, our ObjAct animation {anim}; {}",
            map_name(e.block.map),
            e.block.slot,
            e.info.index,
            e.info.sub,
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
        if decision.is_err() {
            continue;
        }
        // The map flags around it: the last 5 s now, the next 5 s as they come.
        if !debug && s.flag_window_until.is_none_or(|t| now > t) {
            let before: Vec<String> =
                s.flags.iter().filter(|&&(t, ..)| t + FLAG_WINDOW >= e.at).map(|&(t, id, on)| format!("{id} = {on} ({:+} ms)", signed_ms(t, e.at))).collect();
            log::line(&format!("objact: map flags written from {} s before it until now: [{}]", FLAG_WINDOW.as_secs(), before.join(", ")));
        }
        s.flag_window_until = Some(now + FLAG_WINDOW);
        if s.looks.len() < 64 {
            s.looks.push(Look {
                due: e.at + SENDER_LOOK,
                what: format!("{} s after our ObjAct (map {}, info index {})", SENDER_LOOK.as_secs(), map_name(e.block.map), e.info.index),
                block: e.block.addr,
                infos: e.block.infos,
                map: e.block.map,
                slot: e.block.slot,
                index: e.info.index,
                handle: e.info.handle,
                known: e.class.is_some(),
                flag: e.flag,
                state: u32::from(state),
            });
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
    log::line(&format!(
        "objact: peer {} ran ObjAct #{}: map {}, info index {}, sub {}, {} state, target state {}, ObjActParam id {} (#{} received){}",
        from.slot(),
        m.seq,
        map_name(Some(m.map)),
        m.index,
        m.sub,
        class_name(m.class),
        m.state,
        m.param,
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

/// The peers' executions that are due: replayed here when loaded, idle and still closed.
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
        let Some((b, i)) = find(&mem, base, m.map, m.index, m.sub) else {
            if now.duration_since(x.at) < APPLY_WITHIN {
                x.next_try = now + RETRY_EVERY;
                s.incoming.push(x);
            } else {
                log::line(&format!(
                    "objact: peer {}'s ObjAct #{} (map {}, info index {}, sub {}): no such loaded info here within {} s, not replayed (its event flag still applies; a one-state object may stay as it was until a reload)",
                    x.from.slot(),
                    m.seq,
                    map_name(Some(m.map)),
                    m.index,
                    m.sub,
                    APPLY_WITHIN.as_secs()
                ));
            }
            continue;
        };
        let class = class_of(&mem, base, i.state);
        let key = i.objact_key();
        let here = Here {
            executing: i.run == RUN_EXECUTING,
            idle: s.seen.get(&i.addr).map(|x| x.idle == i.run),
            marked: replay_marked(s, key, now),
            replayed: s.replayed_keys.contains(&key),
            executed: s.executed_keys.contains(&key),
        };
        let state = u32::from(m.state);
        let gate = replay_gate(class, m.state, mode, here);
        // 0x6BCBC0 only for a known class and a valid state (the gate checks both).
        let view = if gate.is_ok() { doors::object_view(b.addr, b.slot, i.index, i.handle, state) } else { None };
        let verdict = gate.and_then(|()| object_decision(view.map(|v| v.reached())));
        let what = match verdict {
            Ok(()) => {
                mark(s, key, now);
                s.replayed_keys.insert(key);
                let replay: extern "C" fn(usize, i32, i32, u32) -> u8 = unsafe { std::mem::transmute(base + REPLAY_RVA) };
                let ok = replay(b.addr, i.index, REPLAY_ACTOR, state) != 0;
                s.replayed += 1;
                format!("replayed with 0x6BA910(block, {}, actor {REPLAY_ACTOR}, state {state}): ok = {ok} ({} so far)", i.index, s.replayed)
            }
            Err(why) => format!("not replayed: {why}"),
        };
        log::line(&format!(
            "objact: peer {}'s ObjAct #{} here: block slot {}, {} state{}, run state {} (idle {:?}), info+113/+114 = {}/{}, object {}; {what}",
            x.from.slot(),
            m.seq,
            b.slot,
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
    let Some(ix) = usize::try_from(l.index).ok().filter(|&ix| ix < b.count) else {
        return "the info is gone (index out of range)".to_owned();
    };
    let mut r = [0u8; INFO_STRIDE];
    if !mem.bytes(b.infos + INFO_STRIDE * ix, &mut r) || le_u32(&r, INFO_INDEX) as i32 != l.index {
        return "the info is gone (unreadable or another index)".to_owned();
    }
    let view = if l.known { doors::object_view(b.addr, l.slot, l.index, l.handle, l.state) } else { None };
    format!(
        "run state {}, info+113/+114 = {}/{}, flag {}, object {}",
        le_u32(&r, INFO_RUN_STATE),
        r[INFO_B113],
        r[INFO_B114],
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
    /// (vt RVA, run, b114, sub).
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
        Info { addr, index: 0, sub: 0, run: 0, b113: 0, b114: 0, state: 0, primary, handle: 0 }
    }

    fn here() -> Here {
        Here { executing: false, idle: Some(true), marked: false, replayed: false, executed: false }
    }

    #[test]
    fn wire_round_trip_and_rejects() {
        let m = Msg { map: 0x0B00_0000, index: 17, sub: 2, param: -1, class: 0, state: 1, seq: 9 };
        let d = encode(&m);
        assert_eq!(d.len(), WIRE_LEN);
        assert_eq!(decode(&d), Some(m));
        assert_eq!(decode(&d[..WIRE_LEN - 1]), None);
        let mut bad = d.clone();
        bad[0] = 0;
        assert_eq!(decode(&bad), None);
        let mut other = d;
        other[1] = 2;
        assert_eq!(decode(&other), None);
    }

    #[test]
    fn walk_reads_map_id_and_infos() {
        let m = world(0x0B00_0000, 11, 2, &[(0x298B570, 0, 0, 0), (0x298B628, 3, 1, 1)]);
        let bs = blocks(&m, BASE);
        assert_eq!(bs, vec![Block { addr: BLOCKS, slot: 3, map: Some(0x0B00_0000), infos: INFOS, count: 2 }]);
        let all = infos(&m, &bs[0], &mut Vec::new());
        assert_eq!(all.len(), 2);
        assert_eq!((all[1].index, all[1].sub, all[1].run, all[1].b114, all[1].handle), (1, 1, 3, 1, 0x101));
        assert_eq!(class_of(&m, BASE, all[0].state), Some(Class::One));
        assert_eq!(class_of(&m, BASE, all[1].state), Some(Class::Door));
        assert_eq!(flag_of(&m, all[0].state, Class::One), None);
        assert_eq!(flag_of(&m, all[1].state, Class::Door), Some(11_000_001));
        assert_eq!(find(&m, BASE, 0x0B00_0000, 1, 1).map(|(_, i)| i.addr), Some(INFOS + INFO_STRIDE));
        assert!(find(&m, BASE, 0x0B00_0000, 1, 0).is_none());
        assert!(find(&m, BASE, 0x0C00_0000, 1, 1).is_none());
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
    fn edges() {
        assert!(is_edge(0, 3) && is_edge(4, 3));
        assert!(!is_edge(3, 3) && !is_edge(0, 0) && !is_edge(3, 0));
    }

    #[test]
    fn echo_of_our_replay_is_never_sent() {
        let e = Evidence { our_replay: true, b114_rose: true, b114_now: true, own_anim: true };
        assert!(send_decision(e, Some(Class::One), 1).is_err());
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
    fn send_needs_more_than_our_animation() {
        let ev = |b114_rose, b114_now, own_anim| Evidence { our_replay: false, b114_rose, b114_now, own_anim };
        // L1: a foreign execution while we play an ObjAct animation (no +114) is not sent.
        assert!(send_decision(ev(false, false, true), Some(Class::One), 1).is_err());
        // Sticky +114 alone is not ours.
        assert!(send_decision(ev(false, true, false), Some(Class::One), 1).is_err());
        assert!(send_decision(ev(false, true, true), Some(Class::One), 1).is_ok());
        assert!(send_decision(ev(true, true, false), Some(Class::Door), 1).is_ok());
        assert!(send_decision(ev(false, false, false), Some(Class::One), 1).is_err());
        // One-loop only with mode 2; unknown class never.
        assert!(send_decision(ev(true, true, true), Some(Class::OneLoop), 1).is_err());
        assert!(send_decision(ev(true, true, true), Some(Class::OneLoop), 2).is_ok());
        assert!(send_decision(ev(true, true, true), None, 2).is_err());
    }

    #[test]
    fn doors_rs_mark_suppresses_our_replay() {
        let h = Here { marked: true, ..here() };
        assert!(replay_gate(Some(Class::Door), 1, 1, h).is_err());
        assert_eq!(replay_gate(Some(Class::Door), 1, 1, here()), Ok(()));
    }

    #[test]
    fn state_is_validated() {
        assert!(replay_gate(Some(Class::One), 2, 1, here()).is_err());
        assert!(replay_gate(Some(Class::One), 255, 2, here()).is_err());
        assert!(replay_gate(Some(Class::One), 0, 1, here()).is_err());
        assert_eq!(replay_gate(Some(Class::One), 1, 1, here()), Ok(()));
        assert!(replay_gate(Some(Class::Door), 0, 2, here()).is_err());
        assert_eq!(replay_gate(Some(Class::OneLoop), 0, 2, here()), Ok(()));
    }

    #[test]
    fn replay_gate_and_object() {
        assert!(replay_gate(Some(Class::One), 1, 0, here()).is_err());
        assert!(replay_gate(None, 1, 2, here()).is_err());
        assert!(replay_gate(Some(Class::OneLoop), 1, 1, here()).is_err());
        assert!(replay_gate(Some(Class::One), 1, 1, Here { executing: true, ..here() }).is_err());
        assert!(replay_gate(Some(Class::One), 1, 1, Here { replayed: true, ..here() }).is_err());
        assert!(replay_gate(Some(Class::One), 1, 1, Here { executed: true, ..here() }).is_err());
        assert!(replay_gate(Some(Class::One), 1, 1, Here { idle: Some(false), ..here() }).is_err());
        assert!(replay_gate(Some(Class::One), 1, 1, Here { idle: None, ..here() }).is_err());
        assert_eq!(object_decision(Some(false)), Ok(()));
        assert!(object_decision(Some(true)).is_err() && object_decision(None).is_err());
    }

    #[test]
    fn names() {
        assert_eq!(map_name(Some(0x0B01_0000)), "m11_01_00_00 (0x0b010000)");
        assert_eq!(class_name(class_code(Some(Class::OneDirDoor))), "one-dir-door");
        assert!(logged_flag(61_100_610) && logged_flag(11_000_670) && !logged_flag(9100));
    }
}
