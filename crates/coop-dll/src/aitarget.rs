//! Enemy target choice, observed (`[DEBUG] ai_retarget`, default 1; docs/RE-AI-TARGET.md).
//! Static analysis only; NOT verified live. This build never changes a score.
//!
//! Every AI keeps one NormalEnemy target record per hostile it knows; `TARGET_ENE_0` is the
//! record with the highest score (`0x627EE0`). The score function `0x61DE40`
//! (`float(TargetRecord* rcx, ScoreCtx* rdx)`, result in xmm0) runs every frame per record
//! whose score is <= 100, in the AI update of the machine that simulates the enemy only
//! (NetAI copies run no targeting system). RE-AI-TARGET §3:
//!
//! ```text
//! score = targetPriority + (1 - dist/1000) * hitF * topF + (isTop && !replanFrame ? 50 : 0)
//! hitF  = 1 + hitTimer/10 * hitWeight        topF = 1 + topTimer/14 * 0.1
//! ```
//!
//! We hook it, call the original and return its value unchanged. With `ai_retarget = 1` the
//! records whose target is a player (ours or a friend's native remote) are sampled into a
//! fixed table (no allocation, no logging in the hook); the frame task turns the table into
//! `aitarget:` log lines that answer RE-AI-TARGET §9 open points 1 and 3: how often replan
//! frames come (the +50 is dropped only then) and whether a friend's hit on the enemy shows on
//! this machine's record of the friend's replica (hitF 2).
//!
//! Offsets (all RVAs / offsets for 1.06; [VAR] = read in the decompiled code):
//! - `0x61DE40` prologue `48 89 5C 24 18 48 89 74 24 20 57 48 83 EC 60` (15 bytes, read from
//!   the 2026-11-11 dump; mov/mov/push/sub, nothing RIP-relative). Returns via
//!   `addss xmm0,xmm6` / `movaps xmm0,xmm7` at `0x61DF4E` [VAR, dump bytes].
//! - Record (RE-AI-TARGET §1.2, ctor `0x61C9F0`, used by `0x61DE40` / `0x61DB10`): `+0x00`
//!   u32 state, `+0x08` u32 kind (1 = chr, accessor at `+0x28`), `+0x0C` f32 forget timer,
//!   `+0x14` f32 top timer, `+0x18` f32 hit timer, `+0x1C` f32 hit weight, `+0x20` u8
//!   perceived this tick, `+0x21` u8 is top.
//! - `+0x30` = accessor `+0x08` = the target's chr handle (u32): `0x613840` (accessor vt+0x18)
//!   tests `(int)acc[1] == -1`, `0x612E10` (vt+0x100) looks the ChrIns up with `mov edx,[rbx+8]`
//!   [VAR, decompile + dump bytes; RE-AI-TARGET listed `[rec+0x30]` as [G], now checked].
//! - ChrIns `+0x08` = its handle: `0x6130C0` (accessor vt+0xA0, "handle") returns
//!   `*(u32*)(ChrIns + 8)` of the ChrIns from vt+0x100 [VAR].
//! - Ctx (`0x62BA70`): `+0x00` the AI's own position (vec3, read by the distance term
//!   `0x62BC30`), `+0x28` the targeting-system owner [VAR]. Owner `+0x08` = AiThink, `+0x10` =
//!   the AI's ChrIns (RE-AI-TARGET §1.1); AiThink `+0x7B30` = targeting system, TS `+0x34` = the
//!   replanning byte (`0x60EAC0`, owner vt+0xD8, reads `[[owner+8]+0x7B30]+0x34`) [VAR].
//! - Constants checked in the dump: 1000.0 @`0x1432893A4`, 10.0 @`0x1432892A0`, 14.0
//!   @`0x14296327C`, 0.1 @`0x1432890CC`, 50.0 @`0x1432892F0`, 1.0 @`0x14328919C`; the caller
//!   `0x61DB10` stores the result only while the score is <= 100.0 (@`0x14328931C`).
//!
//! Not seen by the hook: records with a score > 100 (frozen; reasons 3 / 5), and records
//! whose target is not a player (NPC allies / enemies).

use std::panic::AssertUnwindSafe;
use std::sync::atomic::{AtomicBool, AtomicU8, AtomicU32, AtomicU64, AtomicUsize, Ordering::Relaxed};
use std::sync::{Mutex, MutexGuard, OnceLock, PoisonError};
use std::time::Instant;

use crate::game::{Chr, read};
use crate::log;
use crate::peers::{MAX_PLAYERS, PeerSet};

/// `[DEBUG] ai_retarget`: 0 = pass-through (the hook, if installed, returns at once),
/// 1 = observe and log (default). Read every settings re-read.
pub static MODE: AtomicU8 = AtomicU8::new(1);

const SCORE_RVA: usize = 0x61DE40;
/// mov [rsp+18],rbx; mov [rsp+20],rsi; push rdi; sub rsp,60 (dump 2026-11-11).
const SCORE_PROLOGUE: [u8; 15] = [0x48, 0x89, 0x5C, 0x24, 0x18, 0x48, 0x89, 0x74, 0x24, 0x20, 0x57, 0x48, 0x83, 0xEC, 0x60];

// Record (node + 0x20) fields, RE-AI-TARGET §1.2.
const REC_STATE: usize = 0x00;
const REC_KIND: usize = 0x08;
const REC_FORGET: usize = 0x0C;
const REC_TOP_TIMER: usize = 0x14;
const REC_HIT_TIMER: usize = 0x18;
const REC_HIT_WEIGHT: usize = 0x1C;
const REC_PERCEIVED: usize = 0x20;
const REC_IS_TOP: usize = 0x21;
/// Accessor (`+0x28`) + 8: the target chr handle (`0x613840`, `0x612E10`).
const REC_HANDLE: usize = 0x30;
const KIND_CHR: u32 = 1;
/// ChrIns + 8: handle (`0x6130C0`).
const CHR_HANDLE: usize = 0x08;
const CTX_OWNER: usize = 0x28;
const OWNER_THINK: usize = 0x08;
const OWNER_CHR: usize = 0x10;
const THINK_TS: usize = 0x7B30;
const TS_REPLAN: usize = 0x34;
const NO_HANDLE: u32 = u32::MAX;

// Score constants (0x61DE40 and its helpers).
pub const DIST_RANGE: f32 = 1000.0;
pub const HIT_TIME: f32 = 10.0;
pub const TOP_TIME: f32 = 14.0;
pub const TOP_GAIN: f32 = 0.1;
pub const TOP_BONUS: f32 = 50.0;

// ===================================================================== pure, unit-tested

/// The native factors of one record (RE-AI-TARGET §3): `(hitF, topF, bonus)`.
pub fn factors(hit_timer: f32, hit_weight: f32, top_timer: f32, is_top: bool, replan: bool) -> (f32, f32, f32) {
    let hit_f = hit_timer / HIT_TIME * hit_weight + 1.0;
    let top_f = top_timer / TOP_TIME * TOP_GAIN + 1.0;
    let bonus = if is_top && !replan { TOP_BONUS } else { 0.0 };
    (hit_f, top_f, bonus)
}

/// The distance term `0x62BC30`: `1 - d/1000`, 0 beyond 1000 m.
pub fn dist_term(d: f32) -> f32 {
    if d <= DIST_RANGE { 1.0 - d / DIST_RANGE } else { 0.0 }
}

/// What the native score has beyond our reconstruction: targetPriority (SpEffect +0xF8, 0
/// for players in vanilla) plus the error of our distance (a frame-old player position).
pub fn residual(score: f32, d: f32, hit_f: f32, top_f: f32, bonus: f32) -> f32 {
    score - dist_term(d) * hit_f * top_f - bonus
}

/// A new hit by this target: the hit timer is set to 10 s on each hit and only decays
/// otherwise (`0x61DA40`, `0x61DB10`). A record that appears with a fresh timer counts too.
pub fn hit_rose(prev: Option<f32>, cur: f32) -> bool {
    match prev {
        Some(p) => cur > p + 0.5,
        None => cur > HIT_TIME - 0.5,
    }
}

/// Rate limit in milliseconds: due when nothing was logged yet or `gap` has passed.
pub fn due(last: u64, now: u64, gap: u64) -> bool {
    last == 0 || now.saturating_sub(last) >= gap
}

/// One record's latest sample (one per enemy and player).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Sample {
    /// `NOW_MS` / `FRAME` when sampled (0 = never).
    pub ms: u64,
    pub frame: u64,
    pub score: f32,
    pub dist: f32,
    pub hit_timer: f32,
    pub hit_weight: f32,
    pub top_timer: f32,
    pub forget: f32,
    pub state: u32,
    pub perceived: bool,
    pub is_top: bool,
    pub replan: bool,
}

impl Sample {
    pub const NONE: Sample = Sample {
        ms: 0,
        frame: 0,
        score: 0.0,
        dist: 0.0,
        hit_timer: 0.0,
        hit_weight: 0.0,
        top_timer: 0.0,
        forget: 0.0,
        state: 0,
        perceived: false,
        is_top: false,
        replan: false,
    };
}

pub const NO_SLOT: u8 = u8::MAX;
/// A candidate counts as "in the list" this long after its last sample.
pub const PRESENT_MS: u64 = 500;
/// An enemy entry is reused after this long without a sample.
pub const STALE_MS: u64 = 5_000;
/// Per-enemy line gap and summary window.
pub const LINE_GAP_MS: u64 = 1_000;
pub const SUMMARY_MS: u64 = 10_000;

#[derive(Clone, Copy, Debug)]
pub struct Enemy {
    /// The AI's ChrIns (0 = free entry).
    pub ai: usize,
    pub last_ms: u64,
    /// Player slot of the current top record, `NO_SLOT` = a non-player or nothing.
    pub top: u8,
    pub top_since_ms: u64,
    /// The player that lost the top flag last (slot, its sample, since when it held it).
    pub lost: (u8, Sample, u64),
    /// Last frame with the replanning byte set, and the frames counted in this window.
    pub replan_frame: u64,
    pub replans: u32,
    /// Replan frames with 2+ players in the list (this window).
    pub replans_multi: u32,
    pub switches: u32,
    pub hits: [u32; MAX_PLAYERS],
    pub cands: [Sample; MAX_PLAYERS],
    /// The candidates as sampled on the latest replan frame (the raw choice, no +50).
    pub on_replan: [Sample; MAX_PLAYERS],
    pub line_ms: u64,
    pub logged_replan: u64,
    pub window_ms: u64,
}

impl Enemy {
    /// Player slots whose record was sampled within `PRESENT_MS` of `now`.
    pub fn present(&self, now: u64) -> Vec<usize> {
        (0..MAX_PLAYERS).filter(|&k| self.cands[k].ms != 0 && now.saturating_sub(self.cands[k].ms) <= PRESENT_MS).collect()
    }

    pub const FREE: Enemy = Enemy {
        ai: 0,
        last_ms: 0,
        top: NO_SLOT,
        top_since_ms: 0,
        lost: (NO_SLOT, Sample::NONE, 0),
        replan_frame: 0,
        replans: 0,
        replans_multi: 0,
        switches: 0,
        hits: [0; MAX_PLAYERS],
        cands: [Sample::NONE; MAX_PLAYERS],
        on_replan: [Sample::NONE; MAX_PLAYERS],
        line_ms: 0,
        logged_replan: 0,
        window_ms: 0,
    };
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Event {
    /// The top record moved between players (or to / from a non-player): `replan` = it
    /// happened on a replan frame (no +50 that frame).
    Top { ai: usize, from: u8, to: u8, replan: bool, held_ms: u64, s_from: Sample, s_to: Sample },
    /// A hit by this player registered on this machine's record (hit timer back to 10 s).
    Hit { ai: usize, slot: u8, cur: Sample, top: u8 },
    /// A player's record appeared (or came back after `STALE_MS`).
    Appear { ai: usize, slot: u8, cur: Sample },
}

impl Event {
    pub fn ai(&self) -> usize {
        match *self {
            Event::Top { ai, .. } | Event::Hit { ai, .. } | Event::Appear { ai, .. } => ai,
        }
    }
}

/// Keep the log small: an event is logged when a friend (slot >= 1) is in it, or the enemy has
/// both players in its list (`multi`). Our own solo noticing / losing / hits are not logged.
pub fn worth_logging(ev: &Event, multi: bool) -> bool {
    let friend = |s: u8| (1..MAX_PLAYERS).contains(&(s as usize));
    multi
        || match *ev {
            Event::Top { from, to, .. } => friend(from) || friend(to),
            Event::Hit { slot, .. } | Event::Appear { slot, .. } => friend(slot),
        }
}

pub const EVENTS: usize = 64;
pub const ENEMIES: usize = 64;

/// Everything the hook writes. Fixed size: the hook never allocates.
pub struct Table {
    pub enemies: [Enemy; ENEMIES],
    pub events: [Option<Event>; EVENTS],
    pub n_events: usize,
    pub dropped: u32,
}

impl Table {
    pub const fn new() -> Self {
        Table { enemies: [Enemy::FREE; ENEMIES], events: [None; EVENTS], n_events: 0, dropped: 0 }
    }

    fn push(&mut self, e: Event) {
        if self.n_events < EVENTS {
            self.events[self.n_events] = Some(e);
            self.n_events += 1;
        } else {
            self.dropped += 1;
        }
    }

    /// The entry of `ai`: its own, else a free or stale one, else the oldest.
    pub fn entry(&mut self, ai: usize, now: u64) -> usize {
        let mut free = None;
        let mut oldest = (u64::MAX, 0usize);
        for (i, e) in self.enemies.iter().enumerate() {
            if e.ai == ai {
                return i;
            }
            if free.is_none() && (e.ai == 0 || now.saturating_sub(e.last_ms) > STALE_MS) {
                free = Some(i);
            }
            if e.last_ms < oldest.0 {
                oldest = (e.last_ms, i);
            }
        }
        let i = free.unwrap_or(oldest.1);
        self.enemies[i] = Enemy { ai, window_ms: now, ..Enemy::FREE };
        i
    }

    /// One sample from the hook. `frame` / `now` from the frame task.
    pub fn record(&mut self, ai: usize, slot: usize, s: Sample) {
        if slot >= MAX_PLAYERS {
            return;
        }
        let now = s.ms;
        let i = self.entry(ai, now);
        let mut events: [Option<Event>; 3] = [None; 3];
        {
            let e = &mut self.enemies[i];
            e.last_ms = now;
            let prev = e.cands[slot];
            let fresh = prev.ms == 0 || now.saturating_sub(prev.ms) > STALE_MS;
            if fresh {
                events[0] = Some(Event::Appear { ai, slot: slot as u8, cur: s });
            }
            if hit_rose((!fresh).then_some(prev.hit_timer), s.hit_timer) {
                e.hits[slot] += 1;
                events[1] = Some(Event::Hit { ai, slot: slot as u8, cur: s, top: e.top });
            }
            if s.replan {
                if e.replan_frame != s.frame {
                    e.replan_frame = s.frame;
                    e.replans += 1;
                }
                e.on_replan[slot] = s;
                // The second player record scored on the same replan frame.
                if e.on_replan.iter().filter(|c| c.ms != 0 && c.frame == s.frame).count() == 2 {
                    e.replans_multi += 1;
                }
            }
            e.cands[slot] = s;
            // Top (`+0x21`, set by 0x629720): one event when a player record gains it, `from` =
            // the player that had it (also when its loss was seen first, earlier this frame).
            let was = e.top;
            if s.is_top && was != slot as u8 {
                let (from, s_from, since) = if (was as usize) < MAX_PLAYERS {
                    (was, e.cands[was as usize], e.top_since_ms)
                } else {
                    e.lost
                };
                events[2] = Some(Event::Top { ai, from, to: slot as u8, replan: s.replan, held_ms: now.saturating_sub(since), s_from, s_to: s });
                e.top = slot as u8;
                e.top_since_ms = now;
                e.lost = (NO_SLOT, Sample::NONE, now);
                e.switches += 1;
            } else if !s.is_top && was == slot as u8 {
                // Lost to the other player (its record comes later this frame) or to a
                // non-player / nothing (shown as "top other/none" in the next line).
                e.lost = (was, s, e.top_since_ms);
                e.top = NO_SLOT;
                e.top_since_ms = now;
            }
        }
        for ev in events.into_iter().flatten() {
            self.push(ev);
        }
    }

    pub fn take_events(&mut self) -> ([Option<Event>; EVENTS], usize, u32) {
        let out = (self.events, self.n_events, self.dropped);
        self.events = [None; EVENTS];
        self.n_events = 0;
        self.dropped = 0;
        out
    }
}

impl Default for Table {
    fn default() -> Self {
        Self::new()
    }
}

/// Log text of one candidate: native score and its parts.
pub fn describe(label: &str, s: &Sample) -> String {
    let (hit_f, top_f, bonus) = factors(s.hit_timer, s.hit_weight, s.top_timer, s.is_top, s.replan);
    let hit_ago = if s.hit_timer > 0.0 { format!("{:.1}s ago", HIT_TIME - s.hit_timer) } else { "none".to_string() };
    format!(
        "{label} score {:.3} = dist {:.1} m x hitF {:.2} (hit {hit_ago}, w {:.2}) x topF {:.3} + bonus {:.0}{} resid {:+.2}; rec state {} {}{}",
        s.score,
        s.dist,
        hit_f,
        s.hit_weight,
        top_f,
        bonus,
        if s.is_top { " [top]" } else { "" },
        residual(s.score, s.dist, hit_f, top_f, bonus),
        s.state,
        if s.perceived { "seen" } else { "unseen" },
        if s.forget > 0.0 { format!(" forget {:.0}s", s.forget) } else { String::new() },
    )
}

// ===================================================================== game side

/// Player chr handles this frame: `[0]` ours, `[1..]` the peers' native remotes (`NO_HANDLE`
/// = none). Positions as f32 bits. Written by the frame task, read by the hook.
static HANDLES: [AtomicU32; MAX_PLAYERS] = [const { AtomicU32::new(NO_HANDLE) }; MAX_PLAYERS];
static POS: [[AtomicU32; 3]; MAX_PLAYERS] = [const { [const { AtomicU32::new(0) }; 3] }; MAX_PLAYERS];
static NOW_MS: AtomicU64 = AtomicU64::new(0);
static FRAME: AtomicU64 = AtomicU64::new(0);
static TRAMP: AtomicUsize = AtomicUsize::new(0);
/// Install attempted (success or not): never twice.
static TRIED: AtomicBool = AtomicBool::new(false);
static CALLS: AtomicU64 = AtomicU64::new(0);
static PLAYER_CALLS: AtomicU64 = AtomicU64::new(0);
static BUSY: AtomicU64 = AtomicU64::new(0);
static TABLE: Mutex<Table> = Mutex::new(Table::new());

type ScoreFn = extern "C" fn(usize, usize) -> f32;

fn table() -> MutexGuard<'static, Table> {
    TABLE.lock().unwrap_or_else(PoisonError::into_inner)
}

fn start() -> Instant {
    static T: OnceLock<Instant> = OnceLock::new();
    *T.get_or_init(Instant::now)
}

fn slot_of(handle: u32) -> Option<usize> {
    if handle == NO_HANDLE {
        return None;
    }
    HANDLES.iter().position(|h| h.load(Relaxed) == handle)
}

/// Reads a field of a pointer the game itself dereferences in this call chain.
/// # Safety: `addr` must be inside a live game object.
unsafe fn field<T: Copy>(addr: usize) -> T {
    unsafe { (addr as *const T).read_unaligned() }
}

extern "C" fn score_detour(rec: usize, ctx: usize) -> f32 {
    let mut t = TRAMP.load(Relaxed);
    // Installed from the frame task while AI threads run: the stub may be reached before the
    // installer stored the trampoline (it does right after the patch).
    while t == 0 {
        std::hint::spin_loop();
        std::thread::yield_now();
        t = TRAMP.load(Relaxed);
    }
    let orig: ScoreFn = unsafe { std::mem::transmute::<usize, ScoreFn>(t) };
    let score = orig(rec, ctx);
    if MODE.load(Relaxed) == 0 || !crate::mod_alive() {
        return score;
    }
    let _ = std::panic::catch_unwind(AssertUnwindSafe(|| observe(rec, ctx, score)));
    score
}

/// Hot path: two loads for a record that is not about a player.
fn observe(rec: usize, ctx: usize, score: f32) {
    CALLS.fetch_add(1, Relaxed);
    if rec < 0x10000 || ctx < 0x10000 {
        return;
    }
    // SAFETY: the original just read these record fields (0x61DE40) and the ctx fields
    // (0x62BC30, 0x62BCA0); the owner chain is the one 0x60EAC0 dereferences.
    unsafe {
        if field::<u32>(rec + REC_KIND) != KIND_CHR {
            return;
        }
        let Some(slot) = slot_of(field::<u32>(rec + REC_HANDLE)) else { return };
        PLAYER_CALLS.fetch_add(1, Relaxed);
        let owner = field::<usize>(ctx + CTX_OWNER);
        if owner < 0x10000 {
            return;
        }
        let ai = field::<usize>(owner + OWNER_CHR);
        let think = field::<usize>(owner + OWNER_THINK);
        if ai < 0x10000 || think < 0x10000 {
            return;
        }
        let ts = field::<usize>(think + THINK_TS);
        let replan = ts >= 0x10000 && field::<u8>(ts + TS_REPLAN) != 0;
        let own: [f32; 3] = field(ctx);
        let tp = [0, 1, 2].map(|k| f32::from_bits(POS[slot][k].load(Relaxed)));
        let s = Sample {
            ms: NOW_MS.load(Relaxed).max(1),
            frame: FRAME.load(Relaxed),
            score,
            dist: crate::game::distance(own, tp),
            hit_timer: field(rec + REC_HIT_TIMER),
            hit_weight: field(rec + REC_HIT_WEIGHT),
            top_timer: field(rec + REC_TOP_TIMER),
            forget: field(rec + REC_FORGET),
            state: field(rec + REC_STATE),
            perceived: field::<u8>(rec + REC_PERCEIVED) != 0,
            is_top: field::<u8>(rec + REC_IS_TOP) != 0,
            replan,
        };
        // Never wait in the AI update: a busy table drops this sample.
        match TABLE.try_lock() {
            Ok(mut t) => t.record(ai, slot, s),
            Err(std::sync::TryLockError::Poisoned(p)) => p.into_inner().record(ai, slot, s),
            Err(std::sync::TryLockError::WouldBlock) => {
                BUSY.fetch_add(1, Relaxed);
            }
        }
    }
}

fn install() {
    if TRIED.swap(true, Relaxed) {
        return;
    }
    let detour: ScoreFn = score_detour;
    match crate::hook::install_detour(SCORE_RVA, &SCORE_PROLOGUE, detour as usize, "aitarget: target score (0x61DE40)") {
        Some(page) => {
            TRAMP.store(page, Relaxed);
            log::line("aitarget: observing enemy target scores (ai_retarget = 1; scores are never changed in this build)");
        }
        None => log::line("aitarget: score hook not installed: no target observation this run"),
    }
}

/// lib.rs at start (title screen): installed only with `ai_retarget >= 1`; a later switch
/// from 0 installs it from the frame task.
pub fn install_at_start() {
    if MODE.load(Relaxed) >= 1 {
        install();
    }
}

/// Per-player facts for the lines (from what the hub already has: no new reads).
#[derive(Default)]
pub struct Frame {
    labels: [String; MAX_PLAYERS],
    hp: [Option<i32>; MAX_PLAYERS],
}

/// Owned by the puppet hub.
pub struct AiTarget {
    frame: Frame,
    last_perf_ms: u64,
    last_calls: u64,
    last_player_calls: u64,
    last_busy: u64,
}

impl Default for AiTarget {
    fn default() -> Self {
        AiTarget { frame: Frame::default(), last_perf_ms: 0, last_calls: 0, last_player_calls: 0, last_busy: 0 }
    }
}

/// puppet.rs: a loading screen: no handle is valid any more.
pub fn on_loading() {
    for h in &HANDLES {
        h.store(NO_HANDLE, Relaxed);
    }
}

fn enemy_name(ai: usize) -> String {
    let c = Chr(ai);
    let cid = c.character_id().map_or_else(|| "?".to_string(), |v| format!("c{v:04}"));
    let eid = c.entity_id().map_or_else(|| "?".to_string(), |v| v.to_string());
    let hp = c.hp().map_or_else(|| "?".to_string(), |v| v.to_string());
    format!("{cid} id {eid} ({ai:#x}, hp {hp})")
}

impl AiTarget {
    fn label(&self, slot: u8) -> String {
        match self.frame.labels.get(slot as usize) {
            Some(l) if !l.is_empty() => l.clone(),
            _ if slot == NO_SLOT => "other/none".to_string(),
            _ => format!("slot{slot}"),
        }
    }

    fn who(&self, slot: usize) -> String {
        let hp = self.frame.hp.get(slot).copied().flatten();
        format!("{} (hp {})", self.label(slot as u8), hp.map_or_else(|| "?".to_string(), |h| h.to_string()))
    }

    /// Every in-world frame, after the native remotes were refreshed.
    pub fn tick(&mut self, player: Chr, peers: &PeerSet) {
        let mode = MODE.load(Relaxed);
        let now = (start().elapsed().as_millis() as u64).max(1);
        NOW_MS.store(now, Relaxed);
        FRAME.fetch_add(1, Relaxed);

        // Handles and positions for the hook (index 0 = us, then the peers in id order).
        let mut frame = Frame::default();
        let mut set = |i: usize, chr: Option<Chr>, pos: Option<[f32; 3]>, label: String, hp: Option<i32>| {
            let h = chr.and_then(|c| read::<u32>(c.0 + CHR_HANDLE)).unwrap_or(NO_HANDLE);
            HANDLES[i].store(h, Relaxed);
            if let Some(p) = pos {
                for k in 0..3 {
                    POS[i][k].store(p[k].to_bits(), Relaxed);
                }
            }
            frame.labels[i] = label;
            frame.hp[i] = hp;
        };
        set(0, Some(player), player.position(), "me".to_string(), player.hp());
        let mut i = 1;
        for p in peers.iter() {
            if i >= MAX_PLAYERS {
                break;
            }
            set(i, p.native, p.view.map(|v| v.0).or(p.snapshot.map(|s| s.pos)), format!("peer{}", p.id.slot()), p.hp());
            i += 1;
        }
        for h in HANDLES.iter().skip(i) {
            h.store(NO_HANDLE, Relaxed);
        }
        self.frame = frame;

        if mode == 0 {
            return;
        }
        install();
        self.drain(now);
    }

    fn drain(&mut self, now: u64) {
        // Copy out under the lock, format after (the hook only try-locks).
        let (events, n, dropped, snapshot) = {
            let mut t = table();
            let (ev, n, d) = t.take_events();
            (ev, n, d, t.enemies)
        };
        for ev in events.iter().take(n).flatten() {
            let multi = snapshot.iter().find(|e| e.ai == ev.ai()).is_some_and(|e| e.present(now).len() >= 2);
            if worth_logging(ev, multi) {
                self.log_event(ev);
            }
        }
        if dropped > 0 {
            log::line(&format!("aitarget: {dropped} event(s) dropped (more than {EVENTS} in one frame)"));
        }

        let mut marks: Vec<(usize, u64, bool)> = Vec::new();
        for e in snapshot.iter().filter(|e| e.ai != 0 && now.saturating_sub(e.last_ms) <= PRESENT_MS) {
            let present = e.present(now);
            if present.len() >= 2 && due(e.line_ms, now, LINE_GAP_MS) {
                // Prefer the latest replan frame (the raw choice) when there was one since the last line.
                let rp = e.replan_frame != 0 && e.replan_frame != e.logged_replan && present.iter().filter(|&&k| e.on_replan[k].frame == e.replan_frame).count() >= 2;
                let (kind, src) = if rp { ("replan frame (no +50)", &e.on_replan) } else { ("hold", &e.cands) };
                let parts: Vec<String> = present.iter().map(|&k| describe(&self.who(k), &src[k])).collect();
                let winner = present.iter().copied().max_by(|&a, &b| src[a].score.total_cmp(&src[b].score)).map_or(NO_SLOT, |k| k as u8);
                log::line(&format!(
                    "aitarget: {} {kind}: {} | best {} , top {} for {:.1}s",
                    enemy_name(e.ai),
                    parts.join(" | "),
                    self.label(winner),
                    self.label(e.top),
                    now.saturating_sub(e.top_since_ms) as f32 / 1000.0,
                ));
                marks.push((e.ai, if rp { e.replan_frame } else { e.logged_replan }, true));
            }
            if now.saturating_sub(e.window_ms) < SUMMARY_MS {
                continue;
            }
            // The window restarts either way; it is logged only with both players involved.
            marks.push((e.ai, 0, false));
            let friend_hits = e.hits.iter().skip(1).any(|&h| h > 0);
            if present.len() >= 2 || e.replans_multi > 0 || friend_hits {
                let hits: Vec<String> = (0..MAX_PLAYERS).filter(|&k| e.hits[k] > 0).map(|k| format!("{} {}", self.label(k as u8), e.hits[k])).collect();
                log::line(&format!(
                    "aitarget: {} last {:.0}s: {} replan frame(s) ({} with 2+ players), {} top change(s), hits seen {}",
                    enemy_name(e.ai),
                    now.saturating_sub(e.window_ms) as f32 / 1000.0,
                    e.replans,
                    e.replans_multi,
                    e.switches,
                    if hits.is_empty() { "none".to_string() } else { hits.join(", ") },
                ));
            }
        }
        if !marks.is_empty() {
            let mut t = table();
            for (ai, logged, line) in marks {
                if let Some(e) = t.enemies.iter_mut().find(|e| e.ai == ai) {
                    if line {
                        e.line_ms = now;
                        e.logged_replan = logged;
                    } else {
                        e.window_ms = now;
                        e.replans = 0;
                        e.replans_multi = 0;
                        e.switches = 0;
                        e.hits = [0; MAX_PLAYERS];
                    }
                }
            }
        }

        if due(self.last_perf_ms, now, SUMMARY_MS) {
            let (c, p, b) = (CALLS.load(Relaxed), PLAYER_CALLS.load(Relaxed), BUSY.load(Relaxed));
            if self.last_perf_ms != 0 && c > self.last_calls {
                let secs = now.saturating_sub(self.last_perf_ms) as f32 / 1000.0;
                log::line(&format!(
                    "aitarget: score calls {:.0}/s, {:.0}/s about a player, {} sample(s) dropped (table busy) in {secs:.0}s",
                    (c - self.last_calls) as f32 / secs,
                    (p - self.last_player_calls) as f32 / secs,
                    b - self.last_busy,
                ));
            }
            self.last_perf_ms = now;
            self.last_calls = c;
            self.last_player_calls = p;
            self.last_busy = b;
        }
    }

    fn log_event(&self, ev: &Event) {
        match *ev {
            Event::Top { ai, from, to, replan, held_ms, s_from, s_to } => {
                let mut parts = Vec::new();
                if (to as usize) < MAX_PLAYERS {
                    parts.push(describe(&format!("new {}", self.who(to as usize)), &s_to));
                }
                if (from as usize) < MAX_PLAYERS && s_from.ms != 0 {
                    parts.push(describe(&format!("old {}", self.who(from as usize)), &s_from));
                }
                log::line(&format!(
                    "aitarget: {} TARGET {} -> {} ({}; old held {:.1}s): {}",
                    enemy_name(ai),
                    self.label(from),
                    self.label(to),
                    if replan { "on a replan frame" } else { "NOT a replan frame" },
                    held_ms as f32 / 1000.0,
                    parts.join(" | "),
                ));
            }
            Event::Hit { ai, slot, cur, top } => log::line(&format!(
                "aitarget: {} HIT by {} registered on this PC's AI (top {}): {}",
                enemy_name(ai),
                self.who(slot as usize),
                self.label(top),
                describe(&self.label(slot), &cur),
            )),
            Event::Appear { ai, slot, cur } => log::line(&format!(
                "aitarget: {} knows {} now: {}",
                enemy_name(ai),
                self.who(slot as usize),
                describe(&self.label(slot), &cur),
            )),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn s(ms: u64, frame: u64, score: f32) -> Sample {
        Sample { ms, frame, score, hit_weight: 1.0, ..Sample::NONE }
    }

    #[test]
    fn factors_match_the_native_formula() {
        // Fresh hit (10 s), weight 1: hitF 2. New top (14 s): topF 1.1. Top, no replan: +50.
        let (h, t, b) = factors(10.0, 1.0, 14.0, true, false);
        assert!((h - 2.0).abs() < 1e-6 && (t - 1.1).abs() < 1e-6 && b == 50.0);
        // Replan frame: the +50 is gone even for the top record.
        assert_eq!(factors(0.0, 1.0, 0.0, true, true), (1.0, 1.0, 0.0));
        assert_eq!(factors(0.0, 1.0, 0.0, false, false).2, 0.0);
        // Weight 0 (targetSys_DmgEffectRate 0): hits ignored.
        assert_eq!(factors(10.0, 0.0, 0.0, false, false).0, 1.0);
    }

    #[test]
    fn distance_term_and_residual() {
        assert_eq!(dist_term(0.0), 1.0);
        assert!((dist_term(10.0) - 0.99).abs() < 1e-6);
        assert_eq!(dist_term(1000.0), 0.0);
        assert_eq!(dist_term(5000.0), 0.0);
        // score of a top record 10 m away, hit just now, no priority: residual 0.
        let (h, t, b) = factors(10.0, 1.0, 14.0, true, false);
        let score = dist_term(10.0) * h * t + b;
        assert!(residual(score, 10.0, h, t, b).abs() < 1e-4);
        // A targetPriority of -1 (event hide) shows as the residual.
        assert!((residual(score - 1.0, 10.0, h, t, b) + 1.0).abs() < 1e-4);
    }

    #[test]
    fn hit_detection() {
        assert!(hit_rose(Some(3.0), 10.0));
        assert!(!hit_rose(Some(9.99), 9.98), "decay is no hit");
        assert!(!hit_rose(Some(0.0), 0.0));
        assert!(hit_rose(None, 10.0), "a record created by a hit");
        assert!(!hit_rose(None, 0.0), "a record created by sight");
    }

    #[test]
    fn rate_limit() {
        assert!(due(0, 5, 1000));
        assert!(!due(1000, 1999, 1000));
        assert!(due(1000, 2000, 1000));
    }

    #[test]
    fn appear_hit_and_top_change_events() {
        let mut t = Table::new();
        let a = 0x5000;
        // Enemy sees us (top), then the friend appears with a hit.
        t.record(a, 0, Sample { is_top: true, ..s(100, 1, 51.0) });
        t.record(a, 1, Sample { hit_timer: 10.0, ..s(100, 1, 1.9) });
        let (ev, n, d) = t.take_events();
        assert_eq!((n, d), (4, 0));
        assert!(matches!(ev[0], Some(Event::Appear { slot: 0, .. })));
        assert!(matches!(ev[1], Some(Event::Top { from: NO_SLOT, to: 0, .. })));
        assert!(matches!(ev[2], Some(Event::Appear { slot: 1, .. })));
        assert!(matches!(ev[3], Some(Event::Hit { slot: 1, top: 0, .. })));
        // Replan frame: the friend wins, we lose the top flag.
        t.record(a, 0, Sample { replan: true, ..s(200, 2, 1.0) });
        t.record(a, 1, Sample { replan: true, is_top: true, hit_timer: 9.0, ..s(200, 2, 2.0) });
        let (ev, n, _) = t.take_events();
        assert_eq!(n, 1, "one switch, one event, whichever record is scored first");
        assert!(matches!(ev[0], Some(Event::Top { from: 0, to: 1, replan: true, held_ms: 100, .. })));
        // The other order: the new top is scored before the old one loses its flag.
        t.record(a, 0, Sample { is_top: true, ..s(300, 3, 2.0) });
        t.record(a, 1, s(300, 3, 2.0));
        let (ev, n, _) = t.take_events();
        assert_eq!(n, 1);
        assert!(matches!(ev[0], Some(Event::Top { from: 1, to: 0, replan: false, .. })));
        let e = t.enemies[t.entry(a, 300)];
        assert_eq!((e.replans, e.replans_multi, e.top, e.hits[1], e.switches), (1, 1, 0, 1, 3));
        assert_eq!(e.on_replan[0].frame, 2);
        assert_eq!(e.on_replan[1].score, 2.0);
    }

    #[test]
    fn replan_counted_once_per_frame() {
        let mut t = Table::new();
        for f in 1..=3 {
            t.record(7, 0, Sample { replan: true, ..s(f * 10, f, 1.0) });
            t.record(7, 1, Sample { replan: true, ..s(f * 10, f, 1.0) });
        }
        t.record(7, 0, s(40, 4, 1.0));
        let e = t.enemies[t.entry(7, 40)];
        assert_eq!(e.replans, 3);
        assert_eq!(e.replans_multi, 3);
    }

    #[test]
    fn table_reuses_stale_and_never_grows() {
        let mut t = Table::new();
        for ai in 1..=(ENEMIES as usize + 10) {
            t.record(ai, 0, s(ai as u64, 1, 1.0));
        }
        assert_eq!(t.enemies.iter().filter(|e| e.ai != 0).count(), ENEMIES);
        // Bad slot: ignored.
        t.record(1, MAX_PLAYERS, s(1, 1, 1.0));
        // A stale entry is reused for a new enemy.
        let mut t = Table::new();
        t.record(1, 0, s(1, 1, 1.0));
        let i = t.entry(2, 1 + STALE_MS + 1);
        assert_eq!(i, 0);
        assert_eq!(t.enemies[0].ai, 2);
    }

    #[test]
    fn events_overflow_is_counted() {
        let mut t = Table::new();
        for ai in 1..=(EVENTS + 5) {
            t.record(ai, 0, s(1, 1, 1.0));
        }
        let (_, n, d) = t.take_events();
        assert_eq!(n, EVENTS);
        assert_eq!(d, 5);
        assert_eq!(t.take_events().1, 0);
    }

    #[test]
    fn describe_has_the_parts() {
        let line = describe("peer1", &Sample { dist: 4.0, hit_timer: 10.0, is_top: true, replan: true, ..s(1, 1, 1.992) });
        assert!(line.contains("hitF 2.00") && line.contains("bonus 0") && line.contains("[top]") && line.contains("hit 0.0s ago"), "{line}");
    }

    #[test]
    fn solo_events_stay_out_of_the_log() {
        let top = |from, to| Event::Top { ai: 1, from, to, replan: false, held_ms: 0, s_from: Sample::NONE, s_to: Sample::NONE };
        assert!(!worth_logging(&top(NO_SLOT, 0), false), "an enemy noticing only us");
        assert!(!worth_logging(&Event::Hit { ai: 1, slot: 0, cur: Sample::NONE, top: 0 }, false));
        assert!(!worth_logging(&Event::Appear { ai: 1, slot: 0, cur: Sample::NONE }, false));
        assert!(worth_logging(&top(0, 1), false));
        assert!(worth_logging(&top(1, NO_SLOT), false));
        assert!(worth_logging(&Event::Hit { ai: 1, slot: 1, cur: Sample::NONE, top: 0 }, false), "the friend's hit: open point 3");
        assert!(worth_logging(&Event::Appear { ai: 1, slot: 0, cur: Sample::NONE }, true));
        assert_eq!(top(0, 1).ai(), 1);
    }

    #[test]
    fn prologue_is_position_independent() {
        assert_eq!(SCORE_PROLOGUE.len(), 15);
        assert_eq!(&SCORE_PROLOGUE[..5], &[0x48, 0x89, 0x5C, 0x24, 0x18]);
    }
}
