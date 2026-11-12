//! Effects, sounds and look of the friend's native remote ("replica"), docs/RE-REMOTE-FX.md.
//!
//! Three parts, each behind its own [DEBUG] setting:
//!
//! 1. **SpEffect gate mirror** (`fx_speffect_mirror`, default 1). The white slash of every
//!    combat art (TAE type 96, FFX 4400xx), the Dragon Flash sounds (type 129) and the extra
//!    Floating Passage / Mortal Draw effects and hits carry a StateInfo condition: they only
//!    fire while the character has an active SpEffect whose SpEffectParam `stateInfo`
//!    (u16 at row +0x15E) matches [V data: c0000.anibnd + params, E:\RE\work_fx\data]. The
//!    dispatcher checks it with `0xBFEBF0(*(chr+0x11D0), stateInfo)` [V disasm 0xB59170 /
//!    0xB5A6D0]. Those SpEffects (106000 "no enchantment" = 914, 140310 = 995, 140510 = 994,
//!    140610 = 993) are set only by common.emevd 9940 / 9930-9932 on entity 10000, i.e. the
//!    local player; the replica never gets them, so its art slashes and those sounds are
//!    skipped. The owner sends the ids of its active SpEffects with a gate stateInfo
//!    (PlayerRaw type `PTYPE`), the watcher applies / removes exactly those ids on the
//!    owner's replica (`0x9E9C00` / `0xC00340`, the calls speffect.rs uses). The add is
//!    FORCED: unforced, `0x9E9D10` refuses every SpEffect on a remote player (test4: 7735 x
//!    "present false"). Level 2 also mirrors the enchant stateInfos the art events test.
//! 2. **Full-detail parts** (`native_full_parts`, default 1). The ChrAsm model builder
//!    `0xA361C0` passes "not the local player" as the 6th argument of `0xE7B000`, which
//!    stores it at model+0x2B and appends `_L` to every part name (`0x10D1CE0`) [V]. The
//!    `_L` files have byte-identical FLVERs (same dummy polys) and ~4x smaller textures
//!    [V data], so this is only the texture quality the friend noticed. The call at
//!    `0xA363BC` is redirected; for our replicas the argument becomes 0 (as vanilla does
//!    for chr types 3/4/10/11). The builder runs every frame and is cheap while nothing
//!    changes; only real part loads are logged. native.rs registers a new chr at once, so
//!    its first build is full-detail too (test4 built `_L` first, then reloaded).
//! 3. **Diagnostics** (`fx_debug`, default 0): `fx:` lines with the local player's and the
//!    replicas' SpEffect changes, the TAE effect / sound events they start (with the
//!    StateInfo condition and its result), parts flag, SE module and weapon SE offset.
//!
//! Hooks never panic into the game; every hook passes straight through when its setting is off.

use std::collections::VecDeque;
use std::sync::Mutex;
use std::sync::atomic::{AtomicBool, AtomicU8, AtomicU32, AtomicUsize, Ordering::Relaxed};
use std::time::{Duration, Instant};

use crate::game::{Chr, module_base, read, read_bytes, read_ptr};
use crate::peers::{MAX_PLAYERS, PeerId, PeerSet};
use crate::{hook, log};

/// Settings ([DEBUG], live).
pub static FULL_PARTS: AtomicBool = AtomicBool::new(true);
pub static FX_DEBUG: AtomicBool = AtomicBool::new(false);
/// 0 off, 1 art gates, 2 art gates + enchant stateInfos.
pub static MIRROR: AtomicU8 = AtomicU8::new(1);

/// PlayerRaw type of our gate-SpEffect messages (artfx uses 0xAF63).
pub const PTYPE: u32 = 0xAF64;
const MSG_VERSION: u8 = 1;
const MAX_IDS: usize = 16;

// --- Game code / layout (RVAs, Sekiro 1.06) -----------------------------------------------

/// `call 0xE7B000` in the ChrAsm model builder `0xA361C0` (rcx = *(chr+0x2148)).
const SITE_BUILD: usize = 0xA363BC;
const FN_BUILD: usize = 0xE7B000;
/// SprjChrTaeAnimEvent vtable `0x2A7AE70`: slot 1 `0xB59140` only runs slots 2 and 3, and is
/// reached only through the thunk `0x654A8C` of `0x654B70` (used when `0x141043150` says so);
/// the per-frame paths `0xB58420` / `0xB58530` -> `0xB4FBA0` call slot 2 (`0xB4FD9C`) and
/// slot 3 (`0xB4FDA4`) directly [VAR 2026-11-12, E:\RE\work_fx2\o_tae*.txt]. So slot 2, the
/// first dispatcher `0xB59170`, sees every event exactly once. `this+0x10` = chr.
const TAE_SLOT_RVA: usize = 0x2A7AE80;
const TAE_SLOT_FN: usize = 0xB59170;
/// `0x9E9C00(chr, id, source, u8, u8, &rates, f32, u8 force, i32, u8)` returns nothing: it
/// calls `0x9E9D10`, which REFUSES a non-forced add on a remote player (`0x9F36C0(chr)`:
/// player, not `[WorldChrMan+0x88]`, `vt[0x1B0]` false) unless `force` is set; with the source
/// being the main player it instead asks the owner (`vt[0x3C8]`) [VAR 2026-11-12,
/// o_apply*.txt, o_pred.txt]. The mirror therefore forces (the add core `0x9F8320` has no
/// locality check).
const APPLY_RVA: usize = 0x9E9C00;
const APPLY_FORCE: u8 = 1;
const APPLY_PROLOGUE: [u8; 11] = [0x48, 0x8B, 0xC4, 0x48, 0x89, 0x58, 0x08, 0x48, 0x89, 0x68, 0x10];
const HAS_RVA: usize = 0xBFEFB0;
const REMOVE_RVA: usize = 0xC00340;
/// `(list, u16 stateInfo) -> u8`: an active entry whose row has this stateInfo.
const HAS_STATE_RVA: usize = 0xBFEBF0;
/// `(ParamRef* out, id)`: SpEffectParam row lookup (speffect.rs detours it; we call the entry).
const LOOKUP_RVA: usize = 0x10CEC30;

const CHR_TYPE: usize = 0x70;
const CHR_SP_LIST: usize = 0x11D0;
const CHR_MODULES: usize = 0x1FF8;
const MOD_SE: usize = 0xE0;
const MOD_TIME_ACT: usize = 0x10;
/// `chr->vtbl[47]` of PlayerIns = `*(chr+0x20B0)`; its `+8` holds the weapon SE id offset at `+0x350`.
const CHR_WEP_INFO: usize = 0x20B0;
const WEP_SE_OFFSET: usize = 0x350;
const CHR_ASM_MODEL: usize = 0x2148;
/// ChrAsm model: 1 = `_L` parts (the 6th argument of 0xE7B000).
const ASM_LOW_DETAIL: usize = 0x2B;
/// SpEffect list: first node at +8; node: next +0x70, id +0x58, row +0x60, state bits +0x1C.
const LIST_FIRST: usize = 0x8;
const NODE_NEXT: usize = 0x70;
const NODE_ID: usize = 0x58;
const NODE_ROW: usize = 0x60;
const NODE_STATE: usize = 0x1C;
/// Entries with any of these bits are ending / not counted by `0xBFEBF0`.
const NODE_INACTIVE: u32 = 0x800C_0003;
const ROW_STATE_INFO: usize = 0x15E;
/// TAE event: type at +0, params at +8, "first frame" byte at +0x18 [ARV: sounds play once on it].
const EV_PARAMS: usize = 0x8;
const EV_BEGIN: usize = 0x18;

/// StateInfos the art events test (FINDINGS.txt § 4): 914 no enchantment (106000),
/// 993 / 994 / 995 Mortal Draw / Floating Passage / Dragon Flash upgrades (EMEVD 9930-9932).
const GATES_ART: [u16; 4] = [914, 993, 994, 995];
/// Enchant stateInfos the same events test (level 2).
const GATES_ENCHANT: [u16; 9] = [62, 64, 152, 153, 356, 357, 358, 917, 940];

const MIRROR_EVERY: Duration = Duration::from_millis(200);
const SEND_REFRESH: Duration = Duration::from_secs(1);
/// No message from a peer for this long: what we applied for it goes.
const WANT_STALE: Duration = Duration::from_secs(4);
const MIRROR_LOGS: u32 = 40;

// --- Pure logic -------------------------------------------------------------------------------

/// Is `state_info` mirrored at `level`?
pub fn gate_allowed(state_info: u16, level: u8) -> bool {
    match level {
        0 => false,
        1 => GATES_ART.contains(&state_info),
        _ => GATES_ART.contains(&state_info) || GATES_ENCHANT.contains(&state_info),
    }
}

/// `u8 version, u8 n, n * i32 id` (ids sorted, at most MAX_IDS).
pub fn encode(ids: &[i32]) -> Vec<u8> {
    let n = ids.len().min(MAX_IDS);
    let mut m = Vec::with_capacity(2 + 4 * n);
    m.push(MSG_VERSION);
    m.push(n as u8);
    for id in &ids[..n] {
        m.extend_from_slice(&id.to_le_bytes());
    }
    m
}

pub fn decode(data: &[u8]) -> Option<Vec<i32>> {
    let (&v, rest) = data.split_first()?;
    let (&n, rest) = rest.split_first()?;
    let n = n as usize;
    if v != MSG_VERSION || n > MAX_IDS || rest.len() != 4 * n {
        return None;
    }
    let ids: Vec<i32> = rest.chunks_exact(4).map(|c| i32::from_le_bytes([c[0], c[1], c[2], c[3]])).collect();
    ids.iter().all(|&id| id > 0).then_some(ids)
}

/// Tries of one SpEffect on one replica that left it absent, in a row, before the mirror
/// gives up on it (test5: live test4 logged `+106000(ret 0, present false)` ~6400 times).
pub const MIRROR_GIVE_UP: u32 = 5;

/// One try's outcome into the per-replica failure counts. Some(n) = this try made it give up.
/// `present` is the `0xBFEFB0` re-check after the forced add; the add itself returns nothing.
/// A success resets the count; an id the owner no longer has is forgotten (`forget_unwanted`),
/// so its next activation gets fresh tries.
pub fn note_try(fails: &mut Vec<(i32, u32)>, id: i32, present: bool) -> Option<u32> {
    let at = fails.iter().position(|(f, _)| *f == id);
    if present {
        if let Some(at) = at {
            fails.swap_remove(at);
        }
        return None;
    }
    let n = match at {
        Some(at) => {
            fails[at].1 += 1;
            fails[at].1
        }
        None => {
            fails.push((id, 1));
            1
        }
    };
    (n == MIRROR_GIVE_UP).then_some(n)
}

/// Given up on (`MIRROR_GIVE_UP` failures in a row).
pub fn given_up(fails: &[(i32, u32)], id: i32) -> bool {
    fails.iter().any(|&(f, n)| f == id && n >= MIRROR_GIVE_UP)
}

pub fn forget_unwanted(fails: &mut Vec<(i32, u32)>, want: &[i32]) {
    fails.retain(|(id, _)| want.contains(id));
}

/// What to do on a replica: (apply, remove). `want` = the owner's ids (already filtered by the
/// local level), `applied` = what we applied earlier, `has` = present on the replica now.
/// Only ids we applied are ever removed.
pub fn plan(want: &[i32], applied: &[i32], has: impl Fn(i32) -> bool) -> (Vec<i32>, Vec<i32>) {
    let add = want.iter().copied().filter(|&id| !has(id)).collect();
    let del = applied.iter().copied().filter(|id| !want.contains(id) && has(*id)).collect();
    (add, del)
}

// --- Game reads ---------------------------------------------------------------------------------

/// Active SpEffect entries of `chr`: (id, stateInfo), at most 64.
fn speffects(chr: usize) -> Vec<(i32, u16)> {
    let mut v = Vec::new();
    let Some(list) = read_ptr(chr + CHR_SP_LIST).filter(|&l| l >= 0x10000) else { return v };
    let mut node = read_ptr(list + LIST_FIRST).unwrap_or(0);
    while node >= 0x10000 && v.len() < 64 {
        let state = read::<u32>(node + NODE_STATE).unwrap_or(u32::MAX);
        if state & NODE_INACTIVE == 0 {
            let id = read::<i32>(node + NODE_ID).unwrap_or(-1);
            let si = read_ptr(node + NODE_ROW).filter(|&r| r >= 0x10000).and_then(|r| read::<u16>(r + ROW_STATE_INFO)).unwrap_or(0);
            v.push((id, si));
        }
        node = read_ptr(node + NODE_NEXT).unwrap_or(0);
    }
    v
}

#[repr(C)]
struct ParamRef {
    id: i32,
    _pad: u32,
    row: usize,
    _extra: [u64; 2],
}

/// stateInfo of SpEffectParam row `id` (None = no such row).
fn row_state_info(id: i32) -> Option<u16> {
    let f: extern "C" fn(*mut ParamRef, i32) = unsafe { std::mem::transmute(module_base() + LOOKUP_RVA) };
    let mut r = ParamRef { id: -1, _pad: 0, row: 0, _extra: [0; 2] };
    f(&mut r, id);
    (r.row >= 0x10000).then(|| read::<u16>(r.row + ROW_STATE_INFO)).flatten()
}

fn anim_of(chr: usize) -> Option<i32> {
    let ta = read_ptr(read_ptr(chr + CHR_MODULES)? + MOD_TIME_ACT)?;
    let head = read::<i32>(ta + 0xE8)?;
    let tail = read::<i32>(ta + 0xEC)?;
    if !(0..10).contains(&head) || head == tail {
        return None;
    }
    read::<i32>(ta + 0x20 + 0x14 * ((head + 9) % 10) as usize).filter(|&id| id > 0)
}

// --- Shared state ---------------------------------------------------------------------------------

/// Each peer's native remote chr and its ChrAsm model (hook-safe lookups), by `PeerId::index`.
static REPLICA_CHR: [AtomicUsize; MAX_PLAYERS] = [const { AtomicUsize::new(0) }; MAX_PLAYERS];
static REPLICA_ASM: [AtomicUsize; MAX_PLAYERS] = [const { AtomicUsize::new(0) }; MAX_PLAYERS];
static LOCAL_CHR: AtomicUsize = AtomicUsize::new(0);
static LOCAL_ASM: AtomicUsize = AtomicUsize::new(0);

/// native.rs `create` runs the game's player factory right now (any thread: only PlayerIns
/// reach `0xA361C0`, and the local player's model is excluded by address).
static CREATING: AtomicBool = AtomicBool::new(false);
/// Friends' chrs native.rs just created, until `tick` sees them as replicas (their first
/// part builds can run before puppet.rs lists them) or `CREATED_TTL` ticks pass.
static CREATED: [AtomicUsize; MAX_PLAYERS] = [const { AtomicUsize::new(0) }; MAX_PLAYERS];
static CREATED_AGE: [AtomicU32; MAX_PLAYERS] = [const { AtomicU32::new(0) }; MAX_PLAYERS];
const CREATED_TTL: u32 = 600;

static BUILD_TRIED: AtomicBool = AtomicBool::new(false);
static ORIG_BUILD: AtomicUsize = AtomicUsize::new(0);
static TAE_TRIED: AtomicBool = AtomicBool::new(false);
static ORIG_TAE: AtomicUsize = AtomicUsize::new(0);
static APPLY_OK: AtomicU8 = AtomicU8::new(0); // 0 unchecked, 1 ok, 2 bad
/// Real part loads of friends' models started through the build hook (since the last
/// fx_debug line), and how many were logged in full.
static LOAD_COUNT: AtomicU32 = AtomicU32::new(0);
static LOAD_LOGGED: AtomicU32 = AtomicU32::new(0);
const LOAD_LOGS: u32 = 40;
/// ChrAsm model: pending part loads (`0xE7B000` works only while both are < 1, and sets
/// +8 = 2 when it requests a load) [VAR, o_parts.txt].
const ASM_PENDING_A: usize = 0x8;
const ASM_PENDING_B: usize = 0xC;

static OUTBOX: Mutex<VecDeque<Vec<u8>>> = Mutex::new(VecDeque::new());
/// Per peer index: (ids the owner has, when received).
static WANT: Mutex<[Option<(Vec<i32>, Instant)>; MAX_PLAYERS]> = Mutex::new([const { None }; MAX_PLAYERS]);

/// One TAE event a hook saw (drained and logged by `tick`).
struct TaeRec {
    who: String,
    anim: Option<i32>,
    ty: i32,
    params: [u8; 24],
    cond: Option<(u16, bool)>,
}
static TAE_Q: Mutex<VecDeque<TaeRec>> = Mutex::new(VecDeque::new());
static TAE_DROPPED: AtomicU32 = AtomicU32::new(0);

/// Game-thread state of `tick`.
struct State {
    last_sent: Option<(Vec<i32>, Instant)>,
    mirror_at: Option<Instant>,
    /// Per peer index: (replica chr, ids we applied to it).
    applied: [(usize, Vec<i32>); MAX_PLAYERS],
    /// Per peer index: (SpEffect id, tries in a row that left it absent) on that replica.
    fails: [Vec<(i32, u32)>; MAX_PLAYERS],
    mirror_logs: u32,
    /// fx_debug: last SpEffect id set and status line per watched chr.
    seen_sp: Vec<(usize, Vec<i32>)>,
    seen_status: Vec<(usize, String)>,
    log_window: Option<Instant>,
    log_lines: u32,
}
static STATE: Mutex<Option<State>> = Mutex::new(None);

// --- Entry points ---------------------------------------------------------------------------------

/// native.rs, around the game's player factory call (game thread).
pub fn creating(on: bool) {
    if on && FULL_PARTS.load(Relaxed) {
        install_build_hook();
    }
    CREATING.store(on, Relaxed);
}

/// native.rs, right after the factory returned the friend's chr `chr`.
pub fn created(chr: usize) {
    if chr < 0x10000 || CREATED.iter().any(|c| c.load(Relaxed) == chr) {
        return;
    }
    let i = CREATED.iter().position(|c| c.load(Relaxed) == 0).unwrap_or(0);
    CREATED_AGE[i].store(0, Relaxed);
    CREATED[i].store(chr, Relaxed);
}

/// lib.rs, when the mod disables itself.
pub fn shutdown() {
    FULL_PARTS.store(false, Relaxed);
    FX_DEBUG.store(false, Relaxed);
    MIRROR.store(0, Relaxed);
}

/// net.rs: our gate-SpEffect messages for the peers that show us natively.
pub fn take_outbound(forward: bool) -> Vec<Vec<u8>> {
    let Ok(mut q) = OUTBOX.lock() else { return Vec::new() };
    if !forward {
        q.clear();
        return Vec::new();
    }
    q.drain(..).collect()
}

/// net.rs: a PlayerRaw packet; true if it was ours.
pub fn on_raw(from: PeerId, ptype: u32, data: &[u8]) -> bool {
    if ptype != PTYPE {
        return false;
    }
    if let (Some(ids), Ok(mut w)) = (decode(data), WANT.lock())
        && let Some(slot) = w.get_mut(from.index())
    {
        *slot = Some((ids, Instant::now()));
    }
    true
}

/// puppet.rs, every frame on the game thread, after the native remotes were refreshed.
pub fn tick(player: Option<Chr>, peers: &PeerSet) {
    let _ = std::panic::catch_unwind(|| tick_inner(player, peers));
}

fn tick_inner(player: Option<Chr>, peers: &PeerSet) {
    let local = player.map_or(0, |c| c.0);
    LOCAL_CHR.store(local, Relaxed);
    LOCAL_ASM.store(if local != 0 { read_ptr(local + CHR_ASM_MODEL).unwrap_or(0) } else { 0 }, Relaxed);
    for i in 0..MAX_PLAYERS {
        let chr = PeerId::new(i as u8).and_then(|id| peers.by_id(id)).and_then(|p| p.native).map_or(0, |c| c.0);
        REPLICA_CHR[i].store(chr, Relaxed);
        REPLICA_ASM[i].store(if chr != 0 { read_ptr(chr + CHR_ASM_MODEL).unwrap_or(0) } else { 0 }, Relaxed);
    }
    for (c, age) in CREATED.iter().zip(&CREATED_AGE) {
        let v = c.load(Relaxed);
        if v != 0 && (REPLICA_CHR.iter().any(|r| r.load(Relaxed) == v) || age.fetch_add(1, Relaxed) >= CREATED_TTL) {
            c.store(0, Relaxed);
        }
    }
    if FULL_PARTS.load(Relaxed) {
        install_build_hook();
    }
    if FX_DEBUG.load(Relaxed) {
        install_tae_hook();
    }
    let Ok(mut g) = STATE.lock() else { return };
    let s = g.get_or_insert_with(|| State {
        last_sent: None,
        mirror_at: None,
        applied: [const { (0, Vec::new()) }; MAX_PLAYERS],
        fails: [const { Vec::new() }; MAX_PLAYERS],
        mirror_logs: 0,
        seen_sp: Vec::new(),
        seen_status: Vec::new(),
        log_window: None,
        log_lines: 0,
    });
    let now = Instant::now();
    if s.mirror_at.is_none_or(|t| now.duration_since(t) >= MIRROR_EVERY) {
        s.mirror_at = Some(now);
        send_own(s, local, now);
        mirror(s, now);
    }
    if FX_DEBUG.load(Relaxed) {
        debug_frame(s, local, now);
    }
}

/// Owner side: our gate SpEffects (stateInfo in the art + enchant sets, the receiver filters
/// by its own level) when they change, and again every second (PlayerRaw is not reliable).
fn send_own(s: &mut State, local: usize, now: Instant) {
    if local == 0 {
        return;
    }
    let mut ids: Vec<i32> = speffects(local).into_iter().filter(|&(id, si)| id > 0 && gate_allowed(si, 2)).map(|(id, _)| id).collect();
    ids.sort_unstable();
    ids.dedup();
    ids.truncate(MAX_IDS);
    let due = match &s.last_sent {
        Some((prev, at)) => *prev != ids || now.duration_since(*at) >= SEND_REFRESH,
        None => true,
    };
    if !due {
        return;
    }
    if FX_DEBUG.load(Relaxed) && s.last_sent.as_ref().is_none_or(|(prev, _)| *prev != ids) {
        log::line(&format!("fx: mirror send: our gate SpEffects {ids:?}"));
    }
    if let Ok(mut q) = OUTBOX.lock() {
        if q.len() >= 8 {
            q.pop_front();
        }
        q.push_back(encode(&ids));
    }
    s.last_sent = Some((ids, now));
}

/// Watcher side: each replica carries exactly the owner's gate SpEffects allowed by our level.
fn mirror(s: &mut State, now: Instant) {
    let level = MIRROR.load(Relaxed);
    let wants: Vec<Option<Vec<i32>>> = match WANT.lock() {
        Ok(w) => w.iter().map(|e| e.as_ref().filter(|(_, at)| now.duration_since(*at) < WANT_STALE).map(|(ids, _)| ids.clone())).collect(),
        Err(_) => return,
    };
    let base = module_base();
    for i in 0..MAX_PLAYERS {
        let chr = REPLICA_CHR[i].load(Relaxed);
        if s.applied[i].0 != chr {
            s.applied[i] = (chr, Vec::new());
            s.fails[i].clear();
        }
        if chr == 0 {
            continue;
        }
        let Some(list) = read_ptr(chr + CHR_SP_LIST).filter(|&l| l >= 0x10000) else { continue };
        let want: Vec<i32> = wants
            .get(i)
            .cloned()
            .flatten()
            .unwrap_or_default()
            .into_iter()
            .filter(|&id| level > 0 && row_state_info(id).is_some_and(|si| gate_allowed(si, level)))
            .collect();
        if want.is_empty() && s.applied[i].1.is_empty() {
            continue;
        }
        if !want.is_empty() && !apply_checked() {
            continue;
        }
        let has_f: extern "C" fn(usize, i32) -> u8 = unsafe { std::mem::transmute(base + HAS_RVA) };
        forget_unwanted(&mut s.fails[i], &want);
        let (mut add, del) = plan(&want, &s.applied[i].1, |id| has_f(list, id) != 0);
        add.retain(|&id| !given_up(&s.fails[i], id));
        if add.is_empty() && del.is_empty() {
            continue;
        }
        let apply: extern "C" fn(usize, i32, usize, u8, u8, *const [f32; 4], f32, u8, i32, u8) =
            unsafe { std::mem::transmute(base + APPLY_RVA) };
        let remove: extern "C" fn(usize, i32) -> usize = unsafe { std::mem::transmute(base + REMOVE_RVA) };
        static ONE4: [f32; 4] = [1.0; 4];
        let mut done = Vec::new();
        for &id in &add {
            // 0x9E9C00 returns nothing: only the re-check says whether the add took.
            apply(chr, id, chr, 0, 0, &ONE4, 1.0, APPLY_FORCE, 0, 0);
            let present = has_f(list, id) != 0;
            done.push(format!("+{id}(forced, present {present})"));
            if let Some(n) = note_try(&mut s.fails[i], id, present) {
                log::line(&format!(
                    "fx: mirror: replica of slot {i} chr {chr:#x}: SpEffect {id} still absent after {n} forced tries in a row: no more tries on this replica until the owner loses it"
                ));
            }
            if !s.applied[i].1.contains(&id) {
                s.applied[i].1.push(id);
            }
        }
        for &id in &del {
            remove(list, id);
            done.push(format!("-{id}"));
        }
        s.applied[i].1.retain(|id| want.contains(id));
        if s.mirror_logs < MIRROR_LOGS || FX_DEBUG.load(Relaxed) {
            s.mirror_logs += 1;
            log::line(&format!("fx: mirror: replica of slot {i} chr {chr:#x}: {} (owner has {want:?})", done.join(" ")));
        }
    }
}

/// The SpEffect add function has the bytes speffect.rs verified (checked once).
fn apply_checked() -> bool {
    match APPLY_OK.load(Relaxed) {
        1 => true,
        2 => false,
        _ => {
            let mut b = [0u8; 11];
            let ok = read_bytes(module_base() + APPLY_RVA, &mut b) && b == APPLY_PROLOGUE;
            APPLY_OK.store(if ok { 1 } else { 2 }, Relaxed);
            if !ok {
                log::line(&format!("fx: mirror off: SpEffect add 0x9E9C00 has unexpected bytes {b:02x?}"));
            }
            ok
        }
    }
}

// --- Full-detail parts: call-site hook at 0xA363BC ------------------------------------------------

/// Checks that `site` is `call rel32` to `base + target` and returns the rel32 bytes.
fn call_rel(site: usize, target: usize) -> Option<[u8; 4]> {
    let base = module_base();
    let op = read::<u8>(base + site)?;
    let rel = read::<[u8; 4]>(base + site + 1)?;
    let to = (base + site + 5) as i64 + i32::from_le_bytes(rel) as i64;
    (op == 0xE8 && to == (base + target) as i64).then_some(rel)
}

/// Points `site` (a `call rel32` to `target`) at a near stub `mov rax, hook; jmp rax`.
fn redirect_call(site: usize, target: usize, hook_fn: usize, name: &str) -> bool {
    let base = module_base();
    let Some(rel) = call_rel(site, target) else {
        log::line(&format!("fx: {name} at {:#x} is not call {:#x}, not hooked", base + site, base + target));
        return false;
    };
    let Some(stub) = hook::alloc_near_pub(base + site) else {
        log::line(&format!("fx: no memory near the module for {name}"));
        return false;
    };
    let mut code = [0u8; 12];
    code[0] = 0x48;
    code[1] = 0xB8;
    code[2..10].copy_from_slice(&(hook_fn as u64).to_le_bytes());
    code[10] = 0xFF;
    code[11] = 0xE0;
    unsafe { std::ptr::copy_nonoverlapping(code.as_ptr(), stub as *mut u8, code.len()) };
    let Ok(new_rel) = i32::try_from(stub as i64 - (base + site + 5) as i64) else {
        log::line(&format!("fx: stub {stub:#x} out of rel32 range for {name}"));
        return false;
    };
    hook::patch_code(site + 1, &rel, &new_rel.to_le_bytes(), name)
}

fn install_build_hook() {
    if BUILD_TRIED.swap(true, Relaxed) {
        return;
    }
    ORIG_BUILD.store(module_base() + FN_BUILD, Relaxed);
    if redirect_call(SITE_BUILD, FN_BUILD, build_hook as *const () as usize, "ChrAsm part build (0xA363BC -> 0xE7B000)") {
        log::line("fx: full-detail parts for the friends' characters: hook installed (native_full_parts)");
    }
}

/// `0xE7B000(asm, model, a3, u8, u8, u8 low_detail, u8, u8, a9) -> u8` from 0xA361C0.
///
/// `0xA361C0` runs every frame for every PlayerIns (2 calls a frame per replica in the
/// 2026-11-12 logs), and `0xE7B000` is cheap unless something changed: only while no load is
/// pending (`asm+8`, `asm+0xC` < 1) it stores the flags (+0x2A/+0x2B/+0x2C) and requests part
/// loads (`+8 = 2`) when a part id or a flag changed [VAR]. So the override is a compare per
/// call; a real load happens only on a change, and only those are counted and logged.
#[allow(clippy::too_many_arguments)]
extern "C" fn build_hook(asm: usize, a2: usize, a3: usize, a4: u8, a5: u8, a6: u8, a7: u8, a8: u8, a9: usize) -> u8 {
    let mut low = a6;
    let mut replica = false;
    if FULL_PARTS.load(Relaxed) {
        replica = std::panic::catch_unwind(|| is_replica_asm(asm)).unwrap_or(false);
        if replica {
            low = 0;
        }
    }
    let idle = replica && asm_idle(asm);
    let f: extern "C" fn(usize, usize, usize, u8, u8, u8, u8, u8, usize) -> u8 =
        unsafe { std::mem::transmute(ORIG_BUILD.load(Relaxed)) };
    let r = f(asm, a2, a3, a4, a5, low, a7, a8, a9);
    if idle && !asm_idle(asm) {
        let _ = std::panic::catch_unwind(|| note_load(asm, a6));
    }
    r
}

/// No part load pending on this ChrAsm model.
fn asm_idle(asm: usize) -> bool {
    read::<i32>(asm + ASM_PENDING_A).is_some_and(|v| v < 1) && read::<i32>(asm + ASM_PENDING_B).is_some_and(|v| v < 1)
}

fn is_replica_asm(asm: usize) -> bool {
    if asm < 0x10000 || asm == LOCAL_ASM.load(Relaxed) {
        return false;
    }
    CREATING.load(Relaxed)
        || REPLICA_ASM.iter().any(|a| a.load(Relaxed) == asm)
        || CREATED.iter().any(|c| {
            let c = c.load(Relaxed);
            c != 0 && read_ptr(c + CHR_ASM_MODEL) == Some(asm)
        })
}

/// A friend's model started loading parts (a real load, not the per-frame check).
fn note_load(asm: usize, game_low: u8) {
    LOAD_COUNT.fetch_add(1, Relaxed);
    if LOAD_LOGGED.fetch_add(1, Relaxed) < LOAD_LOGS {
        let asked = if game_low != 0 { "the game asked for _L" } else { "the game asked for full detail too" };
        log::line(&format!("fx: parts: friend's model {asm:#x} loads parts full-detail ({asked})"));
    }
}

// --- fx_debug: TAE events (vtable slot of SprjChrTaeAnimEvent) -------------------------------------

fn install_tae_hook() {
    if TAE_TRIED.swap(true, Relaxed) {
        return;
    }
    let base = module_base();
    let orig = base + TAE_SLOT_FN;
    let new = tae_hook as *const () as usize;
    ORIG_TAE.store(orig, Relaxed);
    if hook::patch_code(TAE_SLOT_RVA, &(orig as u64).to_le_bytes(), &(new as u64).to_le_bytes(), "fx TAE event slot 2 (0x2A7AE80)") {
        log::line("fx: TAE event logging installed (SprjChrTaeAnimEvent slot 2 = dispatcher 0xB59170, fx_debug)");
    }
}

/// Event types worth a line: 1 attack, 33 switch weapon, 66/67 SpEffect, 96 FFX, 118 blade FFX,
/// 128/129/131 sounds, 790 default trail off.
fn tae_type_logged(ty: i32) -> bool {
    matches!(ty, 1 | 33 | 66 | 67 | 96 | 112 | 118 | 120 | 128 | 129 | 131 | 132 | 790)
}

/// StateInfo condition field of the event types that have one (FINDINGS.txt layouts).
fn tae_cond_offset(ty: i32) -> Option<usize> {
    match ty {
        1 => Some(0xE),
        96 | 129 => Some(0x10),
        _ => None,
    }
}

extern "C" fn tae_hook(this: usize, ev: usize) -> usize {
    if FX_DEBUG.load(Relaxed) {
        let _ = std::panic::catch_unwind(|| tae_note(this, ev));
    }
    let f: extern "C" fn(usize, usize) -> usize = unsafe { std::mem::transmute(ORIG_TAE.load(Relaxed)) };
    f(this, ev)
}

fn tae_note(this: usize, ev: usize) {
    let Some(chr) = read_ptr(this + 0x10).filter(|&c| c >= 0x10000) else { return };
    let who = if chr == LOCAL_CHR.load(Relaxed) {
        "player".to_string()
    } else if let Some(i) = REPLICA_CHR.iter().position(|a| a.load(Relaxed) == chr) {
        format!("replica{i}")
    } else {
        return;
    };
    let ty = read::<i32>(ev).unwrap_or(-1);
    if !tae_type_logged(ty) || read::<u8>(ev + EV_BEGIN).unwrap_or(0) == 0 {
        return;
    }
    let mut params = [0u8; 24];
    if let Some(p) = read_ptr(ev + EV_PARAMS).filter(|&p| p >= 0x10000) {
        read_bytes(p, &mut params);
    }
    let cond = tae_cond_offset(ty).and_then(|o| {
        let si = u16::from_le_bytes([params[o], params[o + 1]]);
        (si != 0).then(|| {
            let list = read_ptr(chr + CHR_SP_LIST).unwrap_or(0);
            let f: extern "C" fn(usize, u32) -> u8 = unsafe { std::mem::transmute(module_base() + HAS_STATE_RVA) };
            (si, list >= 0x10000 && f(list, si as u32) != 0)
        })
    });
    let rec = TaeRec { who, anim: anim_of(chr), ty, params, cond };
    match TAE_Q.try_lock() {
        Ok(mut q) if q.len() < 256 => q.push_back(rec),
        _ => {
            TAE_DROPPED.fetch_add(1, Relaxed);
        }
    }
}

// --- fx_debug: per-frame state lines (game thread) ---------------------------------------------------

const LOG_WINDOW: Duration = Duration::from_secs(2);
const LOG_LINES_PER_WINDOW: u32 = 60;

fn debug_line(s: &mut State, now: Instant, msg: &str) -> bool {
    if s.log_window.is_none_or(|t| now.duration_since(t) >= LOG_WINDOW) {
        if s.log_lines > LOG_LINES_PER_WINDOW {
            log::line(&format!("fx: ({} lines left out in the last window)", s.log_lines - LOG_LINES_PER_WINDOW));
        }
        s.log_window = Some(now);
        s.log_lines = 0;
    }
    s.log_lines += 1;
    if s.log_lines <= LOG_LINES_PER_WINDOW {
        log::line(msg);
        true
    } else {
        false
    }
}

fn debug_frame(s: &mut State, local: usize, now: Instant) {
    let mut watched: Vec<(String, usize)> = Vec::new();
    if local != 0 {
        watched.push(("player".into(), local));
    }
    for (i, a) in REPLICA_CHR.iter().enumerate() {
        let c = a.load(Relaxed);
        if c != 0 {
            watched.push((format!("replica{i}"), c));
        }
    }
    s.seen_sp.retain(|(c, _)| watched.iter().any(|(_, w)| w == c));
    s.seen_status.retain(|(c, _)| watched.iter().any(|(_, w)| w == c));
    for (who, chr) in &watched {
        // SpEffect changes, with the stateInfo of the new ones.
        let now_sp = speffects(*chr);
        let mut ids: Vec<i32> = now_sp.iter().map(|&(id, _)| id).collect();
        ids.sort_unstable();
        ids.dedup();
        let prev = s.seen_sp.iter().position(|(c, _)| c == chr);
        let old = prev.map(|p| s.seen_sp[p].1.clone()).unwrap_or_default();
        if prev.is_none() || old != ids {
            let added: Vec<String> = ids
                .iter()
                .filter(|id| !old.contains(id))
                .map(|id| {
                    let si = now_sp.iter().find(|(i, _)| i == id).map_or(0, |&(_, si)| si);
                    if si != 0 { format!("{id}(si {si})") } else { id.to_string() }
                })
                .collect();
            let gone: Vec<i32> = old.iter().copied().filter(|id| !ids.contains(id)).collect();
            let anim = anim_of(*chr);
            debug_line(s, now, &format!("fx: sp {who} anim {anim:?}: +[{}] -{gone:?}", added.join(" ")));
            match prev {
                Some(p) => s.seen_sp[p].1 = ids,
                None => s.seen_sp.push((*chr, ids)),
            }
        }
        // Status: chr type, parts flag, SE module, weapon SE id offset.
        let asm = read_ptr(chr + CHR_ASM_MODEL).filter(|&a| a >= 0x10000);
        let status = format!(
            "type {:?}, parts low-detail(+0x2B) {:?}, SE module {}, weapon SE offset {:?}",
            read::<u32>(chr + CHR_TYPE),
            asm.and_then(|a| read::<u8>(a + ASM_LOW_DETAIL)),
            read_ptr(chr + CHR_MODULES).and_then(|m| read_ptr(m + MOD_SE)).is_some_and(|p| p >= 0x10000),
            read_ptr(chr + CHR_WEP_INFO).and_then(|w| read_ptr(w + 8)).filter(|&p| p >= 0x10000).and_then(|p| read::<i32>(p + WEP_SE_OFFSET)),
        );
        let prev = s.seen_status.iter().position(|(c, _)| c == chr);
        if prev.is_none_or(|p| s.seen_status[p].1 != status) {
            debug_line(s, now, &format!("fx: status {who} chr {chr:#x}: {status}"));
            match prev {
                Some(p) => s.seen_status[p].1 = status,
                None => s.seen_status.push((*chr, status)),
            }
        }
    }
    let recs: Vec<TaeRec> = TAE_Q.try_lock().map(|mut q| q.drain(..).collect()).unwrap_or_default();
    for r in recs {
        let cond = match r.cond {
            Some((si, true)) => format!(" cond si {si} = has"),
            Some((si, false)) => format!(" cond si {si} = MISSING (event skipped)"),
            None => String::new(),
        };
        let p = &r.params;
        let w = |o: usize| i32::from_le_bytes([p[o], p[o + 1], p[o + 2], p[o + 3]]);
        debug_line(
            s,
            now,
            &format!("fx: tae {} anim {:?} type {} [{} {} {} {}]{cond}", r.who, r.anim, r.ty, w(0), w(4), w(8), w(12)),
        );
    }
    let dropped = TAE_DROPPED.swap(0, Relaxed);
    if dropped > 0 {
        debug_line(s, now, &format!("fx: tae {dropped} events not queued (queue busy/full)"));
    }
    let loads = LOAD_COUNT.swap(0, Relaxed);
    if loads > 0 {
        debug_line(s, now, &format!("fx: parts: {loads} part load(s) of friends' characters started (full-detail)"));
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn levels_select_the_gates() {
        assert!(!gate_allowed(914, 0));
        assert!(gate_allowed(914, 1));
        assert!(gate_allowed(995, 1));
        assert!(!gate_allowed(152, 1));
        assert!(gate_allowed(152, 2));
        assert!(!gate_allowed(0, 2));
        assert!(!gate_allowed(95, 2));
    }

    #[test]
    fn messages_round_trip_and_bad_ones_are_refused() {
        let ids = vec![106000, 140310];
        assert_eq!(decode(&encode(&ids)), Some(ids));
        assert_eq!(decode(&encode(&[])), Some(vec![]));
        let many: Vec<i32> = (1..=40).collect();
        assert_eq!(decode(&encode(&many)).map(|v| v.len()), Some(MAX_IDS));
        assert_eq!(decode(&[]), None);
        assert_eq!(decode(&[2, 0]), None);
        let mut short = encode(&[106000]);
        short.pop();
        assert_eq!(decode(&short), None);
        assert_eq!(decode(&encode(&[-1])), None);
        assert!(encode(&many).len() <= coop_net::MAX_PLAYER_RAW);
    }

    #[test]
    fn the_mirror_gives_up_after_failures_in_a_row() {
        let mut f = Vec::new();
        for _ in 1..MIRROR_GIVE_UP {
            assert_eq!(note_try(&mut f, 106000, false), None);
            assert!(!given_up(&f, 106000));
        }
        assert_eq!(note_try(&mut f, 106000, false), Some(MIRROR_GIVE_UP), "one summary line");
        assert!(given_up(&f, 106000));
        assert_eq!(note_try(&mut f, 106000, false), None, "never logged twice");
        // Another id is counted on its own.
        assert!(!given_up(&f, 140310));
        // A success resets the count.
        let mut g = Vec::new();
        note_try(&mut g, 5, false);
        note_try(&mut g, 5, true);
        assert!(g.is_empty());
        // Judged by presence only: a forced add that shows up on the last try is not given up.
        let mut h = Vec::new();
        for _ in 1..MIRROR_GIVE_UP {
            note_try(&mut h, 7, false);
        }
        assert_eq!(note_try(&mut h, 7, true), None);
        assert!(!given_up(&h, 7));
        // The owner lost it: forgotten, its next activation gets fresh tries.
        forget_unwanted(&mut f, &[140310]);
        assert!(!given_up(&f, 106000));
    }

    #[test]
    fn plan_adds_missing_and_removes_only_what_we_applied() {
        let has = |id: i32| id == 106000 || id == 5;
        let (add, del) = plan(&[106000, 140310], &[], has);
        assert_eq!(add, vec![140310]);
        assert!(del.is_empty());
        // The owner lost 106000 (enchant): ours goes; 5 was never ours and stays.
        let (add, del) = plan(&[], &[106000], has);
        assert!(add.is_empty());
        assert_eq!(del, vec![106000]);
        let (_, del) = plan(&[], &[], has);
        assert!(del.is_empty());
    }

    #[test]
    fn hooked_slots_and_the_forced_add() {
        // SprjChrTaeAnimEvent vtable 0x2A7AE70: slot 2 (+0x10) = the first dispatcher.
        assert_eq!(TAE_SLOT_RVA, 0x2A7AE70 + 2 * 8);
        assert_eq!(TAE_SLOT_FN, 0xB59170);
        // Unforced adds on a remote player are refused by 0x9E9D10.
        assert_eq!(APPLY_FORCE, 1);
    }

    #[test]
    fn tae_condition_offsets_match_the_templates() {
        assert_eq!(tae_cond_offset(96), Some(0x10));
        assert_eq!(tae_cond_offset(129), Some(0x10));
        assert_eq!(tae_cond_offset(1), Some(0xE));
        assert_eq!(tae_cond_offset(128), None);
        assert!(tae_type_logged(96) && tae_type_logged(790) && !tae_type_logged(0));
    }
}
