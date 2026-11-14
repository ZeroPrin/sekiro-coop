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
//! Log volume (review): per-enemy lines only on a replan frame with both players sampled (at
//! most one a second per enemy) and on a top change; 10 s summaries only for enemies with a
//! replan frame, a hit or a top change in the window and a friend in their list; every
//! `aitarget:` line shares a budget of `LineBudget::PER_SEC` a second, the rest is counted
//! and reported at most once per 10 s.
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
//!   `0x62BC30`), `+0x28` the targeting-system owner [VAR]. Owner vtable `0x142961690`
//!   (`SprjAiTargetingSystemOwner`, the vtable whose `+0xD8` is `0x60EAC0`, dump), owner `+0x08`
//!   = AiThink, `+0x10` = the AI's ChrIns (RE-AI-TARGET §1.1); AiThink `+0x7B30` = targeting
//!   system, TS `+0x34` = the replanning byte (`0x60EAC0` reads `[[owner+8]+0x7B30]+0x34`) [VAR].
//!   The owner is followed only when its vtable matches.
//! - Constants checked in the dump: 1000.0 @`0x1432893A4`, 10.0 @`0x1432892A0`, 14.0
//!   @`0x14296327C`, 0.1 @`0x1432890CC`, 50.0 @`0x1432892F0`, 1.0 @`0x14328919C`; the caller
//!   `0x61DB10` stores the result only while the score is <= 100.0 (@`0x14328931C`).
//!
//! Not seen by the hook: records with a score > 100 (frozen; reasons 3 / 5), and records
//! whose target is not a player (NPC allies / enemies).
//!
//! One enemy can hold two records for the same player (the current one and a stale "unseen /
//! forget" one, seen live 2026-11-14): the table keeps its state per record POINTER within
//! each enemy (`Rec`), never per player, so the two never overwrite each other. The current
//! target is the player of the record that carries the top flag; a TARGET line is logged only
//! when that player changes (a hand-over between two records of the same player is silent),
//! and a HIT only when the hit timer rises on the same record. Record pointers are only keys:
//! never dereferenced outside the hook call that passed them; an entry not sampled for
//! `STALE_MS`, or sampled again for another player, starts over (freed and reused memory).
//!
//! `FRAME` is counted by the frame task, not by the AI update. If the game runs AI updates on
//! worker threads that overlap our frame task, one AI frame can straddle two `FRAME` values;
//! the replan counts are then off by a frame now and then (diagnostics only, review note).

use std::panic::AssertUnwindSafe;
use std::sync::atomic::{AtomicU8, AtomicU32, AtomicU64, AtomicUsize, Ordering::{Acquire, Relaxed, Release}};
use std::sync::{Mutex, MutexGuard, OnceLock, PoisonError};
use std::time::Instant;

use crate::game::{Chr, read};
use crate::hook::{PreparedDetour, freeze};
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
/// `SprjAiTargetingSystemOwner` vtable RVA (its `+0xD8` = `0x60EAC0`, dump 2026-11-11).
const OWNER_VTABLE_RVA: usize = 0x2961690;
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

/// Candidate slots: `[0]` = our player, `[1 + peer id]` = that peer's native remote.
pub const SLOTS: usize = MAX_PLAYERS + 1;

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

/// A new hit by this target. The hit timer is set back to 10 s on each hit and otherwise only
/// decays (`0x61DA40`, `0x61DB10`; a merge `0x61CE10` keeps the larger), so any increase
/// between two samples is a hit, a re-hit 0.1 s later included. Two hits between two
/// samples (one frame, or a sample dropped while the table was busy) count once: the counts
/// are a lower bound. A record that appears with a fresh timer counts too.
pub fn hit_rose(prev: Option<f32>, cur: f32) -> bool {
    match prev {
        Some(p) => cur > p + 1e-3,
        None => cur > HIT_TIME - 0.5,
    }
}

/// Rate limit in milliseconds: due when nothing was logged yet or `gap` has passed.
pub fn due(last: u64, now: u64, gap: u64) -> bool {
    last == 0 || now.saturating_sub(last) >= gap
}

/// Shared budget of every `aitarget:` line: `PER_SEC` a second, the rest counted.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct LineBudget {
    sec_start: u64,
    used: u32,
    pub suppressed: u32,
    last_note: u64,
}

impl LineBudget {
    pub const PER_SEC: u32 = 10;
    pub const NOTE_GAP_MS: u64 = 10_000;

    pub fn allow(&mut self, now: u64) -> bool {
        if self.sec_start == 0 || now.saturating_sub(self.sec_start) >= 1_000 {
            self.sec_start = now;
            self.used = 0;
        }
        if self.used < Self::PER_SEC {
            self.used += 1;
            true
        } else {
            self.suppressed += 1;
            false
        }
    }

    /// The "N suppressed" count to log now (at most once per `NOTE_GAP_MS`), reset.
    pub fn note(&mut self, now: u64) -> Option<u32> {
        if self.suppressed == 0 || !due(self.last_note, now, Self::NOTE_GAP_MS) {
            return None;
        }
        self.last_note = now;
        Some(std::mem::take(&mut self.suppressed))
    }
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
    /// The replanning byte; None = not readable (no targeting system behind the owner).
    pub replan: Option<bool>,
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
        replan: Some(false),
    };

    fn replanning(&self) -> bool {
        self.replan == Some(true)
    }
}

pub const NO_SLOT: u8 = u8::MAX;
/// A candidate counts as "in the list" this long after its last sample.
pub const PRESENT_MS: u64 = 500;
/// An enemy entry is reused after this long without a sample.
pub const STALE_MS: u64 = 5_000;
/// Per-enemy replan line gap and summary window.
pub const LINE_GAP_MS: u64 = 1_000;
pub const SUMMARY_MS: u64 = 10_000;

fn is_friend(slot: usize) -> bool {
    (1..SLOTS).contains(&slot)
}

/// One target record of an enemy, keyed by its address (only a key: never dereferenced
/// outside the hook call that passed it).
#[derive(Clone, Copy, Debug)]
pub struct Rec {
    /// The record's address (0 = free entry).
    pub rec: usize,
    pub slot: u8,
    /// Its latest sample (`ms` 0 = not sampled since the entry was (re)taken).
    pub s: Sample,
}

impl Rec {
    pub const FREE: Rec = Rec { rec: 0, slot: NO_SLOT, s: Sample::NONE };
}

/// Records kept per enemy: two per player and spare.
pub const RECS: usize = 2 * SLOTS + 2;

#[derive(Clone, Copy, Debug)]
pub struct Enemy {
    /// The AI's ChrIns (0 = free entry).
    pub ai: usize,
    pub last_ms: u64,
    /// Player slot of the current top record, `NO_SLOT` = a non-player or nothing.
    pub top: u8,
    /// That record's address (0 = none).
    pub top_rec: usize,
    pub top_since_ms: u64,
    /// The player that lost the top flag last (slot, its sample, since when it held it) and
    /// when it lost it.
    pub lost: (u8, Sample, u64),
    pub lost_ms: u64,
    /// Last frame with the replanning byte set, and the frames counted in this window.
    pub replan_frame: u64,
    pub replans: u32,
    /// Replan frames with 2+ players scored on them (this window).
    pub replans_multi: u32,
    pub switches: u32,
    pub hits: [u32; SLOTS],
    /// A friend's record was sampled in this window.
    pub friend_window: bool,
    /// The player's last sample time in any record (for `Appear`), by slot.
    pub seen_ms: [u64; SLOTS],
    pub recs: [Rec; RECS],
    /// Per player, its best-scored record on the latest replan frame (the raw choice, no +50).
    pub on_replan: [Sample; SLOTS],
    pub line_ms: u64,
    pub logged_replan: u64,
    pub window_ms: u64,
}

impl Enemy {
    pub const FREE: Enemy = Enemy {
        ai: 0,
        last_ms: 0,
        top: NO_SLOT,
        top_rec: 0,
        top_since_ms: 0,
        lost: (NO_SLOT, Sample::NONE, 0),
        lost_ms: 0,
        replan_frame: 0,
        replans: 0,
        replans_multi: 0,
        switches: 0,
        hits: [0; SLOTS],
        friend_window: false,
        seen_ms: [0; SLOTS],
        recs: [Rec::FREE; RECS],
        on_replan: [Sample::NONE; SLOTS],
        line_ms: 0,
        logged_replan: 0,
        window_ms: 0,
    };

    /// Player slots with a record sampled within `PRESENT_MS` of `now`.
    pub fn present(&self, now: u64) -> Vec<usize> {
        (0..SLOTS).filter(|&k| self.seen_ms[k] != 0 && now.saturating_sub(self.seen_ms[k]) <= PRESENT_MS).collect()
    }

    /// The latest sample of record `rec` (NONE when not in the table).
    pub fn rec_sample(&self, rec: usize) -> Sample {
        self.recs.iter().find(|r| r.rec == rec && rec != 0).map_or(Sample::NONE, |r| r.s)
    }

    /// The entry of record `rec` sampled for player `slot`: its own (started over when it was
    /// stale or belonged to another player: freed and reused memory), else a free or stale
    /// one, else the least recently sampled.
    fn rec_entry(&mut self, rec: usize, slot: u8, now: u64) -> usize {
        let stale = |r: &Rec| r.rec == 0 || r.s.ms == 0 || now.saturating_sub(r.s.ms) > STALE_MS;
        let i = match self.recs.iter().position(|r| r.rec == rec) {
            Some(i) if self.recs[i].slot == slot && !stale(&self.recs[i]) => return i,
            Some(i) => i,
            None => self
                .recs
                .iter()
                .position(stale)
                .unwrap_or_else(|| (0..RECS).min_by_key(|&j| self.recs[j].s.ms).unwrap_or(0)),
        };
        // The top record's address now holds a record about another player (freed and
        // reused): the top is no longer known.
        if self.top_rec == rec && self.top != slot {
            self.top_rec = 0;
            self.top = NO_SLOT;
        }
        self.recs[i] = Rec { rec, slot, s: Sample::NONE };
        i
    }

    /// Slots scored on the latest replan frame.
    pub fn on_latest_replan(&self) -> Vec<usize> {
        (0..SLOTS).filter(|&k| self.replan_frame != 0 && self.on_replan[k].ms != 0 && self.on_replan[k].frame == self.replan_frame).collect()
    }

    /// A replan line is due: a replan frame not logged yet with 2+ players scored on it, and
    /// none for this enemy within `LINE_GAP_MS`.
    pub fn replan_line_due(&self, now: u64) -> bool {
        self.replan_frame != self.logged_replan && self.on_latest_replan().len() >= 2 && due(self.line_ms, now, LINE_GAP_MS)
    }

    /// The summary window is over: `Some(log it)`. Logged only with something to say (a
    /// replan frame, a hit or a top change) and a friend in the list: bosses never forget a
    /// player, so an idle enemy stays silent.
    pub fn summary_due(&self, now: u64) -> Option<bool> {
        if now.saturating_sub(self.window_ms) < SUMMARY_MS {
            return None;
        }
        let active = self.replans > 0 || self.switches > 0 || self.hits.iter().any(|&h| h > 0);
        Some(active && self.friend_window)
    }
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Event {
    /// A player record gained the top flag. `from` = the player that had it (`NO_SLOT` = a
    /// non-player or nothing); `replan` = on a replan frame (no +50 that frame).
    Top { ai: usize, from: u8, to: u8, replan: Option<bool>, held_ms: u64, s_from: Sample, s_to: Sample },
    /// A hit by this player registered on this machine's record (hit timer back up).
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

/// Keep the log small: an event is logged when a friend is in it, or the enemy has both
/// players in its list (`multi`). Our own solo noticing / losing / hits are not logged.
pub fn worth_logging(ev: &Event, multi: bool) -> bool {
    let friend = |s: u8| is_friend(s as usize);
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

    /// Anything for the frame task: an event, or an enemy sampled within `PRESENT_MS`.
    pub fn active(&self, now: u64) -> bool {
        self.n_events > 0 || self.dropped > 0 || self.enemies.iter().any(|e| e.ai != 0 && now.saturating_sub(e.last_ms) <= PRESENT_MS)
    }

    /// One sample from the hook: record `rec` (its address, a key only) of enemy `ai` about
    /// player `slot`.
    pub fn record(&mut self, ai: usize, rec: usize, slot: usize, s: Sample) {
        if slot >= SLOTS || rec == 0 {
            return;
        }
        let now = s.ms;
        let k = slot as u8;
        let i = self.entry(ai, now);
        let mut events: [Option<Event>; 3] = [None; 3];
        {
            let e = &mut self.enemies[i];
            e.last_ms = now;
            if is_friend(slot) {
                e.friend_window = true;
            }
            // The player appears (any record), or comes back after `STALE_MS`.
            if e.seen_ms[slot] == 0 || now.saturating_sub(e.seen_ms[slot]) > STALE_MS {
                events[0] = Some(Event::Appear { ai, slot: k, cur: s });
            }
            let r = e.rec_entry(rec, k, now);
            let prev = e.recs[r].s;
            // A hit: the timer rose on THIS record. A new record counts with a fresh timer
            // only when no other record of the player is around (a stale duplicate carries
            // the old timer, no hit).
            let rose = if prev.ms != 0 {
                hit_rose(Some(prev.hit_timer), s.hit_timer)
            } else {
                let twin = e.recs.iter().enumerate().any(|(j, x)| {
                    j != r && x.rec != 0 && x.slot == k && x.s.ms != 0 && now.saturating_sub(x.s.ms) <= PRESENT_MS
                });
                !twin && hit_rose(None, s.hit_timer)
            };
            if rose {
                e.hits[slot] += 1;
                events[1] = Some(Event::Hit { ai, slot: k, cur: s, top: e.top });
            }
            e.seen_ms[slot] = now;
            e.recs[r].s = s;
            if s.replanning() {
                if e.replan_frame != s.frame {
                    e.replan_frame = s.frame;
                    e.replans += 1;
                }
                let had = e.on_replan[slot].ms != 0 && e.on_replan[slot].frame == s.frame;
                if !had || s.score > e.on_replan[slot].score {
                    e.on_replan[slot] = s;
                }
                // The second PLAYER scored on the same replan frame (not a second record).
                if !had && e.on_replan.iter().filter(|c| c.ms != 0 && c.frame == s.frame).count() == 2 {
                    e.replans_multi += 1;
                }
            }
            // Top (`+0x21`, set by 0x629720): the current target is the player of the record
            // that carries the flag. One event when that player changes; a hand-over between
            // two records of the same player is silent.
            if s.is_top && e.top_rec != rec {
                let lost_recent = e.lost.0 != NO_SLOT && now.saturating_sub(e.lost_ms) <= PRESENT_MS;
                let (from, s_from, since) = if e.top != NO_SLOT {
                    (e.top, e.rec_sample(e.top_rec), e.top_since_ms)
                } else if lost_recent {
                    e.lost
                } else {
                    (NO_SLOT, Sample::NONE, now)
                };
                if from == k {
                    e.top_since_ms = since;
                } else {
                    events[2] = Some(Event::Top { ai, from, to: k, replan: s.replan, held_ms: now.saturating_sub(since), s_from, s_to: s });
                    e.top_since_ms = now;
                    e.switches += 1;
                }
                e.top = k;
                e.top_rec = rec;
                e.lost = (NO_SLOT, Sample::NONE, 0);
                e.lost_ms = 0;
            } else if !s.is_top && e.top_rec == rec {
                // Lost to another record (its sample comes later this frame) or to a
                // non-player / nothing (no event).
                e.lost = (e.top, s, e.top_since_ms);
                e.lost_ms = now;
                e.top = NO_SLOT;
                e.top_rec = 0;
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
    let (hit_f, top_f, bonus) = factors(s.hit_timer, s.hit_weight, s.top_timer, s.is_top, s.replanning());
    let hit_ago = if s.hit_timer > 0.0 { format!("{:.1}s ago", HIT_TIME - s.hit_timer) } else { "none".to_string() };
    format!(
        "{label} score {:.3} = dist {:.1} m x hitF {:.2} (hit {hit_ago}, w {:.2}) x topF {:.3} + bonus {:.0}{}{} resid {:+.2}; rec state {} {}{}",
        s.score,
        s.dist,
        hit_f,
        s.hit_weight,
        top_f,
        bonus,
        if s.replan.is_none() { " (replan state unknown)" } else { "" },
        if s.is_top { " [top]" } else { "" },
        residual(s.score, s.dist, hit_f, top_f, bonus),
        s.state,
        if s.perceived { "seen" } else { "unseen" },
        if s.forget > 0.0 { format!(" forget {:.0}s", s.forget) } else { String::new() },
    )
}

fn replan_text(r: Option<bool>) -> &'static str {
    match r {
        Some(true) => "on a replan frame",
        Some(false) => "NOT a replan frame",
        None => "replan state unknown",
    }
}

// ===================================================================== game side

/// Player chr handles this frame (`SLOTS` layout, `NO_HANDLE` = none) and positions as f32
/// bits. Written by the frame task, read by the hook.
static HANDLES: [AtomicU32; SLOTS] = [const { AtomicU32::new(NO_HANDLE) }; SLOTS];
static POS: [[AtomicU32; 3]; SLOTS] = [const { [const { AtomicU32::new(0) }; 3] }; SLOTS];
static NOW_MS: AtomicU64 = AtomicU64::new(0);
static FRAME: AtomicU64 = AtomicU64::new(0);
/// The trampoline: published before the site is patched (`install`), so it is never 0 in
/// the detour.
static TRAMP: AtomicUsize = AtomicUsize::new(0);
/// `module base + OWNER_VTABLE_RVA`, set before the patch.
static OWNER_VT: AtomicUsize = AtomicUsize::new(usize::MAX);
static CALLS: AtomicU64 = AtomicU64::new(0);
static PLAYER_CALLS: AtomicU64 = AtomicU64::new(0);
static BUSY: AtomicU64 = AtomicU64::new(0);
static BAD_OWNER: AtomicU64 = AtomicU64::new(0);
static NO_REPLAN: AtomicU64 = AtomicU64::new(0);
static TABLE: Mutex<Table> = Mutex::new(Table::new());

enum Install {
    NotTried,
    /// Trampoline built and published, site not patched yet (a busy engage, retried).
    Prepared(PreparedDetour),
    Done,
    Failed,
}

static INSTALL: Mutex<Install> = Mutex::new(Install::NotTried);
static BUSY_LOG_MS: AtomicU64 = AtomicU64::new(0);

type ScoreFn = extern "C" fn(usize, usize) -> f32;

fn table() -> MutexGuard<'static, Table> {
    TABLE.lock().unwrap_or_else(PoisonError::into_inner)
}

fn start() -> Instant {
    static T: OnceLock<Instant> = OnceLock::new();
    *T.get_or_init(Instant::now)
}

fn now_ms() -> u64 {
    (start().elapsed().as_millis() as u64).max(1)
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
    // Published before the jump to this detour was written (install).
    let orig: ScoreFn = unsafe { std::mem::transmute::<usize, ScoreFn>(TRAMP.load(Acquire)) };
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
    // (0x62BC30, 0x62BCA0); the owner is followed only with its vtable checked, and then the
    // chain 0x60EAC0 dereferences.
    unsafe {
        if field::<u32>(rec + REC_KIND) != KIND_CHR {
            return;
        }
        let Some(slot) = slot_of(field::<u32>(rec + REC_HANDLE)) else { return };
        PLAYER_CALLS.fetch_add(1, Relaxed);
        let owner = field::<usize>(ctx + CTX_OWNER);
        if owner < 0x10000 || field::<usize>(owner) != OWNER_VT.load(Relaxed) {
            BAD_OWNER.fetch_add(1, Relaxed);
            return;
        }
        let ai = field::<usize>(owner + OWNER_CHR);
        if ai < 0x10000 {
            BAD_OWNER.fetch_add(1, Relaxed);
            return;
        }
        let think = field::<usize>(owner + OWNER_THINK);
        let ts = if think >= 0x10000 { field::<usize>(think + THINK_TS) } else { 0 };
        let replan = (ts >= 0x10000).then(|| field::<u8>(ts + TS_REPLAN) != 0);
        if replan.is_none() {
            NO_REPLAN.fetch_add(1, Relaxed);
        }
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
            Ok(mut t) => t.record(ai, rec, slot, s),
            Err(std::sync::TryLockError::Poisoned(p)) => p.into_inner().record(ai, rec, slot, s),
            Err(std::sync::TryLockError::WouldBlock) => {
                BUSY.fetch_add(1, Relaxed);
            }
        }
    }
}

/// Builds the trampoline, publishes it, then patches the site (two phases: the detour never
/// runs without its trampoline). `budget_ms` 0 at start; in the world a busy engage (a thread
/// inside the prologue, or the suspend phase over budget) is retried on a later frame.
fn install(budget_ms: u64) {
    let mut g = INSTALL.lock().unwrap_or_else(PoisonError::into_inner);
    if matches!(*g, Install::Done | Install::Failed) {
        return;
    }
    if matches!(*g, Install::NotTried) {
        OWNER_VT.store(crate::game::module_base() + OWNER_VTABLE_RVA, Relaxed);
        let detour: ScoreFn = score_detour;
        match crate::hook::prepare_detour(SCORE_RVA, &SCORE_PROLOGUE, detour as usize, "aitarget: target score (0x61DE40)") {
            Some(p) => {
                TRAMP.store(p.page, Release);
                *g = Install::Prepared(p);
            }
            None => {
                *g = Install::Failed;
                log::line("aitarget: score hook not installed: no target observation this run");
                return;
            }
        }
    }
    let Install::Prepared(p) = &*g else { return };
    let (site, page) = (p.site, p.page);
    match crate::hook::engage_detour(p, budget_ms) {
        Ok(()) => {
            *g = Install::Done;
            log::line(&format!(
                "hook: aitarget: target score (0x61DE40) at {site:#x} detoured (trampoline {page:#x}); aitarget: observing enemy target scores (ai_retarget = 1; scores are never changed in this build)"
            ));
        }
        Err(freeze::Error::Busy(rip)) => {
            let now = now_ms();
            if due(BUSY_LOG_MS.load(Relaxed), now, 10_000) {
                BUSY_LOG_MS.store(now, Relaxed);
                log::line(&format!("aitarget: score hook busy (thread at {rip:#x}, 0 = over the {budget_ms} ms budget): retried on a later frame"));
            }
        }
        Err(e) => {
            *g = Install::Failed;
            log::line(&format!("aitarget: score hook not installed ({e:?}): no target observation this run"));
        }
    }
}

/// lib.rs at start (title screen): installed only with `ai_retarget >= 1`; a later switch
/// from 0 installs it from the frame task.
pub fn install_at_start() {
    if MODE.load(Relaxed) >= 1 {
        install(0);
    }
}

/// Per-player facts for the lines (from what the hub already has).
#[derive(Default)]
struct Frame {
    labels: [String; SLOTS],
    hp: [Option<i32>; SLOTS],
}

/// Owned by the puppet hub.
#[derive(Default)]
pub struct AiTarget {
    frame: Frame,
    budget: LineBudget,
    dropped: u32,
    dropped_log_ms: u64,
    last_perf_ms: u64,
    last: [u64; 5],
}

/// puppet.rs: a loading screen: no handle is valid any more, every enemy is gone.
pub fn on_loading() {
    for h in &HANDLES {
        h.store(NO_HANDLE, Relaxed);
    }
    *table() = Table::new();
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

    /// Every `aitarget:` line except the budget's own notes goes through here.
    fn line(&mut self, now: u64, msg: &str) {
        if self.budget.allow(now) {
            log::line(msg);
        }
    }

    /// Every in-world frame, after the native remotes were refreshed.
    pub fn tick(&mut self, player: Chr, peers: &PeerSet) {
        let mode = MODE.load(Relaxed);
        let now = now_ms();
        NOW_MS.store(now, Relaxed);
        FRAME.fetch_add(1, Relaxed);

        // Handles and positions for the hook: slot 0 = us, 1 + id = a peer's native remote
        // (its own position: what the AI measures).
        let mut frame = Frame::default();
        let mut used = [false; SLOTS];
        let mut set = |i: usize, chr: Option<Chr>, label: String, hp: Option<i32>| {
            let h = chr.and_then(|c| read::<u32>(c.0 + CHR_HANDLE)).unwrap_or(NO_HANDLE);
            HANDLES[i].store(h, Relaxed);
            if let Some(p) = chr.and_then(|c| c.position()) {
                for k in 0..3 {
                    POS[i][k].store(p[k].to_bits(), Relaxed);
                }
            }
            frame.labels[i] = label;
            frame.hp[i] = hp;
            used[i] = true;
        };
        set(0, Some(player), "me".to_string(), player.hp());
        for p in peers.iter() {
            let i = 1 + p.id.index();
            if i < SLOTS {
                set(i, p.native, format!("peer{}", p.id.slot()), p.hp());
            }
        }
        for (i, h) in HANDLES.iter().enumerate() {
            if !used[i] {
                h.store(NO_HANDLE, Relaxed);
            }
        }
        self.frame = frame;

        if mode == 0 {
            return;
        }
        install(freeze::ENGAGE_BUDGET_MS);
        self.drain(now);
    }

    fn drain(&mut self, now: u64) {
        // Copy out under the lock (only the enemies seen lately), format after: the hook only
        // try-locks. Nothing to do: no copy at all.
        let got = {
            let mut t = table();
            if t.active(now) {
                let (ev, n, d) = t.take_events();
                let live: Vec<Enemy> = t.enemies.iter().filter(|e| e.ai != 0 && now.saturating_sub(e.last_ms) <= PRESENT_MS).copied().collect();
                Some((ev, n, d, live))
            } else {
                None
            }
        };
        if let Some((events, n, dropped, live)) = got {
            self.dropped += dropped;
            for ev in events.iter().take(n).flatten() {
                let multi = live.iter().find(|e| e.ai == ev.ai()).is_some_and(|e| e.present(now).len() >= 2);
                if worth_logging(ev, multi) {
                    let text = self.event_text(ev);
                    self.line(now, &text);
                }
            }
            self.enemy_lines(now, &live);
        }
        self.notes(now);
    }

    fn enemy_lines(&mut self, now: u64, live: &[Enemy]) {
        let mut marks: Vec<(usize, Option<u64>, bool)> = Vec::new();
        for e in live {
            if e.replan_line_due(now) {
                let on = e.on_latest_replan();
                let parts: Vec<String> = on.iter().map(|&k| describe(&self.who(k), &e.on_replan[k])).collect();
                let best = on.iter().copied().max_by(|&a, &b| e.on_replan[a].score.total_cmp(&e.on_replan[b].score)).map_or(NO_SLOT, |k| k as u8);
                let text = format!(
                    "aitarget: {} replan frame (no +50): {} | best {}, top {} for {:.1}s",
                    enemy_name(e.ai),
                    parts.join(" | "),
                    self.label(best),
                    self.label(e.top),
                    now.saturating_sub(e.top_since_ms) as f32 / 1000.0,
                );
                self.line(now, &text);
                marks.push((e.ai, Some(e.replan_frame), false));
            }
            if let Some(log_it) = e.summary_due(now) {
                if log_it {
                    let hits: Vec<String> = (0..SLOTS).filter(|&k| e.hits[k] > 0).map(|k| format!("{} {}", self.label(k as u8), e.hits[k])).collect();
                    let text = format!(
                        "aitarget: {} last {:.0}s: {} replan frame(s) ({} with 2+ players), {} top change(s), hits seen {} (at least)",
                        enemy_name(e.ai),
                        now.saturating_sub(e.window_ms) as f32 / 1000.0,
                        e.replans,
                        e.replans_multi,
                        e.switches,
                        if hits.is_empty() { "none".to_string() } else { hits.join(", ") },
                    );
                    self.line(now, &text);
                }
                marks.push((e.ai, None, true));
            }
        }
        if marks.is_empty() {
            return;
        }
        let mut t = table();
        for (ai, logged, window) in marks {
            if let Some(e) = t.enemies.iter_mut().find(|e| e.ai == ai) {
                if let Some(f) = logged {
                    e.line_ms = now;
                    e.logged_replan = f;
                }
                if window {
                    e.window_ms = now;
                    e.replans = 0;
                    e.replans_multi = 0;
                    e.switches = 0;
                    e.hits = [0; SLOTS];
                    e.friend_window = false;
                }
            }
        }
    }

    /// Rate-limited notes outside the line budget (each at most once per 10 s).
    fn notes(&mut self, now: u64) {
        if self.dropped > 0 && due(self.dropped_log_ms, now, SUMMARY_MS) {
            log::line(&format!("aitarget: {} event(s) dropped (more than {EVENTS} in one frame) in the last 10 s or so", self.dropped));
            self.dropped = 0;
            self.dropped_log_ms = now;
        }
        if let Some(n) = self.budget.note(now) {
            log::line(&format!("aitarget: {n} line(s) suppressed (more than {} a second)", LineBudget::PER_SEC));
        }
        if due(self.last_perf_ms, now, SUMMARY_MS) {
            let cur = [&CALLS, &PLAYER_CALLS, &BUSY, &BAD_OWNER, &NO_REPLAN].map(|a| a.load(Relaxed));
            if self.last_perf_ms != 0 && cur[0] > self.last[0] {
                let secs = now.saturating_sub(self.last_perf_ms) as f32 / 1000.0;
                log::line(&format!(
                    "aitarget: score calls {:.0}/s, {:.0}/s about a player; in {secs:.0}s {} sample(s) dropped (table busy), {} skipped (owner vtable not 0x142961690), {} with the replan state unknown",
                    (cur[0] - self.last[0]) as f32 / secs,
                    (cur[1] - self.last[1]) as f32 / secs,
                    cur[2] - self.last[2],
                    cur[3] - self.last[3],
                    cur[4] - self.last[4],
                ));
            }
            self.last_perf_ms = now;
            self.last = cur;
        }
    }

    fn event_text(&self, ev: &Event) -> String {
        match *ev {
            Event::Top { ai, from, to, replan, held_ms, s_from, s_to } => {
                let mut parts = vec![describe(&format!("new {}", self.who(to as usize)), &s_to)];
                let first = !((from as usize) < SLOTS && s_from.ms != 0);
                if !first {
                    parts.push(describe(&format!("old {}", self.who(from as usize)), &s_from));
                }
                let held = if first { "no player target before".to_string() } else { format!("old held {:.1}s", held_ms as f32 / 1000.0) };
                format!(
                    "aitarget: {} TARGET {} -> {} ({}; {held}): {}",
                    enemy_name(ai),
                    self.label(from),
                    self.label(to),
                    replan_text(replan),
                    parts.join(" | "),
                )
            }
            Event::Hit { ai, slot, cur, top } => format!(
                "aitarget: {} HIT by {} registered on this PC's AI (top {}): {}",
                enemy_name(ai),
                self.who(slot as usize),
                self.label(top),
                describe(&self.label(slot), &cur),
            ),
            Event::Appear { ai, slot, cur } => format!(
                "aitarget: {} knows {} now: {}",
                enemy_name(ai),
                self.who(slot as usize),
                describe(&self.label(slot), &cur),
            ),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn s(ms: u64, frame: u64, score: f32) -> Sample {
        Sample { ms, frame, score, hit_weight: 1.0, ..Sample::NONE }
    }

    fn rp(ms: u64, frame: u64, score: f32) -> Sample {
        Sample { replan: Some(true), ..s(ms, frame, score) }
    }

    /// The (one) record address of player `slot` in the older tests.
    fn rec(slot: usize) -> usize {
        0x1000 + 0x100 * slot
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
        let (h, t, b) = factors(10.0, 1.0, 14.0, true, false);
        let score = dist_term(10.0) * h * t + b;
        assert!(residual(score, 10.0, h, t, b).abs() < 1e-4);
        // A targetPriority of -1 (event hide) shows as the residual.
        assert!((residual(score - 1.0, 10.0, h, t, b) + 1.0).abs() < 1e-4);
    }

    #[test]
    fn hit_detection() {
        assert!(hit_rose(Some(3.0), 10.0));
        assert!(hit_rose(Some(9.9), 10.0), "a re-hit 0.1 s after the last one");
        assert!(!hit_rose(Some(9.99), 9.98), "decay is no hit");
        assert!(!hit_rose(Some(10.0), 10.0), "no change");
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
        // Enemy sees us (top), then the friend (slot 1) appears with a hit.
        t.record(a, rec(0), 0, Sample { is_top: true, ..s(100, 1, 51.0) });
        t.record(a, rec(1), 1, Sample { hit_timer: 10.0, ..s(100, 1, 1.9) });
        let (ev, n, d) = t.take_events();
        assert_eq!((n, d), (4, 0));
        assert!(matches!(ev[0], Some(Event::Appear { slot: 0, .. })));
        assert!(matches!(ev[1], Some(Event::Top { from: NO_SLOT, to: 0, .. })));
        assert!(matches!(ev[2], Some(Event::Appear { slot: 1, .. })));
        assert!(matches!(ev[3], Some(Event::Hit { slot: 1, top: 0, .. })));
        // Replan frame: the friend wins, we lose the top flag.
        t.record(a, rec(0), 0, rp(200, 2, 1.0));
        t.record(a, rec(1), 1, Sample { is_top: true, hit_timer: 9.0, ..rp(200, 2, 2.0) });
        let (ev, n, _) = t.take_events();
        assert_eq!(n, 1, "one switch, one event, whichever record is scored first");
        assert!(matches!(ev[0], Some(Event::Top { from: 0, to: 1, replan: Some(true), held_ms: 100, .. })));
        // The other order: the new top is scored before the old one loses its flag.
        t.record(a, rec(0), 0, Sample { is_top: true, ..s(300, 3, 2.0) });
        t.record(a, rec(1), 1, s(300, 3, 2.0));
        let (ev, n, _) = t.take_events();
        assert_eq!(n, 1);
        assert!(matches!(ev[0], Some(Event::Top { from: 1, to: 0, replan: Some(false), .. })));
        let e = t.enemies[t.entry(a, 300)];
        assert_eq!((e.replans, e.replans_multi, e.top, e.hits[1], e.switches), (1, 1, 0, 1, 3));
        assert!(e.friend_window);
        assert_eq!(e.on_replan[0].frame, 2);
        assert_eq!(e.on_replan[1].score, 2.0);
    }

    #[test]
    fn unknown_replan_state_is_not_counted() {
        let mut t = Table::new();
        t.record(9, rec(0), 0, Sample { replan: None, ..s(10, 1, 1.0) });
        t.record(9, rec(1), 1, Sample { replan: None, ..s(10, 1, 1.0) });
        let e = t.enemies[t.entry(9, 10)];
        assert_eq!((e.replans, e.replans_multi), (0, 0));
        assert!(describe("me", &e.rec_sample(rec(0))).contains("replan state unknown"));
    }

    #[test]
    fn replan_counted_once_per_frame() {
        let mut t = Table::new();
        for f in 1..=3 {
            t.record(7, rec(0), 0, rp(f * 10, f, 1.0));
            t.record(7, rec(1), 1, rp(f * 10, f, 1.0));
        }
        t.record(7, rec(0), 0, s(40, 4, 1.0));
        let e = t.enemies[t.entry(7, 40)];
        assert_eq!(e.replans, 3);
        assert_eq!(e.replans_multi, 3);
    }

    /// The bug of 2026-11-14: an enemy holds a current and a stale ("unseen / forget") record
    /// for the same player; per player they overwrote each other every frame and logged
    /// `TARGET me -> me` and HIT lines. Per record: nothing but the first target.
    #[test]
    fn two_records_of_one_player_do_not_flap() {
        for (slot, label) in [(0usize, "me"), (1, "peer0")] {
            for cur_first in [true, false] {
                let mut t = Table::new();
                let (a, cur, old) = (0x900, 0x1_0000 + slot, 0x2_0000 + slot);
                for f in 1..=120u64 {
                    let ms = f * 16;
                    // Current: top, its hit timer decaying from 8 s. Stale: unseen, forgetting,
                    // an older (lower) timer, also decaying.
                    let c = Sample { is_top: true, hit_timer: 8.0 - f as f32 * 0.016, perceived: true, ..s(ms, f, 51.0) };
                    let o = Sample { hit_timer: 3.0 - f as f32 * 0.016, forget: 10.0, ..s(ms, f, 0.5) };
                    if cur_first {
                        t.record(a, cur, slot, c);
                        t.record(a, old, slot, o);
                    } else {
                        t.record(a, old, slot, o);
                        t.record(a, cur, slot, c);
                    }
                }
                let (ev, n, _) = t.take_events();
                let ev: Vec<Event> = ev.into_iter().take(n).flatten().collect();
                let tops = ev.iter().filter(|e| matches!(e, Event::Top { .. })).count();
                let hits = ev.iter().filter(|e| matches!(e, Event::Hit { .. })).count();
                assert_eq!((tops, hits), (1, 0), "{label} current first {cur_first}: {ev:?}");
                assert!(matches!(ev.iter().find(|e| matches!(e, Event::Top { .. })), Some(Event::Top { from: NO_SLOT, .. })));
                let e = t.enemies[t.entry(a, 120 * 16)];
                assert_eq!((e.switches, e.hits[slot], e.top, e.top_rec), (1, 0, slot as u8, cur));
            }
        }
    }

    #[test]
    fn top_moves_between_records_and_players() {
        let mut t = Table::new();
        let a = 0x901;
        let (me1, me2, friend) = (0xA0, 0xA8, 0xB0);
        t.record(a, me1, 0, Sample { is_top: true, ..s(100, 1, 51.0) });
        t.take_events();
        // The top moves to another record of us (same frame, both orders): no TARGET line,
        // the holding time goes on.
        t.record(a, me1, 0, s(200, 2, 1.0));
        t.record(a, me2, 0, Sample { is_top: true, ..s(200, 2, 51.0) });
        t.record(a, me1, 0, Sample { is_top: true, ..s(300, 3, 51.0) });
        t.record(a, me2, 0, s(300, 3, 1.0));
        assert_eq!(t.take_events().1, 0);
        let e = t.enemies[t.entry(a, 300)];
        assert_eq!((e.top, e.top_rec, e.top_since_ms, e.switches), (0, me1, 100, 1));
        // A real change to the friend: one line from us, with our current record's sample.
        t.record(a, me1, 0, s(400, 4, 1.0));
        t.record(a, friend, 1, Sample { is_top: true, ..s(400, 4, 51.0) });
        let (ev, n, _) = t.take_events();
        let ev: Vec<Event> = ev.into_iter().take(n).flatten().collect();
        assert!(matches!(ev[..], [Event::Appear { slot: 1, .. }, Event::Top { from: 0, to: 1, held_ms: 300, .. }]), "{ev:?}");
        // Our stale record says "not top" now and then: nothing.
        t.record(a, me2, 0, s(500, 5, 0.5));
        t.record(a, friend, 1, Sample { is_top: true, ..s(500, 5, 51.0) });
        assert_eq!(t.take_events().1, 0);
        // The top goes to a non-player (no sample) and comes back to the friend long after:
        // one line from "other/none".
        t.record(a, friend, 1, s(600, 6, 1.0));
        assert_eq!(t.take_events().1, 0);
        t.record(a, friend, 1, Sample { is_top: true, ..s(600 + PRESENT_MS + 100, 7, 51.0) });
        let (ev, n, _) = t.take_events();
        assert_eq!(n, 1);
        assert!(matches!(ev[0], Some(Event::Top { from: NO_SLOT, to: 1, .. })));
    }

    #[test]
    fn hits_count_per_record() {
        let mut t = Table::new();
        let a = 0x902;
        let (cur, old) = (0xC0, 0xC8);
        t.record(a, cur, 1, Sample { hit_timer: 4.0, ..s(100, 1, 1.0) });
        // A duplicate record appears with a fresh-looking timer: no hit.
        t.record(a, old, 1, Sample { hit_timer: 9.8, ..s(100, 1, 1.0) });
        // The timer rises on the current record: one hit; the stale one decays.
        t.record(a, cur, 1, Sample { hit_timer: 10.0, ..s(200, 2, 1.0) });
        t.record(a, old, 1, Sample { hit_timer: 9.7, ..s(200, 2, 1.0) });
        let (ev, n, _) = t.take_events();
        let hits: Vec<Event> = ev.into_iter().take(n).flatten().filter(|e| matches!(e, Event::Hit { .. })).collect();
        assert_eq!(hits.len(), 1, "{hits:?}");
        assert!(matches!(hits[0], Event::Hit { slot: 1, cur: Sample { ms: 200, .. }, .. }));
        // A record of a player seen for the first time with a fresh timer: a hit (as before).
        t.record(a, 0xD0, 0, Sample { hit_timer: 10.0, ..s(300, 3, 1.0) });
        assert_eq!(t.enemies[t.entry(a, 300)].hits, { let mut h = [0; SLOTS]; h[0] = 1; h[1] = 1; h });
    }

    #[test]
    fn replan_multi_counts_players_not_records() {
        let mut t = Table::new();
        t.record(5, 0xE0, 0, rp(10, 1, 1.0));
        t.record(5, 0xE8, 0, rp(10, 1, 3.0));
        let e = t.enemies[t.entry(5, 10)];
        assert_eq!((e.replans, e.replans_multi, e.on_replan[0].score), (1, 0, 3.0), "the better record of the player");
        assert!(!e.replan_line_due(10));
        t.record(5, 0xF0, 1, rp(10, 1, 2.0));
        assert_eq!(t.enemies[t.entry(5, 10)].replans_multi, 1);
    }

    /// Freed and reused record memory: a stale entry starts over, and an address that comes
    /// back about another player is a new record (and no longer the top).
    #[test]
    fn record_addresses_are_reused_safely() {
        let mut t = Table::new();
        let a = 0x903;
        t.record(a, 0x10, 0, Sample { is_top: true, hit_timer: 2.0, ..s(100, 1, 51.0) });
        t.take_events();
        // Same address, the friend now: not "our" record any more, top unknown.
        t.record(a, 0x10, 1, Sample { hit_timer: 2.0, ..s(200, 2, 1.0) });
        let e = t.enemies[t.entry(a, 200)];
        assert_eq!((e.top, e.top_rec), (NO_SLOT, 0));
        assert_eq!(e.recs.iter().filter(|r| r.rec == 0x10).count(), 1);
        let (ev, n, _) = t.take_events();
        assert!(ev.into_iter().take(n).flatten().all(|e| matches!(e, Event::Appear { slot: 1, .. })), "no hit, no top");
        // More records than entries: the oldest go, the table never grows.
        for k in 0..(RECS as u64 + 5) {
            t.record(a, 0x100 + k as usize, (k % 2) as usize, s(300 + k, 3, 1.0));
        }
        let e = t.enemies[t.entry(a, 400)];
        assert_eq!(e.recs.iter().filter(|r| r.rec != 0).count(), RECS);
        assert!(e.recs.iter().all(|r| r.rec != 0x10), "the oldest went first");
        // A record not sampled for STALE_MS starts over: its timer is no rise.
        let mut t = Table::new();
        t.record(a, 0x20, 0, Sample { hit_timer: 1.0, ..s(100, 1, 1.0) });
        t.record(a, 0x20, 0, Sample { hit_timer: 5.0, ..s(100 + STALE_MS + 1, 2, 1.0) });
        assert_eq!(t.enemies[t.entry(a, 100 + STALE_MS + 1)].hits[0], 0);
    }

    #[test]
    fn table_reuses_stale_and_never_grows() {
        let mut t = Table::new();
        for ai in 1..=(ENEMIES + 10) {
            t.record(ai, rec(0), 0, s(ai as u64, 1, 1.0));
        }
        assert_eq!(t.enemies.iter().filter(|e| e.ai != 0).count(), ENEMIES);
        // Bad slot: ignored.
        t.record(1, rec(SLOTS), SLOTS, s(1, 1, 1.0));
        let mut t = Table::new();
        t.record(1, rec(0), 0, s(1, 1, 1.0));
        let i = t.entry(2, 1 + STALE_MS + 1);
        assert_eq!(i, 0);
        assert_eq!(t.enemies[0].ai, 2);
    }

    #[test]
    fn events_overflow_is_counted() {
        let mut t = Table::new();
        for ai in 1..=(EVENTS + 5) {
            t.record(ai, rec(0), 0, s(1, 1, 1.0));
        }
        let (_, n, d) = t.take_events();
        assert_eq!(n, EVENTS);
        assert_eq!(d, 5);
        assert_eq!(t.take_events().1, 0);
    }

    #[test]
    fn idle_table_needs_no_copy() {
        let mut t = Table::new();
        assert!(!t.active(1_000));
        t.record(3, rec(0), 0, s(1_000, 1, 1.0));
        assert!(t.active(1_000), "an event waits");
        t.take_events();
        assert!(t.active(1_400), "seen within PRESENT_MS");
        assert!(!t.active(1_000 + PRESENT_MS + 1));
    }

    #[test]
    fn describe_has_the_parts() {
        let line = describe("peer1", &Sample { dist: 4.0, hit_timer: 10.0, is_top: true, ..rp(1, 1, 1.992) });
        assert!(line.contains("hitF 2.00") && line.contains("bonus 0") && line.contains("[top]") && line.contains("hit 0.0s ago"), "{line}");
    }

    #[test]
    fn solo_events_stay_out_of_the_log() {
        let top = |from, to| Event::Top { ai: 1, from, to, replan: Some(false), held_ms: 0, s_from: Sample::NONE, s_to: Sample::NONE };
        assert!(!worth_logging(&top(NO_SLOT, 0), false), "an enemy noticing only us");
        assert!(!worth_logging(&Event::Hit { ai: 1, slot: 0, cur: Sample::NONE, top: 0 }, false));
        assert!(!worth_logging(&Event::Appear { ai: 1, slot: 0, cur: Sample::NONE }, false));
        assert!(worth_logging(&top(0, 1), false));
        assert!(worth_logging(&top(1, NO_SLOT), false));
        assert!(worth_logging(&top(0, SLOTS as u8 - 1), false), "peer id 5 = slot 6");
        assert!(worth_logging(&Event::Hit { ai: 1, slot: 1, cur: Sample::NONE, top: 0 }, false), "the friend's hit: open point 3");
        assert!(worth_logging(&Event::Appear { ai: 1, slot: 0, cur: Sample::NONE }, true));
        assert_eq!(top(0, 1).ai(), 1);
    }

    /// Review: the global budget holds every second, the rest is counted and reported at
    /// most once per 10 s.
    #[test]
    fn line_budget_bounds() {
        let mut b = LineBudget::default();
        let mut logged = 0;
        // 100 lines a second for 30 s (every 10 ms).
        let mut notes = Vec::new();
        for ms in (1..=30_000u64).step_by(10) {
            if b.allow(ms) {
                logged += 1;
            }
            if let Some(n) = b.note(ms) {
                notes.push((ms, n));
            }
        }
        assert!(logged <= 31 * LineBudget::PER_SEC as usize, "{logged}");
        assert!(logged >= 29 * LineBudget::PER_SEC as usize, "{logged}");
        assert!(notes.len() <= 4, "{notes:?}");
        for w in notes.windows(2) {
            assert!(w[1].0 - w[0].0 >= LineBudget::NOTE_GAP_MS);
        }
        // Everything over budget is accounted for.
        let total: u32 = notes.iter().map(|n| n.1).sum::<u32>() + b.suppressed;
        assert_eq!(total as usize + logged, 3_000);
        // Quiet: no note.
        let mut q = LineBudget::default();
        assert!(q.allow(5));
        assert_eq!(q.note(20_000), None);
    }

    /// Review: per-enemy lines only on a new replan frame with 2+ players, at most one a
    /// second; summaries only with activity and a friend in the list.
    #[test]
    fn enemy_line_bounds() {
        let mut t = Table::new();
        let a = 0x77;
        // Only us on replan frames: no replan line.
        t.record(a, rec(0), 0, rp(10, 1, 1.0));
        let i = t.entry(a, 10);
        assert!(!t.enemies[i].replan_line_due(10));
        // Both players on replan frames every frame for 3 s (60 fps): one line a second.
        let mut lines = 0;
        for f in 2..=181u64 {
            let ms = f * 16;
            t.record(a, rec(0), 0, rp(ms, f, 1.0));
            t.record(a, rec(1), 1, rp(ms, f, 1.0));
            let e = &mut t.enemies[i];
            if e.replan_line_due(ms) {
                lines += 1;
                e.line_ms = ms;
                e.logged_replan = e.replan_frame;
            }
        }
        assert!((2..=4).contains(&lines), "{lines}");
        // Hold frames (no replan): no line even when due by time.
        let e = &mut t.enemies[i];
        e.line_ms = 0;
        e.logged_replan = e.replan_frame;
        t.record(a, rec(0), 0, s(5_000, 300, 51.0));
        t.record(a, rec(1), 1, s(5_000, 300, 1.0));
        assert!(!t.enemies[i].replan_line_due(5_000));
        // Summary: idle (a boss that just remembers both players) stays silent.
        let idle = Enemy { ai: 1, window_ms: 0, friend_window: true, ..Enemy::FREE };
        assert_eq!(idle.summary_due(SUMMARY_MS - 1), None);
        assert_eq!(idle.summary_due(SUMMARY_MS), Some(false));
        let busy = Enemy { replans: 2, ..idle };
        assert_eq!(busy.summary_due(SUMMARY_MS), Some(true));
        let solo = Enemy { friend_window: false, ..busy };
        assert_eq!(solo.summary_due(SUMMARY_MS), Some(false));
    }
}
