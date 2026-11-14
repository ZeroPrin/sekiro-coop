//! Passive owner view (test14, docs/PLAN-NATIVE-OWNERSHIP.md §4): pure detectors over who
//! runs which enemy under the game's own 22 / 23 distribution. Nothing here decides or
//! writes ownership; nsession.rs (module `ownlog`) reads the game and logs `own:` lines.
//!
//! - `TwoReports`: DOUBLE (one enemy's own AI on two machines) and OWNERLESS-NEAR (an
//!   alive enemy near a living player on nobody's AI) count only when they hold in two
//!   consecutive evaluations at least `MIN_GAP_MS` apart (a 23 in flight takes far less).
//! - `OwnerWatch` + `Flaps`: changes of the native answer per netId (BOSS / FLAP lines,
//!   `dblock::boss_taken` on a 0 -> 1).
//! - `Episode`: a condition held for a while (SLOW, OWNER-DOWN, HOST-SILENT).
//! - `Budget`: line caps per 10 s, everything counted.

use std::collections::{BTreeMap, HashMap, VecDeque};

/// A client sends its OwnView this often.
pub const REPORT_EVERY_MS: u64 = 2000;
/// Two evaluations count as consecutive reports only this far apart (2 s nominal; the
/// arrival jitter of the reliable lane is well under 0.5 s).
pub const MIN_GAP_MS: u64 = 1500;
/// At most this many netIds in one OwnView.
pub const MAX_RUNNING: usize = 2048;
/// OWNERLESS-NEAR / OWNER-DOWN / HOST-SILENT: enemies within this many metres of a living player.
pub const NEAR_M: f32 = 40.0;
/// FLAP: this many changes of the native answer within `FLAP_WINDOW_MS`.
pub const FLAP_CHANGES: usize = 3;
pub const FLAP_WINDOW_MS: u64 = 10_000;
/// SLOW: the enemy is within `SLOW_NEAR_M` of a client that is at least `SLOW_GAP_M` nearer
/// to it than the host, for more than `SLOW_AFTER_MS`, and the host still runs it.
pub const SLOW_NEAR_M: f32 = 10.0;
pub const SLOW_GAP_M: f32 = 8.0;
pub const SLOW_AFTER_MS: u64 = 4000;
/// OWNER-DOWN: held this long before the first line, then a line at most this often.
pub const DOWN_AFTER_MS: u64 = 5000;
/// HOST-SILENT: the host's 4 / 5 / 70 / 71 stopped this long.
pub const SILENT_AFTER_MS: u64 = 2000;
/// Repeat lines of a held OWNER-DOWN / HOST-SILENT at most this often.
pub const REPEAT_MS: u64 = 10_000;

/// Bit of the host (slot 0) in a running mask; a client slot `s` is bit `s`.
pub const HOST_BIT: u8 = 1;

/// One evaluation: who runs which alive enemy (netId -> bit per slot whose own AI runs it),
/// and the alive enemies within `NEAR_M` of a living player (netId -> metres to the nearest).
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Snapshot {
    pub at_ms: u64,
    pub running: BTreeMap<u16, u8>,
    pub near: BTreeMap<u16, f32>,
}

/// What two consecutive snapshots agree on.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Verdict {
    /// netId and the machines (bit per slot) running it in both snapshots (two or more bits).
    pub doubles: Vec<(u16, u8)>,
    /// netId and metres, nearest first: near a living player in both, running nowhere in both.
    pub ownerless: Vec<(u16, f32)>,
}

/// The two-report rule (`step`).
#[derive(Default)]
pub struct TwoReports {
    prev: Option<Snapshot>,
}

impl TwoReports {
    /// `cur` against the previous accepted snapshot. A snapshot sooner than `MIN_GAP_MS`
    /// after the previous one is ignored (None) and does not replace it.
    pub fn step(&mut self, cur: Snapshot) -> Option<Verdict> {
        let Some(prev) = self.prev.take() else {
            self.prev = Some(cur);
            return Some(Verdict::default());
        };
        if cur.at_ms < prev.at_ms.saturating_add(MIN_GAP_MS) {
            self.prev = Some(prev);
            return None;
        }
        let v = verdict(&prev, &cur);
        self.prev = Some(cur);
        Some(v)
    }

    pub fn reset(&mut self) {
        self.prev = None;
    }
}

/// Pure: what `prev` and `cur` agree on.
pub fn verdict(prev: &Snapshot, cur: &Snapshot) -> Verdict {
    let mut doubles = Vec::new();
    for (&n, &m) in &cur.running {
        let both = m & prev.running.get(&n).copied().unwrap_or(0);
        if both.count_ones() >= 2 {
            doubles.push((n, both));
        }
    }
    let nowhere = |s: &Snapshot, n: u16| s.running.get(&n).is_none_or(|&m| m == 0);
    let mut ownerless: Vec<(u16, f32)> = cur
        .near
        .iter()
        .filter(|&(&n, _)| prev.near.contains_key(&n) && nowhere(cur, n) && nowhere(prev, n))
        .map(|(&n, &d)| (n, d))
        .collect();
    ownerless.sort_by(|a, b| a.1.total_cmp(&b.1));
    Verdict { doubles, ownerless }
}

/// "host and peer1", "peer1 and peer2", "host, peer1 and peer2".
pub fn machines(mask: u8) -> String {
    let names: Vec<String> = (0..8u8).filter(|s| mask >> s & 1 != 0).map(who).collect();
    match names.as_slice() {
        [] => "nobody".into(),
        [one] => one.clone(),
        [rest @ .., last] => format!("{} and {last}", rest.join(", ")),
    }
}

pub fn who(slot: u8) -> String {
    if slot == 0 { "host".into() } else { format!("peer{slot}") }
}

/// A change of the native "owned here" answer of one netId.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Change {
    /// First answer this engagement.
    First,
    Same,
    /// 0 -> 1: it moved here.
    Taken,
    /// 1 -> 0: it moved to another machine.
    Lost,
}

/// The last native answer per netId (this engagement).
#[derive(Default)]
pub struct OwnerWatch {
    last: HashMap<u16, bool>,
}

impl OwnerWatch {
    pub fn observe(&mut self, n: u16, here: bool) -> Change {
        match self.last.insert(n, here) {
            None => Change::First,
            Some(was) if was == here => Change::Same,
            Some(_) if here => Change::Taken,
            Some(_) => Change::Lost,
        }
    }

    pub fn owned_here(&self, n: u16) -> Option<bool> {
        self.last.get(&n).copied()
    }
}

/// Changes per netId within `FLAP_WINDOW_MS`.
#[derive(Default)]
pub struct Flaps {
    changes: HashMap<u16, VecDeque<u64>>,
    logged: HashMap<u16, u64>,
}

impl Flaps {
    /// A change of `n` at `ms`: Some(changes in the window) when a FLAP line is due (at least
    /// `FLAP_CHANGES`, and none logged for `n` within the window).
    pub fn record(&mut self, n: u16, ms: u64) -> Option<usize> {
        let q = self.changes.entry(n).or_default();
        q.push_back(ms);
        while q.front().is_some_and(|&t| ms.saturating_sub(t) >= FLAP_WINDOW_MS) {
            q.pop_front();
        }
        let count = q.len();
        if count < FLAP_CHANGES || self.logged.get(&n).is_some_and(|&t| ms.saturating_sub(t) < FLAP_WINDOW_MS) {
            return None;
        }
        self.logged.insert(n, ms);
        Some(count)
    }

    /// Drop the netIds with no change for a window (bounded memory).
    pub fn prune(&mut self, ms: u64) {
        self.changes.retain(|_, q| q.back().is_some_and(|&t| ms.saturating_sub(t) < FLAP_WINDOW_MS));
        self.logged.retain(|_, &mut t| ms.saturating_sub(t) < FLAP_WINDOW_MS);
    }
}

/// A condition held over time: `step` says when a line is due (after `after_ms`, then every
/// `every_ms`; `u64::MAX` = once per episode).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Episode {
    since: Option<u64>,
    logged: Option<u64>,
}

impl Episode {
    /// Some(held ms) when a line is due now.
    pub fn step(&mut self, active: bool, ms: u64, after_ms: u64, every_ms: u64) -> Option<u64> {
        if !active {
            *self = Self::default();
            return None;
        }
        let since = *self.since.get_or_insert(ms);
        let held = ms.saturating_sub(since);
        if held <= after_ms {
            return None;
        }
        if self.logged.is_some_and(|t| ms.saturating_sub(t) < every_ms) {
            return None;
        }
        self.logged = Some(ms);
        Some(held)
    }

    /// Held now (ms), 0 if not.
    #[cfg(test)]
    pub fn held(&self, ms: u64) -> u64 {
        self.since.map_or(0, |s| ms.saturating_sub(s))
    }
}

/// At most `max` lines per `window_ms`; the rest counted.
#[derive(Default)]
pub struct Budget {
    at: Option<u64>,
    lines: u32,
    pub suppressed: u32,
}

impl Budget {
    pub fn allow(&mut self, ms: u64, max: u32, window_ms: u64) -> bool {
        if self.at.is_none_or(|t| ms.saturating_sub(t) >= window_ms) {
            self.at = Some(ms);
            self.lines = 0;
        }
        if self.lines < max {
            self.lines += 1;
            true
        } else {
            self.suppressed += 1;
            false
        }
    }
}

/// Priority histogram bucket (`chr+0x1FC8` / a 22 entry): 0, < 256, < 1024, < 1792, >= 1792.
pub fn prio_bucket(p: u16) -> usize {
    match p {
        0 => 0,
        1..256 => 1,
        256..1024 => 2,
        1024..1792 => 3,
        _ => 4,
    }
}

/// "[a / b / c / d / e]".
pub fn hist(h: &[u32; 5]) -> String {
    format!("[{} / {} / {} / {} / {}]", h[0], h[1], h[2], h[3], h[4])
}

/// The entries of a type-22 payload `{i32 n; u32 netId | prio << 16 ...}` (nsession.rs
/// `parse_22` checked the shape): netId -> prio.
pub fn entries_22(payload: &[u8]) -> HashMap<u16, u16> {
    payload
        .get(4..)
        .unwrap_or_default()
        .chunks_exact(4)
        .map(|b| (u16::from_le_bytes([b[0], b[1]]), u16::from_le_bytes([b[2], b[3]])))
        .collect()
}

/// Per-enemy reads (position, which manipulator runs) every observation only within this
/// many metres of a player (covers every distance rule above: 40 m / 10 m) and for bosses /
/// multi-life enemies; farther ones are re-read every `FAR_REFRESH_MS` (= one OwnView round,
/// so the two-report rule still sees each of them fresh once per report).
pub const READ_NEAR_M: f32 = 60.0;
pub const FAR_REFRESH_MS: u64 = 2000;

/// Pure: read this enemy's position / running flag now? `last` = (ms of its last read, its
/// distance to the nearest player then; None = position unknown). A new enemy, a boss, a
/// near one or an unknown position: every time; a far one once per `FAR_REFRESH_MS`.
pub fn read_due(boss: bool, last: Option<(u64, Option<f32>)>, ms: u64) -> bool {
    match last {
        None => true,
        Some(_) if boss => true,
        Some((_, d)) if d.is_none_or(|d| d <= READ_NEAR_M) => true,
        Some((at, _)) => ms.saturating_sub(at) >= FAR_REFRESH_MS,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn far_enemies_are_read_once_per_report_round() {
        assert!(read_due(false, None, 0), "first sight");
        assert!(read_due(true, Some((0, Some(500.0))), 10), "a boss always");
        assert!(read_due(false, Some((0, Some(59.0))), 10), "near");
        assert!(read_due(false, Some((0, None)), 10), "position unknown");
        assert!(!read_due(false, Some((0, Some(80.0))), 250));
        assert!(!read_due(false, Some((0, Some(80.0))), FAR_REFRESH_MS - 1));
        assert!(read_due(false, Some((0, Some(80.0))), FAR_REFRESH_MS));
        // Every distance rule of the detectors lies inside the always-read radius.
        assert!(NEAR_M <= READ_NEAR_M && SLOW_NEAR_M <= READ_NEAR_M);
    }

    fn snap(at_ms: u64, running: &[(u16, u8)], near: &[(u16, f32)]) -> Snapshot {
        Snapshot { at_ms, running: running.iter().copied().collect(), near: near.iter().copied().collect() }
    }

    #[test]
    fn a_double_needs_two_reports_at_least_the_gap_apart() {
        let mut t = TwoReports::default();
        assert_eq!(t.step(snap(0, &[(0x801, 0b11), (0x802, 0b01)], &[])), Some(Verdict::default()));
        // Too soon: ignored, the first one stays the reference.
        assert_eq!(t.step(snap(1000, &[(0x801, 0b11)], &[])), None);
        let v = t.step(snap(2000, &[(0x801, 0b11), (0x802, 0b11)], &[])).unwrap();
        assert_eq!(v.doubles, vec![(0x801, 0b11)], "0x802 doubled in one report only");
        let v = t.step(snap(4000, &[(0x801, 0b10), (0x802, 0b11)], &[])).unwrap();
        assert_eq!(v.doubles, vec![(0x802, 0b11)], "a hand-over in flight is not a double");
    }

    #[test]
    fn a_double_between_two_clients_counts_too() {
        let v = verdict(&snap(0, &[(7, 0b110)], &[]), &snap(2000, &[(7, 0b111)], &[]));
        assert_eq!(v.doubles, vec![(7, 0b110)]);
        assert_eq!(machines(0b110), "peer1 and peer2");
        assert_eq!(machines(0b11), "host and peer1");
        assert_eq!(machines(0b111), "host, peer1 and peer2");
        assert_eq!(machines(0b10), "peer1");
    }

    #[test]
    fn ownerless_near_needs_two_reports_with_nobody_running_it() {
        let a = snap(0, &[(3, 0b01)], &[(1, 12.0), (2, 30.0), (3, 5.0)]);
        let b = snap(2000, &[(2, 0b10)], &[(1, 11.0), (2, 31.0), (3, 6.0), (4, 2.0)]);
        let v = verdict(&a, &b);
        // 1: nobody in both. 2: the client took it. 3: the host ran it in the first.
        // 4: near in one report only.
        assert_eq!(v.ownerless, vec![(1, 11.0)]);
        // A zero mask is "nobody" too; nearest first.
        let c = snap(4000, &[(2, 0)], &[(1, 20.0), (2, 3.0)]);
        let d = snap(6000, &[], &[(1, 20.0), (2, 3.0)]);
        assert_eq!(verdict(&c, &d).ownerless, vec![(2, 3.0), (1, 20.0)]);
    }

    #[test]
    fn owner_watch_reports_changes_after_the_first_answer() {
        let mut w = OwnerWatch::default();
        assert_eq!(w.observe(5, false), Change::First);
        assert_eq!(w.observe(5, false), Change::Same);
        assert_eq!(w.observe(5, true), Change::Taken);
        assert_eq!(w.observe(5, false), Change::Lost);
        assert_eq!(w.observe(6, true), Change::First, "owned on first sight is not a take");
        assert_eq!((w.owned_here(5), w.owned_here(6), w.owned_here(7)), (Some(false), Some(true), None));
    }

    #[test]
    fn flap_needs_three_changes_in_ten_seconds_and_logs_once_per_window() {
        let mut f = Flaps::default();
        assert_eq!(f.record(9, 0), None);
        assert_eq!(f.record(9, 4000), None);
        assert_eq!(f.record(9, 9000), Some(3));
        assert_eq!(f.record(9, 9500), None, "already logged in this window");
        // Old changes leave the window.
        let mut g = Flaps::default();
        g.record(1, 0);
        g.record(1, 6000);
        assert_eq!(g.record(1, 12_000), None, "the change at 0 is out of the window");
        assert_eq!(g.record(1, 13_000), Some(3));
        g.prune(40_000);
        assert_eq!(g.record(1, 40_000), None);
    }

    #[test]
    fn an_episode_logs_after_its_delay_then_at_its_interval() {
        let mut e = Episode::default();
        assert_eq!(e.step(true, 0, SLOW_AFTER_MS, u64::MAX), None);
        assert_eq!(e.step(true, 4000, SLOW_AFTER_MS, u64::MAX), None);
        assert_eq!(e.step(true, 4100, SLOW_AFTER_MS, u64::MAX), Some(4100));
        assert_eq!(e.step(true, 60_000, SLOW_AFTER_MS, u64::MAX), None, "once per episode");
        assert_eq!(e.step(false, 61_000, SLOW_AFTER_MS, u64::MAX), None);
        assert_eq!(e.held(61_000), 0);
        let mut d = Episode::default();
        d.step(true, 0, DOWN_AFTER_MS, REPEAT_MS);
        assert_eq!(d.step(true, 5001, DOWN_AFTER_MS, REPEAT_MS), Some(5001));
        assert_eq!(d.step(true, 10_000, DOWN_AFTER_MS, REPEAT_MS), None);
        assert_eq!(d.step(true, 15_001, DOWN_AFTER_MS, REPEAT_MS), Some(15_001));
        assert_eq!(d.held(16_000), 16_000);
    }

    #[test]
    fn budget_caps_lines_per_window_and_counts_the_rest() {
        let mut b = Budget::default();
        assert!((0..10).all(|_| b.allow(0, 10, 10_000)));
        assert!(!b.allow(500, 10, 10_000));
        assert_eq!(b.suppressed, 1);
        assert!(b.allow(10_000, 10, 10_000));
    }

    #[test]
    fn prio_buckets_and_22_entries() {
        assert_eq!([0, 1, 255, 256, 1023, 1024, 1791, 1792, 0x7FFF].map(prio_bucket), [0, 1, 1, 2, 2, 3, 3, 4, 4]);
        assert_eq!(hist(&[1, 2, 3, 4, 5]), "[1 / 2 / 3 / 4 / 5]");
        let mut p = 2i32.to_le_bytes().to_vec();
        p.extend_from_slice(&(0x0801u32 | 1791 << 16).to_le_bytes());
        p.extend_from_slice(&(0x5822u32 | 300 << 16).to_le_bytes());
        let e = entries_22(&p);
        assert_eq!((e[&0x0801], e[&0x5822], e.len()), (1791, 300, 2));
        assert!(entries_22(&[]).is_empty());
    }
}
