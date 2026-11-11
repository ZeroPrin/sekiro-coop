//! Doors (ObjAct) stay open for both players (2026-10-24, NOT verified live).
//!
//! Live 7.4 (2026-10-24 01:30-02:05): a door one player opened ended up closed for the
//! other, and opening it there did not fix it. Root cause from IDA (copy
//! `F:\Temp\ida_world`, jobs w1-w6):
//! - the player's own ObjAct (`0x6BA7B0` -> `0x6BC1B0` with `info+114 = 1`) does not SET
//!   the door's event flag, it TOGGLES it: state vtable slot 4 (`0x6B6470`, door and
//!   one-way door; `0x6B64A0` one-loop) is `SetFlag(f, GetFlag(f) == 0)`, `f = state+0x10`;
//! - a flag that arrives from the friend (world.rs, SetEventFlag) changes only the flag;
//!   the door object stays closed until the block is loaded again (the ObjAct infos are
//!   set up from the flags at load);
//! - so: the friend opens door D (flag on, sent here, our door still closed); we open it
//!   too (our door opens, the toggle switches the flag OFF, sent to the friend); after
//!   our next load D is closed here, while the friend's door stays open until it reloads.
//!
//! The game's own replay of a friend's ObjAct (type 45, `0x6BE740` -> `0x6BA910(block,
//! index, actor, state)`) sets the flag to the given state (slot 5, no toggle) and plays
//! the object's animation. This module does the same from the flags we already sync:
//! 1. a door flag the friend switched ON whose door is loaded here and still closed is
//!    replayed with `0x6BA910(block, index, -1, 1)`;
//! 2. guard: our own write switching OFF a door flag the friend switched on this session
//!    is put back ON at once without telling the friend (world.rs `write_quiet`);
//! 3. guard (receiver): a friend's OFF for a known door flag that is ON here is ignored
//!    (vanilla doors never close again; world.rs `apply_flag`);
//! 4. after every rest and every loading screen all loaded doors whose flag is on are
//!    checked (twice each);
//! 5. guard (one-loop doors, the MSB flags 61xxxxxx/62xxxxxx that 1-3 never see): our own
//!    player's start of a one-loop door whose flag was ON toggled it OFF (objact.rs's detour
//!    notes it): put back ON at once without telling the friend. The flag of a friend's
//!    one-loop door that switched ON is replayed as in 1 (objact.rs usually did it already).
//!
//! "Known door flag": the flag of a flag-bearing ObjAct state found by the walker, or,
//! as a fallback for ObjActs whose state keeps no flag (`SprjObjActOneState`, flag
//! driven by EMEVD instead), a flag the friend switched ON right after its character
//! played an ObjAct player animation (`ObjActParam.playerAnimId`, like ownworld.rs).
//! Guards 2/3 stay off for a while after a rest (rest-reset flags are no toggles).
//!
//! Layout, re-derived 2026-10-24 (copy `F:\Temp\ida_doors`, jobs d1-d7) — [VAR] = read in
//! the IDA output, [ARV] = inferred:
//! - `*(base+0x3D55EC8)` is the `FD4Singleton<SprjWorldObjActMan>` instance: `0x6B8400`
//!   and `0x810BB0` pass it straight to its update `0x6BE1B0` -> `0x6BE740` [VAR];
//! - manager `+0x28` i32 block count, `+0x30` inline array of 224-byte blocks
//!   (`0x6BE740`: `*(m+0x30) + 224*slot`, `slot < *(m+0x28)`) [VAR];
//! - block `+0x00` descriptor (`+0x20` = block slot, `0x6BA910`), `+0x10` u8 loaded
//!   (`0x6BC0A0` sets 1 after the setup `0x6B9FE0`, 0 after the unload `0x6BC8E0`),
//!   `+0x78` infos, `+0x80` i32 count, 304 bytes each (`0x6B9D10`) [VAR];
//! - info (ctor `0x6B02C0`, setup `0x6B2430`): `+0x08` own list index ("MyListIdx", =
//!   array index, `0x6B9FE0` passes `v30+v25`), `+0x50` object handle ("ObjHandle"; the
//!   argument of `0x899310`), `+0x54` run state, `+0x88` state object, `+0xA9` u8 state
//!   type, `+0xAC` i32 sub index, `+0xB0` primary info [VAR]. A type-1 door has FOUR
//!   infos (sub 0..3, same flag; `0x6B9FE0` links them, primary = sub 0) [VAR];
//! - state factory `0x6B64E0(info, type)`: 0 `SprjObjActOneState` (vt `0x298B570`, no
//!   flag; slot 16 is a nullsub), 1 `SprjObjActDoorState` (`0x298B628`), 2
//!   `SprjObjActOneLoopState` (`0x298B6E0`), 4 `SprjObjActOneDirOpenDoorState`
//!   (`0x298B798`); 1/2/4 keep the MSB EventFlagID at `state+0x10` (slot 16 `0x6B6E80` /
//!   `0x6B6E90` stores it) [VAR].
//! The old walker read this correctly but only accepted types 1/4 and every sub info.
//! It found 0 doors in 32/32 live checks, so the doors seen live are most likely
//! `OneState` ObjActs (no flag in the state) [ARV]: the first scan now logs a count per
//! state class to settle it.
//! Live 2026-11-12 (m11_00..m13_00): one-state 22, door 0, one-loop 10, one-dir 0. The ten
//! one-loop flags are the bar doors, gates and windows of the MSB table below.
//!
//! ## What an ObjAct is: `kind` (by ObjActParam id), 2026-11-12
//!
//! Data: `E:\RE\sekiro-data\objacts.md` (ObjActParam csv + every MSB ObjAct event, 81, with
//! its stateType and EMEVD users). In Sekiro the state class says nothing about door vs
//! lever: every bar door and gate is a ONE-LOOP state (stateType 2), the sliding / HUB doors
//! and chests are one-state, and so are the temple lift levers. The ObjActParam row does:
//!
//! | ObjActParam | rows (name) | MSB stateType | kind |
//! |---|---|---|---|
//! | 999900-999976 | "扉を開く" open the door (gates 9999 00/01, small 10/11, large 20-22, 1st 40-46, kido 50, middle 60, sliding 70/71, prison 75, loophole 76) | 2 or 0 | Door |
//! | 101050 | お堂の扉, temple door | 2 | Door |
//! | 999980 / 999985 / 999990 | 宝箱を開く, chests | 0 | Chest |
//! | 205010 / 205011, -1 | 寺 エレベーター レバー, the temple lift levers | 0 | Other |
//! | 205100 / 205101 / 252555, -1 | bells | 0 / 2 | Other |
//! | 90070-90076 | どんでん返し, rotating walls | 0 | Other |
//! | 90060-90066 | どんでん床, rotating floors | 2 | Other |
//! | 1100000 | 窓, the meditation room windows (61110703 / 61110705, EMEVD 11115614 shuts them) | 2 | Other |
//! | 109950 / -1 | hidden tatami | 2 | Other |
//!
//! The door rows all use a door player animation (700200-700330) and, where an EMEVD uses
//! them, the door templates 20005615 / 20005616 (or a plot `ForceUseObjact`); levers, bells,
//! walls, floors and windows have their own animations (7104xx-7108xx, 7110xx) and no door
//! template. `sync_objact = 1` syncs Door and Chest of any state class; Other only with 2.
//!
//! One-loop doors [VAR, Ghidra copy `E:\RE\ghidra_doors2`]: `vt[4]` `0x6B64A0` toggles the
//! flag (`SetEventFlag(f, !f)`) on every local start, `vt[5]` `0x6B6810` sets it to the value,
//! and the animation the request asks for, `0x6B1DE0` = `vt[8]` + `row.objAnimId`, has
//! `vt[8]` = `0x6B6B40` = 0: EVERY execution plays the same opening animation, there is no
//! "close" animation. The flag is a use parity; vanilla never uses one twice because
//! 20005616 disables the ObjAct after its `ObjActEventFlag` and the load restores the object
//! from the flag (20005616 does not `ReproduceObjectAnimation`, 20005615 for flagless
//! one-state doors does) [ARV]. So for a one-loop door "open" = flag ON, and a player who
//! uses a door whose flag is already on (the friend's flag arrived, the object did not move)
//! switches it OFF: after the next load the door is closed and its ObjAct disabled (live
//! 2026-11-12, flag 61110602). Guard 5 below undoes that toggle.

use std::collections::HashSet;
use std::sync::{Mutex, OnceLock};
use std::time::{Duration, Instant};

use crate::game::{self, read, read_ptr};
use crate::log;

pub(crate) const WORLD_OBJACT_MAN: usize = 0x3D55EC8;
pub(crate) const OBJ_MAN: usize = 0x3D5D600;
pub(crate) const MAN_BLOCK_COUNT: usize = 0x28;
pub(crate) const MAN_BLOCKS: usize = 0x30;
pub(crate) const BLOCK_STRIDE: usize = 224;
pub(crate) const BLOCK_LOADED: usize = 0x10;
pub(crate) const BLOCK_INFOS: usize = 0x78;
pub(crate) const BLOCK_INFO_COUNT: usize = 0x80;
pub(crate) const BLOCK_DESC_SLOT: usize = 0x20;
pub(crate) const INFO_STRIDE: usize = 304;
pub(crate) const INFO_INDEX: usize = 0x08;
pub(crate) const INFO_HANDLE: usize = 0x50;
pub(crate) const INFO_RUN_STATE: usize = 0x54;
pub(crate) const INFO_STATE: usize = 0x88;
pub(crate) const INFO_SUB: usize = 0xAC;
/// ObjActParam id of the info [VAR, objact.rs header].
pub(crate) const INFO_PARAM_ID: usize = 0x78;
pub(crate) const STATE_FLAG: usize = 0x10;
/// State vtables (RVA) and their class.
pub(crate) const STATE_VTS: [(usize, Class); 4] = [
    (0x298B570, Class::One),
    (0x298B628, Class::Door),
    (0x298B6E0, Class::OneLoop),
    (0x298B798, Class::OneDirDoor),
];
const OBJ_ANIM: usize = 0x138;
const OBJ_ANIM_FLAGS: usize = 0x128;
/// `0x6BA910(block, index, actor handle, state)`: the game's replay of a friend's ObjAct.
pub(crate) const REPLAY_RVA: usize = 0x6BA910;
/// `0x6BCBC0(block, req, -1, slot, index, actor, state, 0)`: request builder (pure).
pub(crate) const REQUEST_RVA: usize = 0x6BCBC0;
/// `0x899310(ObjMan, handle) -> object`.
pub(crate) const OBJ_BY_HANDLE_RVA: usize = 0x899310;
/// Run state "executing" (`0x6B31E0` sets 3).
pub(crate) const RUN_EXECUTING: u32 = 3;
/// A friend door flag is replayed this long after it was applied.
const REPLAY_DELAY: Duration = Duration::from_millis(300);
/// Doors are checked this long after a rest (twice, see restsync.rs).
const AFTER_REST: [Duration; 2] = [Duration::from_millis(2500), Duration::from_millis(7500)];
/// ...and this long after every loading screen (`in_world_for`; `tick` only runs once the
/// world settled, 5 s). Issue #2 (2026-10-31): after the Isshin wipe reload a door by the
/// idol was closed with no "open" prompt; doors were only checked after rests.
const AFTER_LOAD: [Duration; 2] = [Duration::from_secs(6), Duration::from_secs(12)];
/// A pending door flag that never finds a loaded closed door is dropped after this.
const PENDING_FOR: Duration = Duration::from_secs(5);
/// Fallback: a friend ON flag counts as a door flag when it arrives this long after the
/// friend's ObjAct animation started (the flag is written during / at the end of it)...
const OBJACT_FLAG_AFTER: Duration = Duration::from_secs(6);
/// ...or at most this long before the animation shows up here (snapshot vs reliable event).
const OBJACT_FLAG_BEFORE: Duration = Duration::from_millis(1500);
/// Guards 2/3 are off this long after any rest (rest-reset flags go off legitimately).
const REST_QUIET: Duration = Duration::from_secs(15);
/// Door lines per load or rest. A lifetime cap of 60 was used up after 3 h in issue #2's
/// logs and hid every door event of the last 4 h.
const MAX_LOG: u32 = 20;
/// Sanity caps for the walk (garbage pointers must not make it loop forever).
pub(crate) const MAX_BLOCKS: u32 = 64;
pub(crate) const MAX_INFOS: i32 = 4096;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Class {
    One,
    Door,
    OneLoop,
    OneDirDoor,
}

impl Class {
    pub(crate) fn has_flag(self) -> bool {
        self != Class::One
    }
}

/// What an ObjAct is, by its ObjActParam id (table in the header).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Kind {
    Door,
    Chest,
    /// Levers, lifts, bells, rotating walls / floors, windows, hidden tatami, unknown rows.
    Other,
}

pub(crate) fn kind(param: i32) -> Kind {
    match param {
        999_900..=999_979 | 101_050 => Kind::Door,
        999_980..=999_999 => Kind::Chest,
        _ => Kind::Other,
    }
}

/// A one-loop door: every execution opens it, its flag ON = open (header).
pub(crate) fn loop_door(class: Option<Class>, param: i32) -> bool {
    class == Some(Class::OneLoop) && kind(param) == Kind::Door
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct Door {
    block: usize,
    slot: i32,
    index: i32,
    flag: u32,
    handle: u32,
    running: bool,
    class: Class,
    param: i32,
}

impl Door {
    fn loop_door(&self) -> bool {
        loop_door(Some(self.class), self.param)
    }
}

/// What one walk saw (diagnostics).
#[derive(Clone, Debug, Default, PartialEq, Eq)]
struct Report {
    man: usize,
    blocks: u32,
    loaded: u32,
    /// Loaded blocks whose descriptor / info array could not be read.
    bad_blocks: u32,
    infos: u32,
    no_state: u32,
    one: u32,
    door: u32,
    one_loop: u32,
    one_dir: u32,
    other: u32,
    /// Flag-bearing infos skipped: sub index != 0 (door sub infos).
    subs: u32,
    /// Flag-bearing primaries skipped: flag 0 / -1 or index mismatch.
    bad: u32,
    /// The step that ended the walk early.
    fail: Option<&'static str>,
}

/// Fault-safe memory reads (the game, or a byte snapshot in the tests).
pub(crate) trait Mem {
    fn bytes(&self, addr: usize, out: &mut [u8]) -> bool;
    fn u8(&self, a: usize) -> Option<u8> {
        let mut b = [0u8; 1];
        self.bytes(a, &mut b).then_some(b[0])
    }
    fn u32(&self, a: usize) -> Option<u32> {
        let mut b = [0u8; 4];
        self.bytes(a, &mut b).then(|| u32::from_le_bytes(b))
    }
    fn i32(&self, a: usize) -> Option<i32> {
        self.u32(a).map(|v| v as i32)
    }
    fn ptr(&self, a: usize) -> Option<usize> {
        let mut b = [0u8; 8];
        self.bytes(a, &mut b).then(|| u64::from_le_bytes(b) as usize).filter(|&p| p >= 0x10000)
    }
}

pub(crate) struct GameMem;

impl Mem for GameMem {
    fn bytes(&self, addr: usize, out: &mut [u8]) -> bool {
        game::read_bytes(addr, out)
    }
}

pub(crate) fn le_u32(b: &[u8], off: usize) -> u32 {
    u32::from_le_bytes(b[off..off + 4].try_into().unwrap())
}

pub(crate) fn le_ptr(b: &[u8], off: usize) -> usize {
    u64::from_le_bytes(b[off..off + 8].try_into().unwrap()) as usize
}

/// Walks WorldObjActMan (`base` = module base): every primary info of a loaded block
/// whose state keeps a flag.
fn scan(mem: &impl Mem, base: usize) -> (Vec<Door>, Report) {
    let mut out = Vec::new();
    let mut r = Report::default();
    let Some(man) = mem.ptr(base + WORLD_OBJACT_MAN) else {
        r.fail = Some("WorldObjActMan pointer (base+0x3D55EC8) is null");
        return (out, r);
    };
    r.man = man;
    let (Some(blocks), Some(count)) = (mem.ptr(man + MAN_BLOCKS), mem.u32(man + MAN_BLOCK_COUNT)) else {
        r.fail = Some("block array (+0x30) / count (+0x28) unreadable");
        return (out, r);
    };
    r.blocks = count;
    if count == 0 || count > MAX_BLOCKS {
        r.fail = Some("block count (+0x28) is 0 or out of range");
        return (out, r);
    }
    let mut info = [0u8; INFO_STRIDE];
    for b in 0..count as usize {
        let block = blocks + BLOCK_STRIDE * b;
        if mem.u8(block + BLOCK_LOADED).unwrap_or(0) == 0 {
            continue;
        }
        r.loaded += 1;
        let slot = mem.ptr(block).and_then(|d| mem.i32(d + BLOCK_DESC_SLOT));
        let infos = mem.ptr(block + BLOCK_INFOS);
        let n = mem.i32(block + BLOCK_INFO_COUNT);
        let (Some(slot), Some(infos), Some(n)) = (slot, infos, n) else {
            r.bad_blocks += 1;
            continue;
        };
        for i in 0..n.clamp(0, MAX_INFOS) as usize {
            let at = infos + INFO_STRIDE * i;
            if !mem.bytes(at, &mut info) {
                r.bad += 1;
                continue;
            }
            r.infos += 1;
            let state = le_ptr(&info, INFO_STATE);
            let class = if state < 0x10000 {
                None
            } else {
                mem.ptr(state).and_then(|vt| STATE_VTS.iter().find(|(rva, _)| base + rva == vt).map(|&(_, c)| c))
            };
            match class {
                None if state < 0x10000 => r.no_state += 1,
                None => r.other += 1,
                Some(Class::One) => r.one += 1,
                Some(Class::Door) => r.door += 1,
                Some(Class::OneLoop) => r.one_loop += 1,
                Some(Class::OneDirDoor) => r.one_dir += 1,
            }
            let Some(class) = class.filter(|c| c.has_flag()) else {
                continue;
            };
            if le_u32(&info, INFO_SUB) != 0 {
                r.subs += 1;
                continue;
            }
            let index = le_u32(&info, INFO_INDEX) as i32;
            let flag = mem.u32(state + STATE_FLAG).unwrap_or(0);
            if flag == u32::MAX || flag == 0 || index != i as i32 {
                r.bad += 1;
                continue;
            }
            out.push(Door {
                block,
                slot,
                index,
                flag,
                handle: le_u32(&info, INFO_HANDLE),
                running: le_u32(&info, INFO_RUN_STATE) == RUN_EXECUTING,
                class,
                param: le_u32(&info, INFO_PARAM_ID) as i32,
            });
        }
    }
    if r.loaded == 0 {
        r.fail = Some("no loaded block (+0x10)");
    }
    (out, r)
}

// ------------------------------------------------------------------ pure decisions

/// The friend's ObjAct animation (started at `anim`) and its ON flag (arrived at `flag`)
/// belong together.
fn objact_near(anim: Option<Instant>, flag: Instant) -> bool {
    anim.is_some_and(|a| {
        if flag >= a {
            flag.duration_since(a) <= OBJACT_FLAG_AFTER
        } else {
            a.duration_since(flag) <= OBJACT_FLAG_BEFORE
        }
    })
}

/// Guard 2: our own write switched `id` OFF; put it back on?
fn keep_open_decision(friend_on: bool, known_door: bool, rest_quiet: bool) -> bool {
    friend_on && known_door && !rest_quiet
}

/// Guard 3: the friend sent OFF for `id`; ignore it?
fn ignore_off_decision(on: bool, here_on: Option<bool>, known_door: bool, rest_quiet: bool) -> bool {
    !on && here_on == Some(true) && known_door && !rest_quiet
}

/// Guard 5: our own write switched `id` OFF; put it back on? Only a one-loop door's flag that
/// our own player's start toggled off (it was on: the door opened, nothing closed).
pub(crate) fn keep_open_loop_decision(loop_door: bool, own_toggle_off: bool) -> bool {
    loop_door && own_toggle_off
}

/// Door flags are map flags (group 1 / 2) [ARV from the live ids].
pub(crate) fn map_flag(id: u32) -> bool {
    matches!(id / 10_000_000 % 10, 1 | 2)
}

/// An animation is an ObjAct player animation (full id or id within its category, as
/// ownworld.rs `objact_anim`).
pub(crate) fn is_objact_anim(anim: i32, set: &HashSet<u32>) -> bool {
    anim > 0 && (set.contains(&(anim as u32)) || set.contains(&(anim as u32 % 1_000_000)))
}

// ------------------------------------------------------------------ state

#[derive(Default)]
struct State {
    /// Door flags the friend switched ON this session (guard 2).
    friend_on: HashSet<u32>,
    /// Flags of flag-bearing ObjAct states seen by any walk (static game data).
    door_flags: HashSet<u32>,
    /// The subset that belongs to one-loop doors (`loop_door`).
    loop_doors: HashSet<u32>,
    /// Guard 5 writes so far.
    loop_kept_on: u32,
    /// Fallback door flags: friend ON right after its ObjAct animation.
    candidates: HashSet<u32>,
    /// Friend door flags waiting for the replay (flag, applied at).
    pending: Vec<(u32, Instant)>,
    /// Friend ON map flags of the last seconds (for an ObjAct animation seen late).
    recent_on: Vec<(u32, Instant)>,
    /// Each peer's last seen TimeAct id (slot -> anim).
    peer_anim: std::collections::BTreeMap<u8, i32>,
    friend_objact_at: Option<Instant>,
    /// A new load was seen: walk once on the next tick.
    scan_due: bool,
    last_in_world: Duration,
    rest_seen: Option<Instant>,
    rest_checks: usize,
    /// Checks done since the last loading screen (`AFTER_LOAD`).
    load_checks: usize,
    replays: u32,
    kept_on: u32,
    ignored_off: u32,
    logged: u32,
    logged_scan: bool,
    logged_doors: bool,
    fails_logged: Vec<&'static str>,
}

static STATE: Mutex<Option<State>> = Mutex::new(None);

fn with<R>(f: impl FnOnce(&mut State) -> R) -> R {
    let mut g = STATE.lock().unwrap_or_else(|e| e.into_inner());
    f(g.get_or_insert_with(State::default))
}

/// Guard 5: one-loop door flags our own player's start toggled from ON to OFF (objact.rs's
/// `0x6BA7B0` detour, game thread), and when. Its own lock: the detour may run while `STATE`
/// is held further up the stack.
static OWN_LOOP_OFF: Mutex<Vec<(u32, Instant)>> = Mutex::new(Vec::new());
/// world.rs sees the write in the same or the next frames (ring) or at its next flag diff.
const OWN_LOOP_OFF_FOR: Duration = Duration::from_secs(30);

/// objact.rs's detour: our player started one-loop door `flag` and it went ON -> OFF.
pub(crate) fn note_own_loop_toggle(flag: u32) {
    let now = Instant::now();
    let mut v = OWN_LOOP_OFF.lock().unwrap_or_else(|e| e.into_inner());
    v.retain(|&(f, t)| f != flag && now.duration_since(t) < OWN_LOOP_OFF_FOR);
    if v.len() < 32 {
        v.push((flag, now));
    }
}

/// Takes `flag`'s note (one write back per toggle).
fn take_own_loop_toggle(flag: u32, now: Instant) -> bool {
    let mut v = OWN_LOOP_OFF.lock().unwrap_or_else(|e| e.into_inner());
    let hit = v.iter().any(|&(f, t)| f == flag && now.duration_since(t) < OWN_LOOP_OFF_FOR);
    v.retain(|&(f, t)| f != flag && now.duration_since(t) < OWN_LOOP_OFF_FOR);
    hit
}

fn note_log(s: &mut State, line: impl FnOnce() -> String) {
    if s.logged < MAX_LOG {
        s.logged += 1;
        log::line(&line());
    }
}

fn rest_quiet(now: Instant) -> bool {
    crate::restsync::last_rest().is_some_and(|t| now.duration_since(t) < REST_QUIET)
}

fn known_door(s: &State, id: u32) -> bool {
    s.door_flags.contains(&id) || s.candidates.contains(&id)
}

/// `ObjActParam.playerAnimId` of every row (once the params are loaded).
pub(crate) fn objact_anims() -> Option<&'static HashSet<u32>> {
    use fromsoftware_shared::FromStatic;
    use sekiro::param::{OBJ_ACT_PARAM_ST, ParamDef};
    use sekiro::sprj::SoloParamRepository;
    static ANIMS: OnceLock<HashSet<u32>> = OnceLock::new();
    if let Some(a) = ANIMS.get() {
        return Some(a);
    }
    let repo = unsafe { SoloParamRepository::instance() }.ok()?;
    let file = repo.params().find(|p| p.struct_name() == OBJ_ACT_PARAM_ST::NAME).map(|p| &*p.data)?;
    let set: HashSet<u32> = unsafe { file.rows::<OBJ_ACT_PARAM_ST>() }
        .map(|(_, r)| r.player_anim_id())
        .filter(|&a| a > 0 && a != u32::MAX)
        .collect();
    if set.is_empty() {
        return None;
    }
    Some(ANIMS.get_or_init(|| set))
}

/// Walks the game's ObjAct infos; remembers the door flags and logs the diagnostics
/// (first scan with a loaded block, first scan with doors, each failure step once).
fn doors() -> Vec<Door> {
    let (all, r) = scan(&GameMem, game::module_base());
    with(|s| {
        s.door_flags.extend(all.iter().map(|d| d.flag));
        s.loop_doors.extend(all.iter().filter(|d| d.loop_door()).map(|d| d.flag));
        if let Some(step) = r.fail {
            if !s.fails_logged.contains(&step) && s.fails_logged.len() < 8 {
                s.fails_logged.push(step);
                log::line(&format!("doors: scan failed: {step} (manager {:#x}, {} blocks, {} loaded)", r.man, r.blocks, r.loaded));
            }
            return;
        }
        if !s.logged_scan || (!s.logged_doors && !all.is_empty()) {
            s.logged_scan = true;
            s.logged_doors = !all.is_empty();
            log::line(&format!(
                "doors: scan: WorldObjActMan {:#x}, {} blocks, {} loaded ({} unreadable), {} ObjAct infos (state: one-state {}, door {}, one-loop {}, one-dir-door {}, none {}, other {}); {} door sub infos skipped, {} bad; {} doors with a flag {:?}",
                r.man,
                r.blocks,
                r.loaded,
                r.bad_blocks,
                r.infos,
                r.one,
                r.door,
                r.one_loop,
                r.one_dir,
                r.no_state,
                r.other,
                r.subs,
                r.bad,
                all.len(),
                all.iter().map(|d| d.flag).take(12).collect::<Vec<_>>()
            ));
        }
    });
    all
}

/// What the map object of an ObjAct info shows now, against what an execution with `state`
/// would play (objact.rs logs it and dedups with it).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct ObjView {
    /// The map object (`0x899310(ObjMan, info+0x50)`).
    pub obj: usize,
    /// Its animation now (`obj+0x138`) and the "playing" bit (`obj+0x128 & 4`).
    pub anim: u32,
    pub playing: bool,
    /// The animation the execution would play (request builder `0x6BCBC0`, `req[6]`).
    pub want: u32,
}

impl ObjView {
    /// The object already shows the execution's animation (the game's own check `0x8777F0`).
    pub(crate) fn done(&self) -> bool {
        self.anim == self.want && self.playing
    }

    /// The object is at that animation, playing or finished (objact.rs: a door that already
    /// opened here keeps its last animation, `playing` goes false when it ends) [ARV].
    pub(crate) fn reached(&self) -> bool {
        self.anim == self.want
    }
}

/// The game thread only (calls the request builder and `0x899310`).
pub(crate) fn object_view(block: usize, slot: i32, index: i32, handle: u32, state: u32) -> Option<ObjView> {
    let base = game::module_base();
    let mut req = [0u32; 16];
    let build: extern "C" fn(usize, *mut u32, i32, i32, i32, i32, u32, u8) -> u8 = unsafe { std::mem::transmute(base + REQUEST_RVA) };
    if build(block, req.as_mut_ptr(), -1, slot, index, -1, state, 0) == 0 {
        return None;
    }
    let objs = read_ptr(base + OBJ_MAN)?;
    let get: extern "C" fn(usize, u32) -> usize = unsafe { std::mem::transmute(base + OBJ_BY_HANDLE_RVA) };
    let obj = get(objs, handle);
    if obj < 0x10000 {
        return None;
    }
    let anim = read::<u32>(obj + OBJ_ANIM)?;
    let flags = read::<u8>(obj + OBJ_ANIM_FLAGS)?;
    Some(ObjView { obj, anim, playing: flags & 4 != 0, want: req[6] })
}

/// The object of `d` already shows the animation the "opened" execution would play. A
/// one-loop door counts once it is at that animation, playing or ended (`reached`: every
/// execution plays it, a door loaded open from its flag keeps it) [ARV]; the others need it
/// playing (`done`, the game's own check), as before.
fn open_decision(loop_door: bool, v: &ObjView) -> bool {
    if loop_door { v.reached() } else { v.done() }
}

/// Only doors and chests (by ObjActParam) are ever replayed from their flag. Windows,
/// rotating floors, bells and hidden tatami (one-loop, `Kind::Other`) keep what the game and
/// their EMEVD do: test2 replayed the meditation room window 61110703 after every load and
/// rest (live 2026-11-12), and m11_01 11115614 shuts those windows in phase 8302.
fn reopens(param: i32) -> bool {
    kind(param) != Kind::Other
}

/// Opens `d` like the game replays a friend's ObjAct, unless it is open or running.
fn open_if_closed(s: &mut State, d: &Door, why: &str) -> bool {
    if d.running || !reopens(d.param) || !crate::game::event_flag(d.flag).unwrap_or(false) {
        return false;
    }
    let view = object_view(d.block, d.slot, d.index, d.handle, 1);
    match view.map(|v| open_decision(d.loop_door(), &v)) {
        Some(false) => {}
        Some(true) => return false,
        None => {
            note_log(s, || format!("doors: door flag {} (block slot {}, index {}): object not found here, not replayed", d.flag, d.slot, d.index));
            return false;
        }
    }
    let seen = view.map_or(String::new(), |v| format!("; object anim {} playing {}, the open execution plays {}", v.anim, v.playing, v.want));
    // objact.rs replayed this ObjAct a moment ago (the friend's 0xA7): no second 0x6BA910.
    if crate::objact::replayed_recently(d.block, d.index) {
        return false;
    }
    // objact.rs must not send this execution to the friend as one of ours.
    crate::objact::note_replay(d.block, d.index);
    let replay: extern "C" fn(usize, i32, i32, u32) -> u8 = unsafe { std::mem::transmute(game::module_base() + REPLAY_RVA) };
    let ok = replay(d.block, d.index, -1, 1) != 0;
    // The EMEVD waits for ObjActEventFlag, which a replay never sets (objact.rs header).
    crate::objact::kick_after_replay(d.block, d.index, "doors.rs");
    s.replays += 1;
    let n = s.replays;
    note_log(s, || {
        format!(
            "doors: {why}: door flag {} is on but its door (handle {:#x}, block slot {}, index {}, {}ObjActParam {}) was closed here: opened (0x6BA910 ok = {ok}, {} so far){seen}",
            d.flag,
            d.handle,
            d.slot,
            d.index,
            if d.loop_door() { "one-loop door, " } else { "" },
            d.param,
            n
        )
    });
    ok
}

fn add_candidate(s: &mut State, id: u32, why: &str) {
    if s.door_flags.contains(&id) || s.candidates.len() >= 512 || !s.candidates.insert(id) {
        return;
    }
    s.friend_on.insert(id);
    note_log(s, || format!("doors: flag {id} counts as a door flag ({why}): never switched off again by a toggle or the friend"));
}

/// world.rs: a friend's flag was applied here.
pub fn friend_flag(id: u32, on: bool) {
    if !on {
        return;
    }
    let now = Instant::now();
    with(|s| {
        // A one-loop door's MSB flag (guard 5): replayed like a map door flag when its door is
        // loaded and closed here; no candidates, no guard 2 / 3 for it.
        if !map_flag(id) {
            if s.loop_doors.contains(&id) && s.pending.len() < 256 && !s.pending.iter().any(|(f, _)| *f == id) {
                s.pending.push((id, now));
            }
            return;
        }
        if s.door_flags.contains(&id) {
            s.friend_on.insert(id);
        }
        if objact_near(s.friend_objact_at, now) {
            add_candidate(s, id, "the friend set it right after an ObjAct animation");
        }
        s.recent_on.retain(|(_, t)| now.duration_since(*t) <= OBJACT_FLAG_BEFORE);
        if s.recent_on.len() < 64 {
            s.recent_on.push((id, now));
        }
        if s.pending.len() < 256 && !s.pending.iter().any(|(f, _)| *f == id) {
            s.pending.push((id, now));
        }
    });
}

/// world.rs, our own write of `id` switching it OFF while it was on: true = it is a door
/// flag the friend opened this session, and our ObjAct toggle just closed it (guard 2):
/// the caller writes it back on quietly.
pub fn keep_open(id: u32) -> bool {
    let now = Instant::now();
    let quiet = rest_quiet(now);
    let own_toggle = take_own_loop_toggle(id, now);
    with(|s| {
        // The detour notes only a start whose info is a one-loop door (`loop_door` of its state
        // class and ObjActParam, objact.rs `pre_start`): that is proof enough, also for a door
        // in a block streamed in after the last walk (no loading screen, no scan since).
        if own_toggle && s.loop_doors.insert(id) {
            log::line(&format!("doors: one-loop door flag {id} learnt from our own start (no walk had seen it yet)"));
        }
        // Guard 5 (no rest window: a rest never resets these MSB flags; a script's OFF, e.g.
        // m11_00 11100600 closing gate 61100610, is no start of ours and is not noted).
        if keep_open_loop_decision(s.loop_doors.contains(&id), own_toggle) {
            s.loop_kept_on += 1;
            log::line(&format!(
                "doors: our player used one-loop door {id} whose flag was on already (the friend opened it, our object had not moved): its toggle switched the flag off, kept on ({} so far)",
                s.loop_kept_on
            ));
            return true;
        }
        // Guard 2 never for one-loop doors: their OFF without a start of ours is a script's.
        if s.loop_doors.contains(&id) {
            return false;
        }
        if !keep_open_decision(s.friend_on.contains(&id), known_door(s, id), quiet) {
            return false;
        }
        s.kept_on += 1;
        let n = s.kept_on;
        note_log(s, || format!("doors: our ObjAct toggled door flag {id} off, but the friend had opened it: kept on ({n} so far)"));
        true
    })
}

/// world.rs `apply_flag`, a friend's value for `id`: true = it is an OFF for a known door
/// flag that is ON here (guard 3): the caller drops it (and tells the friend it is on).
pub fn ignore_friend_off(id: u32, on: bool) -> bool {
    if on || !map_flag(id) {
        return false;
    }
    let now = Instant::now();
    let quiet = rest_quiet(now);
    let known = with(|s| known_door(s, id));
    if !known {
        return false;
    }
    if !ignore_off_decision(on, crate::game::event_flag(id), known, quiet) {
        return false;
    }
    with(|s| {
        s.ignored_off += 1;
        let n = s.ignored_off;
        note_log(s, || format!("doors: the friend switched door flag {id} off (its ObjAct toggle); ignored, it stays on here ({n} so far)"));
    });
    true
}

/// world.rs, a new session: the friend's door flags of the last one no longer count.
pub fn reset() {
    with(|s| {
        s.friend_on.clear();
        s.pending.clear();
        s.recent_on.clear();
        s.friend_objact_at = None;
    });
}

/// world.rs, every frame while connected and settled (game thread; flags readable).
/// `peer_anims`: every other player's newest TimeAct id (slot, anim; net snapshots; the
/// 2-player "friend" generalized: any peer's ObjAct animation marks the flags that arrive
/// with it); `in_world_for`: time since our last load.
pub fn tick(peer_anims: &[(u8, i32)], in_world_for: Duration) {
    let now = Instant::now();
    // 0. The peers' ObjAct animations (fallback door flags) and our loads.
    let started = with(|s| {
        if in_world_for < s.last_in_world {
            // A loading screen: walk again, check the doors again, a fresh log budget.
            s.scan_due = true;
            s.load_checks = 0;
            s.logged = 0;
        }
        s.last_in_world = in_world_for;
        let started: Vec<i32> = peer_anims
            .iter()
            .filter(|&&(slot, anim)| s.peer_anim.insert(slot, anim) != Some(anim))
            .map(|&(_, anim)| anim)
            .collect();
        s.peer_anim.retain(|slot, _| peer_anims.iter().any(|(p, _)| p == slot));
        started
    });
    if !started.is_empty() && objact_anims().is_some_and(|set| started.iter().any(|&a| is_objact_anim(a, set))) {
        with(|s| {
            s.friend_objact_at = Some(now);
            let late: Vec<u32> = s.recent_on.iter().filter(|(_, t)| now.duration_since(*t) <= OBJACT_FLAG_BEFORE).map(|(f, _)| *f).collect();
            for f in late {
                add_candidate(s, f, "the friend set it right before its ObjAct animation showed up here");
            }
        });
    }
    if with(|s| std::mem::take(&mut s.scan_due) || !s.logged_scan && s.fails_logged.is_empty()) {
        doors();
    }
    // 1. Friend door flags: replay once the flag has settled.
    let due: Vec<u32> = with(|s| {
        let mut due = Vec::new();
        s.pending.retain(|&(f, t)| {
            if now.duration_since(t) < REPLAY_DELAY {
                return true;
            }
            due.push(f);
            false
        });
        due
    });
    if !due.is_empty() {
        let all = doors();
        with(|s| {
            for f in due {
                let mine: Vec<&Door> = all.iter().filter(|d| d.flag == f).collect();
                if mine.is_empty() {
                    continue; // not a walked door, or its block is not loaded here
                }
                s.friend_on.insert(f);
                for d in mine {
                    open_if_closed(s, d, "the friend opened a door");
                }
            }
            s.pending.retain(|&(_, t)| now.duration_since(t) < PENDING_FOR);
        });
    }
    // 4. After a rest or a loading screen: every loaded door whose flag is on must be open.
    let rest = crate::restsync::last_rest();
    let (after_rest, after_load) = with(|s| {
        if rest != s.rest_seen {
            s.rest_seen = rest;
            s.rest_checks = 0;
            s.logged = 0;
        }
        let after_rest = rest.is_some_and(|t| check_due(s.rest_checks, now.duration_since(t), &AFTER_REST));
        let after_load = check_due(s.load_checks, in_world_for, &AFTER_LOAD);
        s.rest_checks += usize::from(after_rest);
        s.load_checks += usize::from(after_load);
        (after_rest, after_load)
    });
    if after_rest || after_load {
        let (why, when) = if after_rest { ("after a rest", "the rest") } else { ("after a load", "the load") };
        let all = doors();
        with(|s| {
            let on = all.iter().filter(|d| crate::game::event_flag(d.flag) == Some(true)).count();
            let opened = all.iter().filter(|d| open_if_closed(s, d, why)).count();
            // Nothing to say when no loaded door has its flag on (these filled the old budget).
            if on > 0 {
                note_log(s, || format!("doors: after {when}: {} doors loaded, {on} with the flag on, {opened} were closed and opened again", all.len()));
            }
        });
    }
}

/// Is the `done`-th check of a series (`at`: its times after the start) due `since` the start?
fn check_due(done: usize, since: Duration, at: &[Duration]) -> bool {
    at.get(done).is_some_and(|&t| since >= t)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::BTreeMap;

    /// Sparse little-endian memory: (start, bytes).
    #[derive(Default)]
    struct Snap(BTreeMap<usize, Vec<u8>>);

    impl Snap {
        fn put(&mut self, at: usize, data: Vec<u8>) {
            self.0.insert(at, data);
        }
        fn put_u64(&mut self, at: usize, v: u64) {
            self.put(at, v.to_le_bytes().to_vec());
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
    const INFOS: usize = 0x40_0000;
    const STATES: usize = 0x50_0000;

    /// (state class vt RVA or 0 = no state, flag, sub index, index override)
    fn world(infos: &[(usize, u32, u32, Option<u32>)], loaded: u8) -> Snap {
        let mut m = Snap::default();
        m.put_u64(BASE + WORLD_OBJACT_MAN, MAN as u64);
        let mut man = vec![0u8; 0x40];
        man[MAN_BLOCK_COUNT..MAN_BLOCK_COUNT + 4].copy_from_slice(&2u32.to_le_bytes());
        man[MAN_BLOCKS..MAN_BLOCKS + 8].copy_from_slice(&(BLOCKS as u64).to_le_bytes());
        m.put(MAN, man);
        // Block 0 not loaded (garbage pointers: never read), block 1 loaded.
        let mut blocks = vec![0u8; BLOCK_STRIDE * 2];
        let b1 = BLOCK_STRIDE;
        blocks[b1..b1 + 8].copy_from_slice(&(DESC as u64).to_le_bytes());
        blocks[b1 + BLOCK_LOADED] = loaded;
        blocks[b1 + BLOCK_INFOS..b1 + BLOCK_INFOS + 8].copy_from_slice(&(INFOS as u64).to_le_bytes());
        blocks[b1 + BLOCK_INFO_COUNT..b1 + BLOCK_INFO_COUNT + 4].copy_from_slice(&(infos.len() as u32).to_le_bytes());
        blocks[BLOCK_INFOS..BLOCK_INFOS + 8].copy_from_slice(&0xDEAD_0000u64.to_le_bytes());
        blocks[BLOCK_INFO_COUNT..BLOCK_INFO_COUNT + 4].copy_from_slice(&9999u32.to_le_bytes());
        m.put(BLOCKS, blocks);
        let mut desc = vec![0u8; 0x40];
        desc[BLOCK_DESC_SLOT..BLOCK_DESC_SLOT + 4].copy_from_slice(&7u32.to_le_bytes());
        m.put(DESC, desc);
        let mut arr = vec![0u8; INFO_STRIDE * infos.len()];
        for (i, &(vt, flag, sub, idx)) in infos.iter().enumerate() {
            let at = INFO_STRIDE * i;
            arr[at + INFO_INDEX..at + INFO_INDEX + 4].copy_from_slice(&idx.unwrap_or(i as u32).to_le_bytes());
            arr[at + INFO_HANDLE..at + INFO_HANDLE + 4].copy_from_slice(&(0x100 + i as u32).to_le_bytes());
            arr[at + INFO_SUB..at + INFO_SUB + 4].copy_from_slice(&sub.to_le_bytes());
            // ObjActParam: a gate row for the one-loop info, a lever row for the others.
            let param: i32 = if vt == 0x298B6E0 { 999_900 } else { 205_011 };
            arr[at + INFO_PARAM_ID..at + INFO_PARAM_ID + 4].copy_from_slice(&param.to_le_bytes());
            if vt != 0 {
                let st = STATES + 0x100 * i;
                arr[at + INFO_STATE..at + INFO_STATE + 8].copy_from_slice(&(st as u64).to_le_bytes());
                let mut s = vec![0u8; 0x18];
                s[0..8].copy_from_slice(&((BASE + vt) as u64).to_le_bytes());
                s[STATE_FLAG..STATE_FLAG + 4].copy_from_slice(&flag.to_le_bytes());
                m.put(st, s);
            }
        }
        m.put(INFOS, arr);
        m
    }

    #[test]
    fn walker_finds_primary_flag_states_only() {
        let m = world(
            &[
                (0x298B570, 0, 0, None),          // one-state: counted, no flag
                (0x298B628, 21005024, 0, None),   // door primary
                (0x298B628, 21005024, 1, None),   // door sub infos
                (0x298B628, 21005024, 2, None),
                (0x298B628, 21005024, 3, None),
                (0x298B6E0, 11000670, 0, None),   // one-loop
                (0x298B798, 11000671, 0, None),   // one-dir door
                (0x298B798, u32::MAX, 0, None),   // no flag
                (0x298B798, 11000672, 0, Some(99)), // index mismatch
                (0, 0, 0, None),                  // no state
                (0x1234, 0, 0, None),             // unknown vtable
            ],
            1,
        );
        let (doors, r) = scan(&m, BASE);
        let flags: Vec<u32> = doors.iter().map(|d| d.flag).collect();
        assert_eq!(flags, vec![21005024, 11000670, 11000671]);
        assert_eq!(
            doors[0],
            Door { block: BLOCKS + BLOCK_STRIDE, slot: 7, index: 1, flag: 21005024, handle: 0x101, running: false, class: Class::Door, param: 205_011 }
        );
        // The one-loop gate (ObjActParam 999900) is the only one-loop door.
        assert_eq!((doors[1].class, doors[1].param, doors[1].loop_door()), (Class::OneLoop, 999_900, true));
        assert!(!doors[0].loop_door() && !doors[2].loop_door());
        assert_eq!((r.man, r.blocks, r.loaded, r.infos), (MAN, 2, 1, 11));
        assert_eq!((r.one, r.door, r.one_loop, r.one_dir, r.no_state, r.other), (1, 4, 1, 3, 1, 1));
        assert_eq!((r.subs, r.bad, r.fail), (3, 2, None));
    }

    /// Every ObjActParam id placed in a Sekiro MSB (objacts.md section 2) and its kind.
    #[test]
    fn kind_of_every_msb_objact_param() {
        let doors = [
            999_900, 999_901, 999_910, 999_911, 999_920, 999_921, 999_922, 999_940, 999_941, 999_945, 999_946, 999_950, 999_960, 999_970, 999_971,
            999_975, 999_976, 101_050,
        ];
        for p in doors {
            assert_eq!(kind(p), Kind::Door, "{p}");
        }
        for p in [999_980, 999_985, 999_990] {
            assert_eq!(kind(p), Kind::Chest, "{p}");
        }
        // Levers, bells, rotating walls / floors, windows, hidden tatami (-1), garbage.
        for p in [205_010, 205_011, 205_100, 205_101, 252_555, 90_060, 90_061, 90_071, 90_075, 90_076, 1_100_000, 109_950, -1, 0, 10_000] {
            assert_eq!(kind(p), Kind::Other, "{p}");
        }
        // One-loop + door row = one-loop door; the same row in another class is not.
        assert!(loop_door(Some(Class::OneLoop), 999_900));
        assert!(loop_door(Some(Class::OneLoop), 999_975));
        assert!(!loop_door(Some(Class::OneLoop), 1_100_000));
        assert!(!loop_door(Some(Class::OneLoop), 252_555));
        assert!(!loop_door(Some(Class::One), 999_941));
        assert!(!loop_door(None, 999_900));
    }

    #[test]
    fn only_doors_and_chests_are_reopened_from_their_flag() {
        assert!(reopens(999_900) && reopens(999_941) && reopens(101_050) && reopens(999_980));
        // The window 61110703 (ObjActParam 1100000), rotating floor, bell, hidden tatami, lever.
        for p in [1_100_000, 90_061, 252_555, 109_950, 205_011, -1] {
            assert!(!reopens(p), "{p}");
        }
    }

    #[test]
    fn guard_5_only_after_our_own_toggle_of_a_loop_door() {
        assert!(keep_open_loop_decision(true, true));
        // A script's OFF (no start of ours), or not a one-loop door.
        assert!(!keep_open_loop_decision(true, false));
        assert!(!keep_open_loop_decision(false, true));
        let now = Instant::now();
        note_own_loop_toggle(61_110_602);
        assert!(!take_own_loop_toggle(61_110_603, now));
        assert!(take_own_loop_toggle(61_110_602, Instant::now()));
        // Used up: one write back per toggle.
        assert!(!take_own_loop_toggle(61_110_602, Instant::now()));
    }

    #[test]
    fn a_loop_door_at_its_animation_counts_as_open() {
        let v = |anim, playing| ObjView { obj: 0x1000, anim, playing, want: 1 };
        assert!(open_decision(true, &v(1, false)) && open_decision(true, &v(1, true)));
        assert!(!open_decision(true, &v(0, false)));
        // Other classes as before: playing that animation.
        assert!(open_decision(false, &v(1, true)));
        assert!(!open_decision(false, &v(1, false)));
    }

    #[test]
    fn walker_reports_the_failed_step() {
        let m = world(&[(0x298B628, 21005024, 0, None)], 0);
        let (doors, r) = scan(&m, BASE);
        assert!(doors.is_empty());
        assert_eq!(r.fail, Some("no loaded block (+0x10)"));
        let empty = Snap::default();
        assert_eq!(scan(&empty, BASE).1.fail, Some("WorldObjActMan pointer (base+0x3D55EC8) is null"));
        let mut m = world(&[], 1);
        m.0.get_mut(&MAN).unwrap()[MAN_BLOCK_COUNT..MAN_BLOCK_COUNT + 4].copy_from_slice(&500u32.to_le_bytes());
        assert_eq!(scan(&m, BASE).1.fail, Some("block count (+0x28) is 0 or out of range"));
    }

    #[test]
    fn objact_window() {
        let a = Instant::now();
        assert!(objact_near(Some(a), a + Duration::from_secs(5)));
        assert!(!objact_near(Some(a), a + Duration::from_secs(7)));
        assert!(objact_near(Some(a + Duration::from_secs(1)), a));
        assert!(!objact_near(Some(a + Duration::from_secs(2)), a));
        assert!(!objact_near(None, a));
    }

    #[test]
    fn guard_decisions() {
        // Guard 2: only a door flag the friend opened, never right after a rest.
        assert!(keep_open_decision(true, true, false));
        assert!(!keep_open_decision(false, true, false));
        assert!(!keep_open_decision(true, false, false));
        assert!(!keep_open_decision(true, true, true));
        // Guard 3: a friend OFF of a known door flag that is on here.
        assert!(ignore_off_decision(false, Some(true), true, false));
        assert!(!ignore_off_decision(true, Some(true), true, false));
        assert!(!ignore_off_decision(false, Some(false), true, false));
        assert!(!ignore_off_decision(false, None, true, false));
        assert!(!ignore_off_decision(false, Some(true), false, false));
        assert!(!ignore_off_decision(false, Some(true), true, true));
    }

    #[test]
    fn door_checks_after_rests_and_loads() {
        let s = Duration::from_secs;
        // After a load (in_world_for): at 6 s and 12 s, each once, in order.
        assert!(!check_due(0, s(5), &AFTER_LOAD));
        assert!(check_due(0, s(6), &AFTER_LOAD));
        assert!(!check_due(1, s(11), &AFTER_LOAD));
        assert!(check_due(1, s(12), &AFTER_LOAD));
        assert!(!check_due(2, s(600), &AFTER_LOAD));
        // The world settled late (the tick starts after both times): one check per tick.
        assert!(check_due(0, s(30), &AFTER_LOAD) && check_due(1, s(30), &AFTER_LOAD));
        // After a rest: 2.5 s and 7.5 s.
        assert!(!check_due(0, Duration::from_millis(2400), &AFTER_REST));
        assert!(check_due(0, Duration::from_millis(2500), &AFTER_REST));
        assert!(!check_due(2, s(60), &AFTER_REST));
    }

    #[test]
    fn flag_and_anim_filters() {
        assert!(map_flag(21005024) && map_flag(11000670));
        assert!(!map_flag(9900) && !map_flag(71000000) && !map_flag(31000000));
        let set: HashSet<u32> = [700250].into_iter().collect();
        assert!(is_objact_anim(700250, &set));
        assert!(is_objact_anim(100_700_250, &set));
        assert!(!is_objact_anim(700251, &set) && !is_objact_anim(-1, &set));
    }
}
