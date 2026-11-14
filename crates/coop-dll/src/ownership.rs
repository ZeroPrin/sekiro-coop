//! Host-authored enemy ownership (docs/PLAN-OWNERSHIP.md, test11). NOT verified live.
//!
//! The host alone decides which machine runs each enemy's AI. It keeps one table
//! `netId -> owner slot | nobody` and sends it on the control lane (nsession.rs `Assign`);
//! on every machine `IsOwnedHere` (0xA4A650) answers only from that table for enemy netIds
//! while engaged (nsession.rs `own`). The game's own 22 / 23 keep running, only as the
//! client's liveness gate; their result is ignored.
//!
//! Pure: no game reads. The pieces:
//! - `decide`: the rule for one enemy (distance with a dead band and a minimum hold, the
//!   boss pin, stealth claims judged on the host);
//! - `publish_step`: release-then-acquire; a hand-over away from a client goes through
//!   NOBODY until that client acks, so a lost or late message never makes two brains;
//! - `HostTable`: both of the above over every enemy the host sees;
//! - `LinkOut`: what the host sends one client (delta, full, a full again when its ack is
//!   behind);
//! - `Mirror`: the receiver side (per-entry sequence numbers, full resync, contiguous ack);
//! - `DoubleWatch`: the host's cross-machine two-brains detector;
//! - `KindCache`: boss or ordinary per enemy (a boss answer kept, an ordinary one re-checked).

use std::collections::{HashMap, HashSet, VecDeque};

/// Owner "nobody": NetAI on every machine, no records anywhere (frozen until in range).
pub const NOBODY: u8 = 0xFF;
pub const HOST: u8 = 0;

/// Host decision tick (also run at once on a Claims, Ack, Status-paused or presence change).
pub const DECIDE_EVERY_MS: u64 = 250;
/// An unowned ordinary enemy gets an owner when a present player is within this.
pub const TAKE_M: f32 = 40.0;
/// The current owner stays a candidate up to this.
pub const KEEP_M: f32 = 60.0;
/// Dead band: A -> B only if `d_B + SWITCH_M < d_A`; a first assignment goes to the host
/// unless a client is nearer by more than this.
pub const SWITCH_M: f32 = 8.0;
/// No distance switch within this after the last change (not for an absent owner or a claim).
pub const MIN_HOLD_MS: u64 = 4000;
/// Out of range without a break this long before the entry goes to nobody.
pub const RELEASE_AFTER_MS: u64 = 5000;
/// A boss gets an owner when a present player is within this ...
pub const BOSS_TAKE_M: f32 = 150.0;
/// ... and its owner keeps it up to this.
pub const BOSS_KEEP_M: f32 = 170.0;
/// A boss held by a present client goes back to a present host within this (hysteresis).
pub const BOSS_BACK_M: f32 = 130.0;
/// A claim is refused while another present player is within this of the enemy (the same
/// value as enemies.rs `FIGHT_NEAR_M`, judged on the host now).
pub const FIGHT_NEAR_M: f32 = 20.0;
/// After a claim ends the claimant keeps the enemy at least this long.
pub const CLAIM_HOLD_MS: u64 = 3000;
/// Full resync of the table to every engaged client.
pub const FULL_EVERY_MS: u64 = 5000;
/// A client behind on its acks this long gets a full again (at most once per this).
pub const ACK_RESEND_MS: u64 = 1000;
/// An entry waiting for a release ack this long: a WARN line every this often (it stays
/// nobody meanwhile, never double).
pub const ACK_WARN_MS: u64 = 5000;
/// Client Status heartbeat.
pub const STATUS_EVERY_MS: u64 = 2000;
/// Missing from the host's scan this long before the entry goes to nobody.
pub const UNLOAD_GRACE_MS: u64 = 1000;
/// An enemy classified Ordinary is classified again at most this often (a Boss answer is
/// kept): its entity id or NpcParam row may not be readable when it is first seen.
pub const KIND_RECHECK_MS: u64 = 1000;

/// One player as an enemy's rule sees it. `present` = engaged, alive, not waiting dead in a
/// boss fight, world not paused; `dist` = metres to the enemy (None = position unknown).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Player {
    pub slot: u8,
    pub present: bool,
    pub dist: Option<f32>,
}

impl Player {
    /// Present with a known distance.
    fn here(&self) -> Option<f32> {
        if self.present { self.dist } else { None }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Kind {
    Ordinary,
    /// A table boss or a multi-life miniboss (nsession.rs classifies).
    Boss,
}

/// The host's decision state of one enemy. `owner` is the decided target (what
/// `publish_step` publishes, through nobody when a client must release first).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Entry {
    pub owner: u8,
    pub since_ms: u64,
    pub out_since_ms: Option<u64>,
    pub claim_until_ms: u64,
}

impl Entry {
    pub fn new(now: u64) -> Self {
        Self { owner: NOBODY, since_ms: now, out_since_ms: None, claim_until_ms: 0 }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Why {
    Nearer,
    OwnerAbsent,
    Claim,
    BossHost,
    BossHostAway,
    InRange,
    OutOfRange,
    Unloaded,
}

impl Why {
    pub fn label(self) -> &'static str {
        match self {
            Why::Nearer => "nearer",
            Why::OwnerAbsent => "owner absent",
            Why::Claim => "claim",
            Why::BossHost => "boss: host",
            Why::BossHostAway => "boss: host away",
            Why::InRange => "in range",
            Why::OutOfRange => "out of range",
            Why::Unloaded => "unloaded",
        }
    }
}

/// "host" / "peerN" / "none" (log lines).
pub fn who(slot: u8) -> String {
    match slot {
        HOST => "host".into(),
        NOBODY => "none".into(),
        s => format!("peer{s}"),
    }
}

/// Why `claimant`'s claim on this enemy is refused (None = granted). Bosses are never
/// claimed; nor is an enemy another present player stands within `FIGHT_NEAR_M` of.
pub fn claim_refusal(kind: Kind, claimant: u8, players: &[Player]) -> Option<String> {
    if kind == Kind::Boss {
        return Some("boss".into());
    }
    if !players.iter().any(|p| p.slot == claimant && p.here().is_some()) {
        return Some(format!("{} absent", who(claimant)));
    }
    players
        .iter()
        .filter(|p| p.slot != claimant)
        .find(|p| p.here().is_some_and(|d| d <= FIGHT_NEAR_M))
        .map(|p| format!("{} within {FIGHT_NEAR_M:.0} m", who(p.slot)))
}

/// The nearest of `c` (tie: the lowest slot).
fn nearest(c: impl Iterator<Item = (u8, f32)>) -> Option<(u8, f32)> {
    c.fold(None, |best: Option<(u8, f32)>, (s, d)| match best {
        Some((bs, bd)) if bd < d || (bd == d && bs < s) => Some((bs, bd)),
        _ => Some((s, d)),
    })
}

/// Rule 4: an ordinary enemy's target (None = no player in range).
fn ordinary_target(cur: u8, cur_present: bool, players: &[Player]) -> Option<(u8, Why)> {
    let cands: Vec<(u8, f32)> = players
        .iter()
        .filter_map(|p| p.here().map(|d| (p.slot, d)))
        .filter(|&(s, d)| d <= if s == cur { KEEP_M } else { TAKE_M })
        .collect();
    if cands.is_empty() {
        return None;
    }
    if cur_present && let Some(&(_, d_cur)) = cands.iter().find(|c| c.0 == cur) {
        return match nearest(cands.iter().copied().filter(|c| c.0 != cur)) {
            Some((b, d_b)) if d_b + SWITCH_M < d_cur => Some((b, Why::Nearer)),
            _ => Some((cur, Why::Nearer)),
        };
    }
    let host = cands.iter().find(|c| c.0 == HOST).map(|c| c.1);
    let client = nearest(cands.iter().copied().filter(|c| c.0 != HOST));
    let t = match (host, client) {
        (Some(dh), Some((c, dc))) if dc + SWITCH_M < dh => c,
        (Some(_), _) => HOST,
        (None, Some((c, _))) => c,
        (None, None) => return None,
    };
    Some((t, if cur == NOBODY { Why::InRange } else { Why::Nearer }))
}

/// Rule 3: a boss's target (None = nobody in range). Pinned to a present host within
/// `BOSS_TAKE_M` (`BOSS_KEEP_M` while it holds it, `BOSS_BACK_M` to take it back from a
/// present client); a client only while the host is absent or beyond that.
fn boss_target(cur: u8, cur_present: bool, players: &[Player]) -> Option<(u8, Why)> {
    let dh = players.iter().find(|p| p.slot == HOST).and_then(Player::here);
    if let Some(dh) = dh {
        let lim = if cur == HOST {
            BOSS_KEEP_M
        } else if cur != NOBODY && cur_present {
            BOSS_BACK_M
        } else {
            BOSS_TAKE_M
        };
        if dh <= lim {
            return Some((HOST, Why::BossHost));
        }
    }
    let clients: Vec<(u8, f32)> = players
        .iter()
        .filter(|p| p.slot != HOST)
        .filter_map(|p| p.here().map(|d| (p.slot, d)))
        .filter(|&(s, d)| d <= if s == cur { BOSS_KEEP_M } else { BOSS_TAKE_M })
        .collect();
    if clients.iter().any(|c| c.0 == cur) {
        return Some((cur, Why::BossHostAway));
    }
    nearest(clients.into_iter()).map(|(c, _)| (c, Why::BossHostAway))
}

/// The host's rule for one alive enemy, applied to its entry. Some((new owner, why)) when
/// the target changes. `players` = every player in the session (host first is not needed);
/// a slot missing from it has left (its enemies go at once), a listed one that is not
/// present is dead / paused / waiting (its enemies move to a present player, or stay with
/// it when nobody is in range: a wipe changes nothing). `claimants` = the slots whose
/// stealth claims name this enemy.
pub fn decide(kind: Kind, e: &mut Entry, players: &[Player], claimants: &[u8], now: u64) -> Option<(u8, Why)> {
    let cur = e.owner;
    let listed = cur != NOBODY && players.iter().any(|p| p.slot == cur);
    let cur_present = cur != NOBODY && players.iter().any(|p| p.slot == cur && p.here().is_some());
    let absent = cur != NOBODY && !cur_present;
    // 2. Claims (ordinary enemies; judged here, refused near another player).
    if kind == Kind::Ordinary {
        let mut ok: Vec<u8> = claimants.iter().copied().filter(|&c| claim_refusal(kind, c, players).is_none()).collect();
        ok.sort_unstable();
        ok.dedup();
        if let Some(&low) = ok.first() {
            let w = if ok.contains(&cur) { cur } else { low };
            e.claim_until_ms = now + CLAIM_HOLD_MS;
            e.since_ms = now;
            e.out_since_ms = None;
            if w != cur {
                e.owner = w;
                return Some((w, Why::Claim));
            }
            return None;
        }
        if cur_present && now < e.claim_until_ms {
            return None;
        }
    }
    // 3 / 4. The target by distance.
    let t = match kind {
        Kind::Boss => boss_target(cur, cur_present, players),
        Kind::Ordinary => ordinary_target(cur, cur_present, players),
    };
    let Some((t, why)) = t else {
        if cur == NOBODY {
            return None;
        }
        if !listed {
            // Left the session: nobody runs it there any more.
            *e = Entry { owner: NOBODY, since_ms: now, out_since_ms: None, claim_until_ms: 0 };
            return Some((NOBODY, Why::OwnerAbsent));
        }
        if absent {
            // Dead / paused and nobody else in range (a wipe): hold as is.
            return None;
        }
        let out = *e.out_since_ms.get_or_insert(now);
        if now.saturating_sub(out) >= RELEASE_AFTER_MS {
            *e = Entry { owner: NOBODY, since_ms: now, out_since_ms: None, claim_until_ms: 0 };
            return Some((NOBODY, Why::OutOfRange));
        }
        return None;
    };
    e.out_since_ms = None;
    if t == cur {
        return None;
    }
    // 5. Hold: no distance switch within `MIN_HOLD_MS` of the last change, except away
    // from an absent owner or out of "nobody".
    if cur_present && now.saturating_sub(e.since_ms) < MIN_HOLD_MS {
        return None;
    }
    e.owner = t;
    e.since_ms = now;
    Some((t, if absent { Why::OwnerAbsent } else { why }))
}

// --- Publisher: release then acquire ------------------------------------------------

/// `a` is a newer sequence number than `b` (wrapping u32).
pub fn newer(a: u32, b: u32) -> bool {
    a != b && a.wrapping_sub(b) < 0x8000_0000
}

/// `acked` has reached `s` (equal or newer).
pub fn reached(acked: u32, s: u32) -> bool {
    acked == s || newer(acked, s)
}

/// The next sequence number (never 0: 0 = "nothing acked yet").
pub fn next_seq(c: &mut u32) -> u32 {
    *c = c.wrapping_add(1);
    if *c == 0 {
        *c = 1;
    }
    *c
}

/// What the host has published for one enemy.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Pub {
    pub published: u8,
    /// Sequence number of the last publication of this entry.
    pub seq: u32,
    /// A client released it: (that client, the seq it must ack, since ms). Nothing is
    /// published to anyone else before that ack (or before it leaves).
    pub wait: Option<(u8, u32, u64)>,
}

impl Default for Pub {
    fn default() -> Self {
        Self { published: NOBODY, seq: 0, wait: None }
    }
}

/// One publication.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Change {
    pub from: u8,
    pub to: u8,
    pub seq: u32,
    /// `to` is NOBODY because `from` (a client) releases it first; this target follows
    /// after its ack.
    pub pending: Option<u8>,
    /// The release wait this publication ends: (the client waited for, ms, true = it acked /
    /// false = it left).
    pub after: Option<(u8, u64, bool)>,
}

/// One step of the host's release-then-acquire for an entry whose decided owner is `target`.
/// `acked(slot)` = the client's latest contiguous applied seq, `engaged(slot)` = the client
/// is still in the session.
pub fn publish_step(p: &mut Pub, target: u8, acked: impl Fn(u8) -> u32, engaged: impl Fn(u8) -> bool, seq: &mut u32, now: u64) -> Option<Change> {
    let mut after = None;
    if let Some((x, s1, since)) = p.wait {
        if target == x {
            // It goes back to the one releasing it: nobody else ever had it.
            p.wait = None;
        } else if !engaged(x) {
            p.wait = None;
            after = Some((x, now.saturating_sub(since), false));
        } else if reached(acked(x), s1) {
            p.wait = None;
            after = Some((x, now.saturating_sub(since), true));
        } else {
            return None;
        }
    }
    if target == p.published {
        return None;
    }
    let from = p.published;
    if from != HOST && from != NOBODY && engaged(from) {
        let s = next_seq(seq);
        *p = Pub { published: NOBODY, seq: s, wait: Some((from, s, now)) };
        return Some(Change { from, to: NOBODY, seq: s, pending: (target != NOBODY).then_some(target), after: None });
    }
    let s = next_seq(seq);
    p.published = target;
    p.seq = s;
    Some(Change { from, to: target, seq: s, pending: None, after })
}

/// The release wait has run past `ACK_RESEND_MS` (the client gets a full again).
pub fn resend_due(p: &Pub, now: u64) -> bool {
    p.wait.is_some_and(|(_, _, since)| now.saturating_sub(since) >= ACK_RESEND_MS)
}

// --- Host table ------------------------------------------------------------------------

/// One enemy in the host's scan this tick (deduplicated by netId).
#[derive(Clone, Debug)]
pub struct Seen {
    pub net: u16,
    pub entity: i32,
    pub kind: Kind,
    /// HP 0 (or unreadable): no change, the owner keeps it.
    pub dead: bool,
    pub players: Vec<Player>,
}

#[derive(Clone, Debug)]
pub struct Row {
    pub entry: Entry,
    pub kind: Kind,
    pub entity: i32,
    pub publ: Pub,
    /// When this entry was last published (ms).
    pub published_ms: u64,
    pub seen_ms: u64,
    /// The reason of the last decision (logged when a pending publication goes out).
    pub why: Why,
    /// The players as last seen (log distances).
    pub players: Vec<Player>,
}

/// One publication with what led to it.
#[derive(Clone, Debug)]
pub struct Decided {
    pub net: u16,
    pub entity: i32,
    pub kind: Kind,
    pub why: Why,
    pub change: Change,
    pub players: Vec<Player>,
}

#[derive(Default)]
pub struct TickOut {
    pub decided: Vec<Decided>,
    /// Claims refused this tick: (netId, claimant, why).
    pub refused: Vec<(u16, u8, String)>,
    /// Decisions by reason (publications to nobody for a release are not decisions).
    pub whys: Vec<Why>,
}

/// The host's whole table. `seq` is one counter per process: it never resets.
#[derive(Default)]
pub struct HostTable {
    pub rows: HashMap<u16, Row>,
    pub seq: u32,
}

impl HostTable {
    /// One decision tick. `claims` = (slot, its claimed netIds); `acked` / `engaged` as in
    /// `publish_step`.
    pub fn tick(&mut self, seen: &[Seen], claims: &[(u8, Vec<u16>)], acked: impl Fn(u8) -> u32, engaged: impl Fn(u8) -> bool, now: u64) -> TickOut {
        let mut out = TickOut::default();
        let mut here: HashSet<u16> = HashSet::with_capacity(seen.len());
        for s in seen {
            if !here.insert(s.net) {
                continue;
            }
            let row = self.rows.entry(s.net).or_insert_with(|| Row {
                entry: Entry::new(now),
                kind: s.kind,
                entity: s.entity,
                publ: Pub::default(),
                published_ms: now,
                seen_ms: now,
                why: Why::InRange,
                players: Vec::new(),
            });
            row.seen_ms = now;
            row.kind = s.kind;
            row.entity = s.entity;
            row.players.clone_from(&s.players);
            if !s.dead {
                let claimants: Vec<u8> = claims.iter().filter(|(_, ids)| ids.contains(&s.net)).map(|(c, _)| *c).collect();
                for &c in &claimants {
                    if let Some(why) = claim_refusal(s.kind, c, &s.players) {
                        out.refused.push((s.net, c, why));
                    }
                }
                if let Some((_, why)) = decide(s.kind, &mut row.entry, &s.players, &claimants, now) {
                    row.why = why;
                    out.whys.push(why);
                }
            }
        }
        for (&net, row) in self.rows.iter_mut() {
            if !here.contains(&net) && now.saturating_sub(row.seen_ms) >= UNLOAD_GRACE_MS && row.entry.owner != NOBODY {
                row.entry = Entry::new(now);
                row.why = Why::Unloaded;
                out.whys.push(Why::Unloaded);
            }
            if let Some(change) = publish_step(&mut row.publ, row.entry.owner, &acked, &engaged, &mut self.seq, now) {
                row.published_ms = now;
                out.decided.push(Decided { net, entity: row.entity, kind: row.kind, why: row.why, change, players: row.players.clone() });
            }
        }
        // Unloaded and settled: forgotten (a full no longer names it = nobody there).
        self.rows.retain(|n, r| here.contains(n) || r.entry.owner != NOBODY || r.publ.published != NOBODY || r.publ.wait.is_some());
        out.decided.sort_by_key(|d| d.change.seq);
        out
    }

    /// Every published owner (a full): (netId, owner, entry seq), owners only.
    pub fn full(&self) -> Vec<(u16, u8, u32)> {
        let mut v: Vec<(u16, u8, u32)> =
            self.rows.iter().filter(|(_, r)| r.publ.published != NOBODY).map(|(&n, r)| (n, r.publ.published, r.publ.seq)).collect();
        v.sort_unstable();
        v
    }

    /// Entries waiting for a release ack.
    pub fn waiting(&self) -> usize {
        self.rows.values().filter(|r| r.publ.wait.is_some()).count()
    }

    /// An entry has waited for `slot`'s release ack past `ACK_RESEND_MS`.
    pub fn wait_overdue(&self, slot: u8, now: u64) -> bool {
        self.rows.values().any(|r| r.publ.wait.is_some_and(|w| w.0 == slot) && resend_due(&r.publ, now))
    }

    /// The seq of the last publication of `net` (0 = none).
    pub fn change_seq(&self, net: u16) -> u32 {
        self.rows.get(&net).map_or(0, |r| r.publ.seq)
    }

    pub fn published(&self, net: u16) -> u8 {
        self.rows.get(&net).map_or(NOBODY, |r| r.publ.published)
    }
}

// --- What the host sends one client ------------------------------------------------------

/// One Assign to a client.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Send {
    Full,
    /// A delta on top of the Assign with this seq (the previous one on this link).
    Delta { base: u32 },
}

/// Host, per engaged client: the last Assign seq sent to it (None = it needs a full), the
/// recent sends, its last full.
#[derive(Default, Clone, Debug)]
pub struct LinkOut {
    pub sent: Option<u32>,
    /// (ms, seq) of recent sends; the front is the newest one at least `ACK_RESEND_MS` old.
    history: VecDeque<(u64, u32)>,
    pub full_at: u64,
}

impl LinkOut {
    /// The client has not acked a seq we sent at least `ACK_RESEND_MS` ago: a delta was
    /// lost (a gap stops its ack until a full), or its ack was. An ack that only trails the
    /// newest send by a round trip (steady churn) is not behind.
    pub fn behind(&mut self, acked: u32, now: u64) -> bool {
        while self.history.len() >= 2 && now.saturating_sub(self.history[1].0) >= ACK_RESEND_MS {
            self.history.pop_front();
        }
        self.history.front().is_some_and(|&(t, s)| now.saturating_sub(t) >= ACK_RESEND_MS && !reached(acked, s))
    }

    /// What to send this tick (None = nothing) and whether it is a resend: a full when the
    /// link is new, every `FULL_EVERY_MS` (`full_all`), when the client is behind or an entry
    /// waits for its release ack past `ACK_RESEND_MS` (`wait_overdue`; at most one such full
    /// per `ACK_RESEND_MS`), or when the delta would not fit (`too_big`); else the delta if
    /// there is one. `seq` = the table's seq now.
    #[allow(clippy::too_many_arguments)]
    pub fn plan(&mut self, acked: u32, seq: u32, has_delta: bool, full_all: bool, wait_overdue: bool, too_big: bool, now: u64) -> Option<(Send, bool)> {
        let resend = (self.behind(acked, now) || wait_overdue) && now.saturating_sub(self.full_at) >= ACK_RESEND_MS;
        let send = match self.sent {
            None => Send::Full,
            Some(_) if full_all || resend || too_big => Send::Full,
            Some(base) if has_delta => Send::Delta { base },
            Some(_) => return None,
        };
        self.sent = Some(seq);
        self.history.push_back((now, seq));
        if send == Send::Full {
            self.full_at = now;
        }
        Some((send, resend))
    }
}

// --- Mirror: the receiving side --------------------------------------------------------

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct MirrorRow {
    pub owner: u8,
    pub seq: u32,
    /// When the owner last changed here (ms).
    pub at_ms: u64,
}

/// A client's copy of the host's table. `applied` is the newest message seq applied
/// without a gap since the last full (what the client acks); a lost delta stops it until
/// the next full, so an ack always covers every release before it.
#[derive(Default)]
pub struct Mirror {
    pub rows: HashMap<u16, MirrorRow>,
    pub applied: u32,
    last: Option<u32>,
    /// A full has been applied (the table is complete).
    pub ready: bool,
    pub fulls: u32,
    pub deltas: u32,
    pub stale: u32,
    pub gaps: u32,
}

impl Mirror {
    /// One Assign: `full` = the whole table (owners only; a stored entry missing from it
    /// with an older or equal seq becomes nobody), else a delta whose `base` must be the
    /// previous message's seq. Returns the owner transitions (netId, old, new).
    pub fn apply(&mut self, full: bool, seq: u32, base: u32, entries: &[(u16, u8, u32)], now: u64) -> Vec<(u16, u8, u8)> {
        let mut out = Vec::new();
        for &(n, owner, es) in entries {
            match self.rows.get_mut(&n) {
                Some(r) if !newer(es, r.seq) => self.stale += 1,
                Some(r) => {
                    if r.owner != owner {
                        out.push((n, r.owner, owner));
                        r.at_ms = now;
                    }
                    r.owner = owner;
                    r.seq = es;
                }
                None => {
                    if owner != NOBODY {
                        out.push((n, NOBODY, owner));
                    }
                    self.rows.insert(n, MirrorRow { owner, seq: es, at_ms: now });
                }
            }
        }
        if full {
            let listed: HashSet<u16> = entries.iter().map(|e| e.0).collect();
            for (&n, r) in self.rows.iter_mut() {
                if !listed.contains(&n) && !newer(r.seq, seq) && r.owner != NOBODY {
                    out.push((n, r.owner, NOBODY));
                    *r = MirrorRow { owner: NOBODY, seq, at_ms: now };
                }
            }
            self.rows.retain(|_, r| r.owner != NOBODY || newer(r.seq, seq));
            self.fulls += 1;
            self.applied = seq;
            self.last = Some(seq);
            self.ready = true;
        } else {
            self.deltas += 1;
            if self.last == Some(base) {
                self.applied = seq;
                self.last = Some(seq);
            } else if self.last.is_some() {
                self.gaps += 1;
                self.last = None;
            }
        }
        out
    }

    #[cfg(test)]
    pub fn owner(&self, n: u16) -> u8 {
        self.rows.get(&n).map_or(NOBODY, |r| r.owner)
    }
}

// --- Kind cache --------------------------------------------------------------------------

/// Boss or ordinary per netId for one engagement. A Boss answer is kept; an Ordinary one is
/// asked again at most every `KIND_RECHECK_MS`: on the first sight the entity id (table
/// bosses) or the NpcParam row behind `chr+0x2010` (lives, multi-life minibosses) may not be
/// readable yet, and livesync `lives0_of` reads 1 then. [Г] Both are set when the chr is
/// built, before it is in a chr set; not verified live, the "is a boss now" line shows it.
#[derive(Default)]
pub struct KindCache {
    rows: HashMap<u16, (Kind, u64)>,
}

impl KindCache {
    /// The kind of `n` (`is_boss` asked only when due). The bool: it was Ordinary until now
    /// and is a Boss from here (log it).
    pub fn get(&mut self, n: u16, now: u64, is_boss: impl FnOnce() -> bool) -> (Kind, bool) {
        let was = match self.rows.get(&n) {
            Some(&(Kind::Boss, _)) => return (Kind::Boss, false),
            Some(&(Kind::Ordinary, at)) if now.saturating_sub(at) < KIND_RECHECK_MS => return (Kind::Ordinary, false),
            Some(_) => true,
            None => false,
        };
        let k = if is_boss() { Kind::Boss } else { Kind::Ordinary };
        self.rows.insert(n, (k, now));
        (k, was && k == Kind::Boss)
    }

    /// The last answer for `n`, without asking (None = never seen).
    pub fn known(&self, n: u16) -> Option<Kind> {
        self.rows.get(&n).map(|r| r.0)
    }
}

// --- Double watch ------------------------------------------------------------------------

/// Host: an enemy whose AI runs on the host and on a client, counted only when the client
/// has applied the entry's last change (not a hand-over in flight) in two Status reports
/// in a row (>= 2 s apart, far more than a round trip).
#[derive(Default)]
pub struct DoubleWatch {
    prev: HashMap<u8, HashSet<u16>>,
}

impl DoubleWatch {
    pub fn report(&mut self, slot: u8, applied_seq: u32, running: &[u16], host_running: &HashSet<u16>, change_seq: impl Fn(u16) -> u32) -> Vec<u16> {
        let now: HashSet<u16> =
            running.iter().copied().filter(|n| host_running.contains(n) && reached(applied_seq, change_seq(*n))).collect();
        let prev = self.prev.insert(slot, now.clone()).unwrap_or_default();
        let mut v: Vec<u16> = now.intersection(&prev).copied().collect();
        v.sort_unstable();
        v
    }

    pub fn forget(&mut self, slot: u8) {
        self.prev.remove(&slot);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const C: u8 = 1;

    fn p(slot: u8, d: f32) -> Player {
        Player { slot, present: true, dist: Some(d) }
    }
    fn gone(slot: u8, d: f32) -> Player {
        Player { slot, present: false, dist: Some(d) }
    }
    fn two(h: f32, c: f32) -> Vec<Player> {
        vec![p(HOST, h), p(C, c)]
    }
    fn fresh() -> Entry {
        Entry::new(0)
    }
    /// Run `decide` from a fresh entry at `now`.
    fn first(kind: Kind, players: &[Player]) -> u8 {
        let mut e = fresh();
        decide(kind, &mut e, players, &[], 10_000);
        e.owner
    }

    #[test]
    fn dead_band_on_the_first_assignment() {
        assert_eq!(first(Kind::Ordinary, &two(10.0, 5.0)), HOST);
        assert_eq!(first(Kind::Ordinary, &two(20.0, 5.0)), C);
        assert_eq!(first(Kind::Ordinary, &two(13.0, 5.0)), HOST, "exactly the band: host");
        assert_eq!(first(Kind::Ordinary, &two(50.0, 39.0)), C, "the host beyond TAKE_M");
        assert_eq!(first(Kind::Ordinary, &two(41.0, 41.0)), NOBODY, "both beyond TAKE_M");
        // A client nearer than another client by any amount; no host: the nearest client.
        assert_eq!(first(Kind::Ordinary, &[p(HOST, 90.0), p(1, 12.0), p(2, 8.0)]), 2);
        assert_eq!(first(Kind::Ordinary, &[p(HOST, 90.0), p(2, 10.0), p(3, 10.0)]), 2, "tie: lowest slot");
    }

    #[test]
    fn no_flips_at_the_thresholds() {
        // 39 / 41 m alternating every tick for 30 s (host only near): taken once, kept.
        let mut e = fresh();
        let mut changes = 0;
        for i in 0..120u64 {
            let d = if i % 2 == 0 { 39.0 } else { 41.0 };
            if decide(Kind::Ordinary, &mut e, &two(d, 200.0), &[], 1000 + i * 250).is_some() {
                changes += 1;
            }
        }
        assert_eq!((changes, e.owner), (1, HOST));
        // +-7 m around equal distances (inside the band), both near, 30 s: no switch.
        let mut e = fresh();
        let mut changes = 0;
        for i in 0..120u64 {
            let (h, c) = if i % 2 == 0 { (17.0, 10.0) } else { (10.0, 17.0) };
            if decide(Kind::Ordinary, &mut e, &two(h, c), &[], 1000 + i * 250).is_some() {
                changes += 1;
            }
        }
        assert_eq!((changes, e.owner), (1, HOST));
    }

    #[test]
    fn min_hold_blocks_at_3_9_s_and_allows_at_4_s() {
        let mut e = fresh();
        assert_eq!(decide(Kind::Ordinary, &mut e, &two(10.0, 30.0), &[], 1000), Some((HOST, Why::InRange)));
        // The client clearly nearer now.
        assert_eq!(decide(Kind::Ordinary, &mut e, &two(30.0, 5.0), &[], 1000 + 3900), None);
        assert_eq!(e.owner, HOST);
        assert_eq!(decide(Kind::Ordinary, &mut e, &two(30.0, 5.0), &[], 1000 + 4000), Some((C, Why::Nearer)));
        // And the hold again right after that switch.
        assert_eq!(decide(Kind::Ordinary, &mut e, &two(5.0, 30.0), &[], 1000 + 4100), None);
    }

    #[test]
    fn an_absent_owner_bypasses_the_hold() {
        let mut e = fresh();
        decide(Kind::Ordinary, &mut e, &two(10.0, 15.0), &[], 1000);
        assert_eq!(e.owner, HOST);
        // The host dies 100 ms later: the client takes it at once.
        let players = [gone(HOST, 10.0), p(C, 15.0)];
        assert_eq!(decide(Kind::Ordinary, &mut e, &players, &[], 1100), Some((C, Why::OwnerAbsent)));
        // A wipe: both absent, nothing moves.
        let mut e = fresh();
        decide(Kind::Ordinary, &mut e, &two(10.0, 15.0), &[], 1000);
        assert_eq!(decide(Kind::Ordinary, &mut e, &[gone(HOST, 10.0), gone(C, 15.0)], &[], 20_000), None);
        assert_eq!(e.owner, HOST);
        // A client that left the session: its enemies go now (nobody when nobody is near).
        let mut e = Entry { owner: C, ..fresh() };
        assert_eq!(decide(Kind::Ordinary, &mut e, &[p(HOST, 100.0)], &[], 1000), Some((NOBODY, Why::OwnerAbsent)));
        let mut e = Entry { owner: C, ..fresh() };
        assert_eq!(decide(Kind::Ordinary, &mut e, &[p(HOST, 10.0)], &[], 1000), Some((HOST, Why::OwnerAbsent)));
    }

    #[test]
    fn out_of_range_goes_to_nobody_only_after_5_s() {
        let mut e = fresh();
        decide(Kind::Ordinary, &mut e, &two(30.0, 200.0), &[], 0);
        assert_eq!(e.owner, HOST);
        // 55 m: still within KEEP_M.
        for t in (250..20_000).step_by(250) {
            assert_eq!(decide(Kind::Ordinary, &mut e, &two(55.0, 200.0), &[], t), None);
        }
        // Beyond 60 m from t = 20 s; back for one tick at 23 s restarts the clock.
        for t in (20_000..23_000).step_by(250) {
            assert_eq!(decide(Kind::Ordinary, &mut e, &two(61.0, 200.0), &[], t), None);
        }
        decide(Kind::Ordinary, &mut e, &two(59.0, 200.0), &[], 23_000);
        let mut gone_at = None;
        for t in (23_250..40_000).step_by(250) {
            if decide(Kind::Ordinary, &mut e, &two(61.0, 200.0), &[], t).is_some() {
                gone_at = Some(t);
                break;
            }
        }
        assert_eq!(gone_at, Some(23_250 + 5000));
        assert_eq!(e.owner, NOBODY);
    }

    #[test]
    fn boss_pin_and_hand_over() {
        let k = Kind::Boss;
        // The host at 140 m with the client at 1 m: the host.
        let mut e = fresh();
        assert_eq!(decide(k, &mut e, &two(140.0, 1.0), &[], 1000), Some((HOST, Why::BossHost)));
        // Host down: the client at once.
        assert_eq!(decide(k, &mut e, &[gone(HOST, 140.0), p(C, 1.0)], &[], 1100), Some((C, Why::OwnerAbsent)));
        // Host back at 140 m: stays with the client (back needs <= 130 m).
        for t in (1200..20_000).step_by(250) {
            assert_eq!(decide(k, &mut e, &two(140.0, 1.0), &[], t), None);
        }
        // Host at 120 m: back to the host, only after the hold.
        let mut e = Entry { owner: C, since_ms: 50_000, ..fresh() };
        assert_eq!(decide(k, &mut e, &two(120.0, 1.0), &[], 53_000), None);
        assert_eq!(decide(k, &mut e, &two(120.0, 1.0), &[], 54_000), Some((HOST, Why::BossHost)));
        // The host at 160 m with the client at 10 m: the client.
        assert_eq!(first(k, &two(160.0, 10.0)), C);
        // The host keeps it up to 170 m once it holds it.
        let mut e = Entry { owner: HOST, ..fresh() };
        assert_eq!(decide(k, &mut e, &two(165.0, 10.0), &[], 10_000), None);
        assert_eq!(decide(k, &mut e, &two(171.0, 10.0), &[], 10_000), Some((C, Why::BossHostAway)));
        // Nobody within 150 m: nobody.
        assert_eq!(first(k, &two(151.0, 151.0)), NOBODY);
        // A dead client owner and the host at 140 m: the host takes it (no back limit).
        let mut e = Entry { owner: C, ..fresh() };
        assert_eq!(decide(k, &mut e, &[p(HOST, 140.0), gone(C, 5.0)], &[], 1000), Some((HOST, Why::OwnerAbsent)));
    }

    #[test]
    fn claims_are_judged_on_the_host() {
        let o = Kind::Ordinary;
        // Granted with the host at 30 m (the client 5 m, the host owned it).
        let mut e = Entry { owner: HOST, since_ms: 9_000, ..fresh() };
        assert_eq!(decide(o, &mut e, &two(30.0, 5.0), &[C], 10_000), Some((C, Why::Claim)));
        // Refused with the host at 15 m.
        let mut e = Entry { owner: HOST, since_ms: 9_000, ..fresh() };
        assert_eq!(decide(o, &mut e, &two(15.0, 5.0), &[C], 10_000), None);
        assert_eq!(claim_refusal(o, C, &two(15.0, 5.0)).as_deref(), Some("host within 20 m"));
        // Refused on a boss.
        assert_eq!(claim_refusal(Kind::Boss, C, &two(90.0, 5.0)).as_deref(), Some("boss"));
        let mut e = Entry { owner: HOST, since_ms: 9_000, ..fresh() };
        assert_eq!(decide(Kind::Boss, &mut e, &two(90.0, 5.0), &[C], 10_000), None);
        // Both claim (3 players, neither within 20 m of the enemy and the other): the owner keeps it.
        let three = [p(HOST, 80.0), p(1, 21.0), p(2, 25.0)];
        let mut e = Entry { owner: 2, since_ms: 9_000, ..fresh() };
        assert_eq!(decide(o, &mut e, &three, &[1, 2], 10_000), None);
        assert_eq!(e.owner, 2);
        let mut e = fresh();
        assert_eq!(decide(o, &mut e, &three, &[2, 1], 10_000), Some((1, Why::Claim)), "neither owns it: the lowest slot");
        // After the claim ends the hold applies: the host clearly nearer does not take it for
        // 3 s (claim hold) and 4 s (min hold from the last claimed tick).
        let mut e = Entry { owner: HOST, since_ms: 0, ..fresh() };
        decide(o, &mut e, &two(30.0, 5.0), &[C], 10_000);
        decide(o, &mut e, &two(30.0, 5.0), &[C], 11_000);
        assert_eq!(e.owner, C);
        assert_eq!(decide(o, &mut e, &two(25.0, 50.0), &[], 14_900), None);
        assert_eq!(decide(o, &mut e, &two(25.0, 50.0), &[], 15_000), Some((HOST, Why::Nearer)));
    }

    #[test]
    fn dead_enemies_keep_their_owner_and_unloaded_ones_go() {
        let mut t = HostTable::default();
        let seen = |dead: bool, h: f32| Seen { net: 0x800, entity: 1, kind: Kind::Ordinary, dead, players: two(h, 200.0) };
        let all = |_: u8| true;
        let none = |_: u8| 0u32;
        let out = t.tick(&[seen(false, 10.0)], &[], none, all, 1000);
        assert_eq!(out.decided.len(), 1);
        assert_eq!(t.published(0x800), HOST);
        // Dead at 500 m: no change.
        let out = t.tick(&[seen(true, 500.0)], &[], none, all, 20_000);
        assert!(out.decided.is_empty());
        assert_eq!(t.published(0x800), HOST);
        // Not in the scan: kept for UNLOAD_GRACE, then nobody, then forgotten.
        assert!(t.tick(&[], &[], none, all, 20_500).decided.is_empty());
        let out = t.tick(&[], &[], none, all, 21_000);
        assert_eq!(out.decided[0].change.to, NOBODY);
        assert_eq!(out.decided[0].why, Why::Unloaded);
        assert!(t.rows.is_empty());
        assert!(t.full().is_empty());
        // Duplicated scan entries count once.
        let out = t.tick(&[seen(false, 10.0), seen(false, 10.0)], &[], none, all, 30_000);
        assert_eq!(out.decided.len(), 1);
    }

    #[test]
    fn publisher_releases_before_it_acquires() {
        let mut seq = 0u32;
        let all = |_: u8| true;
        // Nobody -> host -> client: one step each (the host drops its bit the same tick).
        let mut p = Pub::default();
        let c = publish_step(&mut p, HOST, |_| 0, all, &mut seq, 0).unwrap();
        assert_eq!((c.from, c.to, c.seq), (NOBODY, HOST, 1));
        let c = publish_step(&mut p, C, |_| 0, all, &mut seq, 0).unwrap();
        assert_eq!((c.from, c.to), (HOST, C));
        assert_eq!(publish_step(&mut p, C, |_| 0, all, &mut seq, 0), None);
        // Client -> host: nobody first, then the host only after the client acks s1.
        let c = publish_step(&mut p, HOST, |_| 2, all, &mut seq, 1000).unwrap();
        assert_eq!((c.from, c.to, c.pending), (C, NOBODY, Some(HOST)));
        let s1 = c.seq;
        assert_eq!(publish_step(&mut p, HOST, |_| s1 - 1, all, &mut seq, 1500), None);
        assert!(!resend_due(&p, 1999) && resend_due(&p, 2000), "the resend flag after 1 s");
        let c = publish_step(&mut p, HOST, |_| s1, all, &mut seq, 2046).unwrap();
        assert_eq!((c.from, c.to, c.after), (NOBODY, HOST, Some((C, 1046, true))));
        // A disengaged client: the next owner at once, no wait.
        let mut p = Pub { published: C, seq: 5, wait: None };
        let c = publish_step(&mut p, HOST, |_| 0, |_| false, &mut seq, 0).unwrap();
        assert_eq!((c.from, c.to), (C, HOST));
        // ... or one that leaves during the wait.
        let mut p = Pub { published: C, seq: 5, wait: None };
        publish_step(&mut p, 2, |_| 0, all, &mut seq, 0).unwrap();
        let c = publish_step(&mut p, 2, |_| 0, |s| s != C, &mut seq, 300).unwrap();
        assert_eq!((c.to, c.after), (2, Some((C, 300, false))));
        // Released to nobody: the wait still holds a later owner back.
        let mut p = Pub { published: C, seq: 5, wait: None };
        let c = publish_step(&mut p, NOBODY, |_| 0, all, &mut seq, 0).unwrap();
        assert_eq!((c.to, c.pending), (NOBODY, None));
        assert_eq!(publish_step(&mut p, HOST, |_| 0, all, &mut seq, 100), None);
        // Back to the releasing client: at once (nobody else had it).
        let c = publish_step(&mut p, C, |_| 0, all, &mut seq, 200).unwrap();
        assert_eq!((c.from, c.to), (NOBODY, C));
        assert_eq!(p.wait, None);
    }

    #[test]
    fn sequence_numbers_wrap() {
        assert!(newer(1, 0) && !newer(0, 1) && !newer(5, 5));
        assert!(newer(0x0000_0002, 0xFFFF_FFF0), "across the wrap");
        assert!(!newer(0xFFFF_FFF0, 0x0000_0002));
        assert!(reached(7, 7) && reached(8, 7) && !reached(6, 7) && reached(3, u32::MAX));
        let mut c = u32::MAX;
        assert_eq!(next_seq(&mut c), 1, "0 is never used");
    }

    #[test]
    fn mirror_applies_newer_entries_and_full_resyncs() {
        let mut m = Mirror::default();
        // A full: two owners.
        let t = m.apply(true, 10, 0, &[(0x800, HOST, 4), (0x801, C, 9)], 0);
        assert_eq!(t, vec![(0x800, NOBODY, HOST), (0x801, NOBODY, C)]);
        assert!(m.ready && m.applied == 10);
        // A stale delta entry is ignored, a newer one applies.
        let t = m.apply(false, 12, 10, &[(0x801, NOBODY, 8), (0x800, C, 11)], 0);
        assert_eq!(t, vec![(0x800, HOST, C)]);
        assert_eq!((m.stale, m.applied, m.owner(0x801)), (1, 12, C));
        // A gap (base 13, not 12): entries apply, the ack stops until the next full.
        let t = m.apply(false, 15, 13, &[(0x802, HOST, 14)], 0);
        assert_eq!(t, vec![(0x802, NOBODY, HOST)]);
        assert_eq!(m.applied, 12);
        m.apply(false, 16, 15, &[], 0);
        assert_eq!(m.applied, 12, "still behind the gap");
        // A full replaces: missing entries with an older seq become nobody, a newer one stays.
        m.rows.insert(0x900, MirrorRow { owner: C, seq: 30, at_ms: 0 });
        let t = m.apply(true, 20, 0, &[(0x800, C, 11)], 0);
        assert!(t.contains(&(0x801, C, NOBODY)) && t.contains(&(0x802, HOST, NOBODY)));
        assert_eq!((m.owner(0x800), m.owner(0x801), m.owner(0x900)), (C, NOBODY, C));
        assert_eq!(m.applied, 20);
        // Wrap-around: seq 2 after 0xFFFF_FFF0 is newer.
        let mut m = Mirror::default();
        m.apply(true, 0xFFFF_FFF0, 0, &[(0x800, HOST, 0xFFFF_FFEF)], 0);
        assert_eq!(m.apply(false, 2, 0xFFFF_FFF0, &[(0x800, C, 1)], 0), vec![(0x800, HOST, C)]);
        assert_eq!(m.applied, 2);
    }

    #[test]
    fn double_watch_needs_an_applied_change_and_two_reports() {
        let mut w = DoubleWatch::default();
        let host: HashSet<u16> = [0x800, 0x801].into_iter().collect();
        let seq = |n: u16| if n == 0x800 { 50 } else { 10 };
        // 0x800's change (50) is in flight (the client applied 40): never counted.
        assert!(w.report(C, 40, &[0x800], &host, seq).is_empty());
        assert!(w.report(C, 40, &[0x800], &host, seq).is_empty());
        // 0x801 applied: one report is not enough, two in a row are.
        assert!(w.report(C, 40, &[0x801], &host, seq).is_empty());
        assert_eq!(w.report(C, 40, &[0x801], &host, seq), vec![0x801]);
        // Not running on the host: nothing.
        assert!(w.report(C, 60, &[0x802], &host, seq).is_empty());
        assert!(w.report(C, 60, &[0x802], &host, seq).is_empty());
    }

    #[test]
    fn link_out_resends_a_full_only_when_an_old_send_is_unacked() {
        let mut l = LinkOut::default();
        // New link: a full.
        assert_eq!(l.plan(0, 1, false, false, false, false, 0), Some((Send::Full, false)));
        // Steady churn: a delta every 250 ms, each acked 300 ms later. The ack always trails
        // the newest send, but never one sent >= 1 s ago: no resend for a minute.
        let mut sends = vec![(0u64, 1u32)];
        for i in 1..240u64 {
            let now = i * 250;
            let acked = sends.iter().rev().find(|s| s.0 + 300 <= now).map_or(0, |s| s.1);
            let seq = i as u32 + 1;
            let (send, resend) = l.plan(acked, seq, true, false, false, false, now).unwrap();
            assert_eq!((send, resend), (Send::Delta { base: seq - 1 }, false), "at {now} ms");
            sends.push((now, seq));
        }
        // The client stops acking at seq 240 (a lost delta: its ack stops at the gap): a full
        // once the first unacked send is 1 s old, then at most once a second.
        let mut fulls = Vec::new();
        for i in 240..260u64 {
            let now = i * 250;
            if let Some((Send::Full, true)) = l.plan(240, i as u32 + 1, true, false, false, false, now) {
                fulls.push(now);
            }
        }
        assert_eq!(fulls, vec![61_000, 62_000, 63_000, 64_000]);
        // Nothing to send and nothing behind: nothing.
        let mut l = LinkOut::default();
        l.plan(0, 5, false, false, false, false, 0);
        assert_eq!(l.plan(5, 5, false, false, false, false, 5000), None);
        // A release wait past 1 s: a full even with the ack caught up.
        assert_eq!(l.plan(5, 6, true, false, true, false, 6000), Some((Send::Full, true)));
        // Every FULL_EVERY_MS and when the delta is too big: a full, not a resend.
        assert_eq!(l.plan(6, 7, true, true, false, false, 6100), Some((Send::Full, false)));
        assert_eq!(l.plan(7, 8, true, false, false, true, 6200), Some((Send::Full, false)));
    }

    #[test]
    fn kind_cache_keeps_a_boss_and_rechecks_an_ordinary_once_a_second() {
        let mut k = KindCache::default();
        let mut asked = 0;
        // First sight: the param row not readable yet (ordinary).
        assert_eq!(k.get(0x800, 1000, || { asked += 1; false }), (Kind::Ordinary, false));
        // Within the second: not asked again.
        assert_eq!(k.get(0x800, 1999, || { asked += 1; true }), (Kind::Ordinary, false));
        assert_eq!(asked, 1);
        // After it: asked, a boss now (flagged once).
        assert_eq!(k.get(0x800, 2000, || { asked += 1; true }), (Kind::Boss, true));
        assert_eq!(k.get(0x800, 9000, || { asked += 1; false }), (Kind::Boss, false), "a boss answer is kept");
        assert_eq!(asked, 2);
        assert_eq!(k.known(0x800), Some(Kind::Boss));
        assert_eq!(k.known(0x801), None);
        // A boss on the first sight is not "a boss now".
        assert_eq!(k.get(0x801, 0, || true), (Kind::Boss, false));
    }

    #[test]
    fn a_hand_over_never_has_two_owners_published() {
        // Host table with a client walking in and out; with acks late by a variable time, at
        // every tick at most one machine has the entry: the host by its own publication, the
        // client by what it applied (acked).
        let mut t = HostTable::default();
        let mut client_applied = 0u32;
        let mut sent: Vec<(u64, u32, u8)> = Vec::new(); // (deliver at, seq, owner)
        let mut client_owner = NOBODY;
        let mut last_at = 0u64;
        let (mut host_ticks, mut client_ticks) = (0u32, 0u32);
        for i in 0..400u64 {
            let now = i * 250;
            let (h, c) = if (i / 40) % 2 == 0 { (10.0, 30.0) } else { (40.0, 2.0) };
            let applied = client_applied;
            let out = t.tick(&[Seen { net: 0x800, entity: 1, kind: Kind::Ordinary, dead: false, players: two(h, c) }], &[], |_| applied, |_| true, now);
            for d in out.decided {
                // The lane delivers in order, each message 100-1000 ms late.
                last_at = last_at.max(now + 100 + (i % 7) * 150);
                sent.push((last_at, d.change.seq, d.change.to));
            }
            sent.retain(|&(at, s, owner)| {
                if at <= now {
                    client_owner = owner;
                    client_applied = s;
                    false
                } else {
                    true
                }
            });
            let host_runs = t.published(0x800) == HOST;
            assert!(!(host_runs && client_owner == C), "two owners at {now} ms");
            host_ticks += host_runs as u32;
            client_ticks += (client_owner == C) as u32;
        }
        assert!(host_ticks > 100 && client_ticks > 100, "it did move: host {host_ticks}, client {client_ticks}");
    }
}
