//! Lift scripts (test6, review M3): the flags of the game's shared lift script
//! `common_func` event 20005621, which the co-op save merge must not copy (busy / delay) or
//! copy only as a consistent pair (position / state), and doors.rs never takes for a door's.
//!
//! Event 20005621 (E:\RE\sekiro-data\event\js\common_func.emevd.dcx.js:3597-3678), params:
//! `eventFlagId` = position (requested side), `eventFlagId2` = state (side the lift is at),
//! `eventFlagId3` = busy, `eventFlagId4` = delay, `eventFlagId5` = optional enable flag, the
//! ObjAct flags of the call / start levers, the lift entity. Every instance in the unpacked
//! EMEVD (all `InitializeCommonEvent(20005621, ...)` in `event\js\*.js`, 2026-11-12):
//! - m20_00_00_00 L37 and its variant m20_00_12_60 L37 (Senpou temple):
//!   `20005621, 12000655, 12000656, 2001650, 2001653, 12000651, 2001651, 12000652, 12000653,
//!   12000654, 12000657, 12000658, 0, 2001652, 2001654`.
//!
//! No other `common_func` event has the "busy flag set, then `WaitFor(!busy)`" pattern
//! (20004115 waits on a flag it never sets: a passive hit switch; 20005620 is one-shot).
//!
//! Why the busy flag is transient [VAR, read in the EMEVD]: the busy flag is switched ON only
//! inside `if (NumberOfClientsOfType(ClientType.Invader) != 0)` (L3619-3622 / L3656-3659), i.e.
//! never in vanilla single player, and switched OFF by the same ride at L1 / L4 (L3635,
//! L3672) right after the lift animation (`ForceAnimationPlayback(..., true, ...)`, waits for
//! the end). A ride holds it ON for its length (12-17 s live, 2026-11-12); nothing else in the
//! game writes it. The delay flag (L3629-3632 / L3666-3669) is ON for the 2 s before the
//! animation of a lever-called ride. When the script starts (every load) with position ==
//! state and busy ON, it goes to L9 `WaitFor(!EventFlag(busy))` (L3676) and nothing will ever
//! switch busy OFF: the lift and its lever prompts are dead for good.

use std::collections::BTreeSet;

/// One instance of `common_func` 20005621.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Lift {
    pub name: &'static str,
    /// `eventFlagId`: the side asked for.
    pub position: u32,
    /// `eventFlagId2`: the side the lift is at.
    pub state: u32,
    /// `eventFlagId3`: a ride is running (network rides only).
    pub busy: u32,
    /// `eventFlagId4`: the 2 s before a called ride.
    pub delay: u32,
    /// The levers' ObjAct flags (`objactEventFlag` 1-4).
    pub objact_flags: [u32; 4],
    /// The ObjActParam rows of its levers (205010 via the model, 205011).
    pub params: [i32; 2],
}

/// Every lift-script instance found in the unpacked EMEVD (module doc).
pub const LIFTS: &[Lift] = &[Lift {
    name: "Senpou temple lift (m20_00)",
    position: 12000655,
    state: 12000656,
    busy: 12000657,
    delay: 12000658,
    objact_flags: [12000651, 12000652, 12000653, 12000654],
    params: [205010, 205011],
}];

/// A lift's busy or delay flag: never session progress, never merged into an own world.
pub fn transient(id: u32) -> bool {
    LIFTS.iter().any(|l| l.busy == id || l.delay == id)
}

/// A lift's position or state flag (merged only as a consistent pair).
pub fn pair_flag(id: u32) -> bool {
    LIFTS.iter().any(|l| l.position == id || l.state == id)
}

/// Pure (review M3): makes a set of progress flags (ORed into our own world, whose value is
/// `own_on`) safe for the lift scripts. Busy / delay flags are dropped; position and state are
/// kept only when the merged world ends with position == state (else both go, and our own
/// world's pair stays). Returns the flags dropped.
pub fn sanitize_progress(progress: &mut BTreeSet<u32>, own_on: impl Fn(u32) -> bool) -> Vec<u32> {
    let mut dropped = Vec::new();
    for l in LIFTS {
        for id in [l.busy, l.delay] {
            if progress.remove(&id) {
                dropped.push(id);
            }
        }
        let merged = |id: u32| progress.contains(&id) || own_on(id);
        if merged(l.position) != merged(l.state) {
            for id in [l.position, l.state] {
                if progress.remove(&id) {
                    dropped.push(id);
                }
            }
        }
    }
    dropped
}

#[cfg(test)]
mod tests {
    use super::*;

    const L: Lift = LIFTS[0];

    #[test]
    fn the_senpou_lift_is_the_emevd_instance() {
        assert_eq!((L.position, L.state, L.busy, L.delay), (12000655, 12000656, 12000657, 12000658));
        assert!(transient(12000657) && transient(12000658));
        assert!(!transient(12000655) && !transient(12000656) && !transient(12000654));
        assert!(pair_flag(12000655) && pair_flag(12000656) && !pair_flag(12000657));
    }

    /// 12000655-658 all in progress (as test5 recorded them after one ride), our own world
    /// without the record: busy and delay go, the pair (both ON) stays.
    #[test]
    fn busy_and_delay_never_merged() {
        let mut p: BTreeSet<u32> = [11000000, 12000000, 12000655, 12000656, 12000657, 12000658, 12005827].into();
        let dropped = sanitize_progress(&mut p, |_| false);
        assert_eq!(dropped, vec![12000657, 12000658]);
        assert_eq!(p, [11000000, 12000000, 12000655, 12000656, 12005827].into());
    }

    #[test]
    fn a_half_pair_goes_unless_our_world_completes_it() {
        // Position alone, our own world has the lift at the other side: inconsistent -> dropped.
        let mut p: BTreeSet<u32> = [12000655].into();
        assert_eq!(sanitize_progress(&mut p, |_| false), vec![12000655]);
        assert!(p.is_empty());
        // Position alone, our own world already has the state ON: consistent -> kept.
        let mut p: BTreeSet<u32> = [12000655].into();
        assert!(sanitize_progress(&mut p, |id| id == 12000656).is_empty());
        assert_eq!(p, [12000655].into());
        // State alone with our own position ON: kept.
        let mut p: BTreeSet<u32> = [12000656].into();
        assert!(sanitize_progress(&mut p, |id| id == 12000655).is_empty());
        // State alone, own position OFF: dropped.
        let mut p: BTreeSet<u32> = [12000656, 1].into();
        assert_eq!(sanitize_progress(&mut p, |_| false), vec![12000656]);
        assert_eq!(p, [1].into());
    }

    #[test]
    fn unrelated_flags_untouched() {
        let mut p: BTreeSet<u32> = [12000654, 12000659, 11100500].into();
        assert!(sanitize_progress(&mut p, |_| true).is_empty());
        assert_eq!(p.len(), 3);
    }
}
