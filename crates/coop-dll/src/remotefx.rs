//! Effects, sounds and look of the friend's native remote ("replica"), docs/RE-REMOTE-FX.md.
//!
//! Two parts, each behind its own [DEBUG] setting (the `fx_debug` diagnostics of test4-test9,
//! a TAE event hook and per-frame `fx:` state lines, were removed in test10; restore them from
//! commit 60f22f6 if needed):
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
//!    "present false"). Level 1 also mirrors the prosthetic tool stateInfos (`GATES_TOOL`),
//!    level 2 also the enchant stateInfos the art events test.
//! 2. **Full-detail parts** (`native_full_parts`, default 1). The ChrAsm model builder
//!    `0xA361C0` passes "not the local player" as the 6th argument of `0xE7B000`, which
//!    stores it at model+0x2B and appends `_L` to every part name (`0x10D1CE0`) [V]. The
//!    `_L` files have byte-identical FLVERs (same dummy polys) and ~4x smaller textures
//!    [V data], so this is only the texture quality the friend noticed. The call at
//!    `0xA363BC` is redirected; for our replicas the argument becomes 0 (as vanilla does
//!    for chr types 3/4/10/11). The builder runs every frame and is cheap while nothing
//!    changes; only real part loads are logged. native.rs registers a new chr at once, so
//!    its first build is full-detail too (test4 built `_L` first, then reloaded).
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
/// `(ParamRef* out, id)`: SpEffectParam row lookup (speffect.rs detours it; we call the entry).
const LOOKUP_RVA: usize = 0x10CEC30;

const CHR_SP_LIST: usize = 0x11D0;
const CHR_ASM_MODEL: usize = 0x2148;
/// SpEffect list: first node at +8; node: next +0x70, id +0x58, row +0x60, state bits +0x1C.
const LIST_FIRST: usize = 0x8;
const NODE_NEXT: usize = 0x70;
const NODE_ID: usize = 0x58;
const NODE_ROW: usize = 0x60;
const NODE_STATE: usize = 0x1C;
/// Entries with any of these bits are ending / not counted by `0xBFEBF0`.
const NODE_INACTIVE: u32 = 0x800C_0003;
const ROW_STATE_INFO: usize = 0x15E;

/// StateInfos the art events test (FINDINGS.txt § 4): 914 no enchantment (106000),
/// 993 / 994 / 995 Mortal Draw / Floating Passage / Dragon Flash upgrades (EMEVD 9930-9932).
const GATES_ART: [u16; 4] = [914, 993, 994, 995];
/// Prosthetic tool stateInfos (level 1 too), docs/RE-REMOTE-FX.md "Протезы на реплике": the
/// resident SpEffect of the active tool (SpEffectParam 127000..127921, e.g. 127010 shuriken
/// = 905, 127810 spear = 941) and the coin-throw upgrades 127060..127063 (996..999, set by
/// common.emevd on 10000 only) gate the tool FFX / sounds of the c0000 prosthetic animations
/// [V params]. The replica loses its tool resident when loadout.rs dresses it and never gets
/// the owner's (test9 client log: 0 "has" of any tool stateInfo on replica0, ~200 MISSING).
const GATES_TOOL: [u16; 29] = [
    905, 907, 908, 911, 913, 915, 916, 918, 919, 920, 922, 923, 925, 926, 927, 929, 930, 931, 934, 935, 941, 942, 943, 945, 946, 996,
    997, 998, 999,
];
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
        1 => GATES_ART.contains(&state_info) || GATES_TOOL.contains(&state_info),
        _ => GATES_ART.contains(&state_info) || GATES_TOOL.contains(&state_info) || GATES_ENCHANT.contains(&state_info),
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

// --- Shared state ---------------------------------------------------------------------------------

/// Each peer's native remote chr and its ChrAsm model (hook-safe lookups), by `PeerId::index`.
static REPLICA_CHR: [AtomicUsize; MAX_PLAYERS] = [const { AtomicUsize::new(0) }; MAX_PLAYERS];
static REPLICA_ASM: [AtomicUsize; MAX_PLAYERS] = [const { AtomicUsize::new(0) }; MAX_PLAYERS];
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
static APPLY_OK: AtomicU8 = AtomicU8::new(0); // 0 unchecked, 1 ok, 2 bad
/// Real part loads of friends' models started through the build hook that were logged.
static LOAD_LOGGED: AtomicU32 = AtomicU32::new(0);
const LOAD_LOGS: u32 = 40;
/// ChrAsm model: pending part loads (`0xE7B000` works only while both are < 1, and sets
/// +8 = 2 when it requests a load) [VAR, o_parts.txt].
const ASM_PENDING_A: usize = 0x8;
const ASM_PENDING_B: usize = 0xC;

static OUTBOX: Mutex<VecDeque<Vec<u8>>> = Mutex::new(VecDeque::new());
/// Per peer index: (ids the owner has, when received).
static WANT: Mutex<[Option<(Vec<i32>, Instant)>; MAX_PLAYERS]> = Mutex::new([const { None }; MAX_PLAYERS]);

/// Game-thread state of `tick`.
struct State {
    last_sent: Option<(Vec<i32>, Instant)>,
    mirror_at: Option<Instant>,
    /// Per peer index: (replica chr, ids we applied to it).
    applied: [(usize, Vec<i32>); MAX_PLAYERS],
    /// Per peer index: (SpEffect id, tries in a row that left it absent) on that replica.
    fails: [Vec<(i32, u32)>; MAX_PLAYERS],
    mirror_logs: u32,
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
    let Ok(mut g) = STATE.lock() else { return };
    let s = g.get_or_insert_with(|| State {
        last_sent: None,
        mirror_at: None,
        applied: [const { (0, Vec::new()) }; MAX_PLAYERS],
        fails: [const { Vec::new() }; MAX_PLAYERS],
        mirror_logs: 0,
    });
    let now = Instant::now();
    if s.mirror_at.is_none_or(|t| now.duration_since(t) >= MIRROR_EVERY) {
        s.mirror_at = Some(now);
        send_own(s, local, now);
        mirror(s, now);
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
        if s.mirror_logs < MIRROR_LOGS {
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
    if LOAD_LOGGED.fetch_add(1, Relaxed) < LOAD_LOGS {
        let asked = if game_low != 0 { "the game asked for _L" } else { "the game asked for full detail too" };
        log::line(&format!("fx: parts: friend's model {asm:#x} loads parts full-detail ({asked})"));
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
    fn prosthetic_tool_gates_are_mirrored_at_level_1() {
        // 127010 shuriken (905), 127810 spear (941), 127330 axe LV4 (920), 127063 coin throw (996).
        for si in [905, 941, 920, 996, 999, 946] {
            assert!(gate_allowed(si, 1) && gate_allowed(si, 2), "tool stateInfo {si}");
            assert!(!gate_allowed(si, 0));
        }
        // Deflect spark variants 902 / 906 (105025 / 105026) are set by the game on the replica
        // itself: never mirrored. 940 is an enchant gate (level 2 only).
        for si in [902, 906, 909, 910, 900, 901] {
            assert!(!gate_allowed(si, 2), "stateInfo {si}");
        }
        assert!(!gate_allowed(940, 1) && gate_allowed(940, 2));
        // Disjoint sets, no duplicates.
        let mut all: Vec<u16> = GATES_ART.iter().chain(&GATES_TOOL).chain(&GATES_ENCHANT).copied().collect();
        let n = all.len();
        all.sort_unstable();
        all.dedup();
        assert_eq!(all.len(), n);
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
    fn the_mirror_add_is_forced() {
        // Unforced adds on a remote player are refused by 0x9E9D10.
        assert_eq!(APPLY_FORCE, 1);
    }
}
