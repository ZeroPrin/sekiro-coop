# Plan: one host-authored owner table for enemy AI (Sekiro Together, branch anton/test7 @ 6e6358f)

## 0. Summary

Native type-22/23 cannot be the only decider (§3 has the evidence), so the plan uses the fallback the user named.

- **The host decides.** It keeps a table `netId -> owner slot | nobody`, built from distance with a dead band and a minimum hold, plus the boss pin and stealth-claim requests.
- **The table is sent on our reliable control lane.** Per-entry sequence numbers, sent at once on every change, plus a full resync every 5 s.
- **On every machine, `IsOwnedHere 0xA4A650` answers only from that table** for enemy netIds while engaged. It ignores the game's native table. This one change is what makes two brains impossible: today the forced bits can only *add* ownership and can never take native ownership away.
- **A hand-over away from a client goes through "nobody"** until that client acks (release, then acquire). So even a lost or skipped lane message cannot produce two brains. The cost is a gap of about one RTT with no owner, which is acceptable.
- **The native 22/23 keeps running**, only as the liveness signal for the client's first-23 gate. Its result is ignored.
- **Deleted:** ownfill's local decisions, `BOSS_FORCED`, claims through prio `0xFFFF` in type 22, host contests, `resolve_double_claims`, the pause hand-over, `force_due`, and the ini keys `ns_force_distribution` and `ns_throw_claims`.

---

## 1. What the test9 logs actually show (`E:\RE\logs-2026-11-14-test9`)

Host log `host\logs\2026-11-14_04-08-27.log`, client log `client-final\2026-11-14_04-04-21.log`. Log time = epoch; 04:08:27 = 1794604107.

- **Dead or spectating host keeps the boss natively while the client forces it.** This is a real double brain lasting tens of seconds.
  - Guardian Ape, 06:37: host `[1794613058] ownfill: BOSS (boss) netId 0x48ac ent 1700800 drop why=we are dead (spectating) ... game_owned=1 forced=false`.
  - Client `[1794613060] ... take ... forced=true`.
  - Host tally `[1794613060]` and `[1794613140]`: `claimed by both machines [0x48ac ent 1700800 (game here)]`.
  - The miniboss 2000279 at 05:16 is the same pattern (host `[1794608145]` death, `[1794608171/181]` "both", client take at `[1794608148]`).
  - v8.57 (`IMPLEMENTATION-NOTES.md:1739-1741`) already saw `game_owned=1` on a dead host. A forced bit cannot remove native ownership. **This is the decisive argument for an override detour.**
- **Mass churn where both machines force and both drop.** Example: host `[1794604473]` gives back 11 enemies "after 266 ms (another player owns it)", the client does the same at `[1794604477]`. This produced the 24/25-enemy lines (host log lines 3291 and 4055).
- **The client takes 32 times with no host position.** Lines read `"? from the nearest other player"`. With an empty `others` list, `beats()` (`nsession.rs:3064`) is true for everything; with a paused host it is excluded from `active` (`:5712`). Example: client `[1794609141]`, four takes; that is the 60 s overlap 05:32:29-05:33:29.
- **Miniboss 2000279 parked at its spawn.** The host takes it 17 times from 129-150 m (`wanted_boss` at ≤ 150 m, give-back at 170 m with 5 s dwell), and the client flips it too. Its position is constant at [-709.6, 48.8, 559.6].
- **05:07:22 "owned here false" plus "AI here" on both sides: not a double brain.** The enemy is a corpse (`hp 0/679`, host `[1794607642]`, client `[1794607643]`). `aiview` reports whichever manipulator was left active on a dead chr. The new detectors skip dead enemies.
- **`ns_force_distribution` is ON by default** (`settings.rs:471`, ini template `:977`, comment "v7.9 ... deflect timing was off without it"). The KB note says "default off"; that is wrong.

---

## 2. Current ownership map

All anchors are `crates/coop-dll/src/nsession.rs` unless another file is named. Each entry is marked **K** (keep), **R** (rewrite) or **D** (delete).

### Layer A: the game's own distribution (22/23)

| What | file:line | Fate |
|---|---|---|
| `feed_22`: the friend's 22 into the host record | 2597-2611 | K |
| `host_candidates`: strip, block, contest, refeed, first-22 `make_due` | 5871-5982 | R: plain feed plus the first-22 `make_due(T_DUE_23)` (5951-5958) only |
| `make_due` | 2400-2402 | K |
| `force_due`, `FORCE_DISTRIBUTION`, `FORCE_MIN_GAP_MS`, `FORCED_AT_MS`, `FORCES_SKIPPED` | 1615-1643 | D |
| Client `CLAIMS_GREW -> force_due(T_DUE_22)` | 5765-5769 | D |
| Engage-time `make_due` | 5514-5519 | K |
| Client late chr-loop patch gated on the first 23 | 5586-5657 (5623 `if served_23 && !self.late.is_empty()`) | R: also require the first full Assign |
| `owners()`, `owned_ids()` (call 0xA4A650, so they go through the detour) | 2365-2382, 2405-2413 | K (`owned_ids` becomes unused once `Owned` goes: D) |
| `settings.rs` `ns_force_distribution` | 471, ini template 975-977 | D |

### Layer B: local forced bits (ownfill)

| What | file:line | Fate |
|---|---|---|
| Bits, `set`, `clear_all`, detour, `game_owned`, `install` | 2982-3053 | R into module `own`: `MINE` bits plus `ACTIVE`; the detour answers only from the table |
| `key`, `beats`, `wanted`, `wanted_boss`, `keep_boss`, `host_far`, `host_listed`, `boss_rivals`, `keep`, `give_back_due`, `conflict_drop`, `RETAKE_COOLDOWN`, `drop_on_native`, `rival_down`, `arm_take_veto`, constants (`NEAR_M`, `GIVE_BACK_*`, `TIE_M`, `HOST_CONFLICT_DROP`, `BOSS_HOST_M`, `BOSS_HOST_BACK_M`) | 3055-3179 | D (replaced by `ownership::decide`) |
| `Fill`, `Fill::step` | 3181-3421 | D |
| `BOSS_FORCED`, `STRIPPED_BOSS`, `set_boss_forced`, `blocked_with_bosses` | 3425-3445 | D |
| `forced_here`, `game_owned_here` (readers: `statelog.rs:424-426`) | 2957-2964 | R: `forced_here` becomes `table_owner(n) -> Option<u8>`; `game_owned_here` stays (diagnostic: native answer, now ignored) |
| Engine `fill` field, init, teardown, step call, stats | 4923, 4974, 5054-5062, 5753-5762, 5744-5751 | R: field `own` |
| Pause hand-over: `OWNED_PAUSED_MARK`, `owned_is_paused`, `handover_targets`, `handed`, `pause_step` (Begin/End body) | 2428-2440, 2470-2487, 4944, 5781-5819 | D. `PauseTracker` (2442-2468) is K: it only sets our "paused" status flag |
| Spectating: bosses left out of the Owned list; `boss_net_ids` | 5725-5728, 2415-2426 | D |
| `dblock::boss_taken` (fresh-take stealth veto) | 4000-4015 | K; new caller (§4.6) |
| Throwprobe `ownfill::forced` | 8102, 8110, 8113 | R: `table owner {..}` |
| Tests `ownerless_enemies_go_to_exactly_one_machine`, `ownfill_nearest_player_takes_it_tie_lowest_slot`, `a_dead_spectating_host_leaves_the_boss_to_the_living_friend`, `a_boss_is_the_hosts_not_the_nearest_players`, `a_host_boss_is_not_dropped_on_a_native_owner_answer`, `a_boss_stays_with_the_living_player_while_a_rival_is_down`, `fresh_take_veto_arms_once_per_rival_down_episode`, `ownfill_hysteresis_no_flapping_at_the_threshold`, `world_pause_hands_enemies_on_and_back`, `ownfill_bits_set_and_clear`, `forced_boss_members_leave_the_friends_candidates` | 7280, 7309, 7357, 7381, 7399, 7411, 7428, 7444, 7507, 7537, 7189 | D (bits test R) |

### Layer C: claims (stealth, throw, deathblow)

| What | file:line | Fate |
|---|---|---|
| `enemies.rs` `stealth_claims`, `FIGHT_NEAR_M` = 20, `CLAIM_LINGER` = 3 s | `enemies.rs:1614-1708`, 1494, 1410 | K, unchanged (it still calls `set_stealth_claims`) |
| `CLAIMS`, `set_stealth_claims` | 1590-1606 | K (on change sets `CLAIMS_DIRTY`, which replaces `CLAIMS_GREW`) |
| `DROPPED_CLAIMS`, `CLAIM_PACKETS`, `CLAIM_ENTRIES`, `STRIPPED`, `CLAIM_PRIO` | 1592-1597 | D |
| `THROW_CLAIMS_ON` (`ns_throw_claims`, default 0), `THROW_CLAIM_MS`, `THROW_CLAIM_LOGS`, `THROW_CLAIMS`, the `note_throw` claim branch | 1619-1621, 3471, 3474, 3577-3588; `settings.rs:472, 978-979` | D |
| `claims_now`, `claims_raw` | 1650-1671 | R: read `CLAIMS` only |
| `claimed_in_22`, `host_wins_contest`, `CONTEST_HYST_M`, `encode_22`, `claim_22`, `strip_22` | 1673-1708, 1720-1768 | D (`list_22`/`parse_22` stay for `feed_22`) |
| `route_send` client claim injection | 1783-1792 | D |
| `slot_throw_age`, `our_throw_targets`, `Contender`, `contest_winner` | 3614-3634, 3648-3671 | D (`player_throw_age` and `throw_collides` stay: the collide path) |
| `contest`, Engine `winners`/`contested`/`contest_at`/`contest_logs`, `CONTEST_EVERY`, Link `claimed`/`blocked`/`last22`-refeed, engage clear | 5985-6045, 4936-4941, 4727, 5504-5513 | D (Link `last22` can go too) |
| `double_claim_drop`, `DOUBLE_CLAIM_AFTER`, `resolve_double_claims`, Engine `both_since`/`double_logged` | 2489-2493, 2434-2436, 5821-5864, 4945-4947, 5065-5069 | D |
| Stats lines "stealth claims (10 s)" and "stripped boss members" | 6110-6131 | R: the new summaries (§4.8) |
| Tests `stealth_claims_raise_our_prio_and_strip_the_friends`, `contested_claims_go_to_the_deathblow_then_the_nearer_player`, `contests_between_any_machines_are_settled_by_the_host`, `double_claim_host_wins` | 7161, 7237, 7257, 7475 | D. Keep `two_deathblows_at_once_do_not_cancel_each_other` (7202) |

### Coordination and diagnostics

| What | file:line | Fate |
|---|---|---|
| `Msg::Owned` + encode/decode/relay/store (`MSG_OWNED` = 5) | 248, 291-300, 387-399, 2815-2825, `Origin.owned` 2693, `owned_lists` 2930-2942, `OWNED_EVERY`/`OWNED_FRESH` 4646-4648, send block 5714-5737, Engine `owned_sent_*` 4915-4918 | R: id 5 becomes `Status` (§4.5); no relay |
| `OwnerTally`, `ownerless_report` ("claimed by both machines" at 2589) | 2495-2594 | R: host-only report from the table; "both" replaced by the DOUBLE detector |
| `PROTO` = 5 | 155 | R: 6 |

### Readers that stay correct untouched (they go through the detour)

- `owned_here` (2950); `livesync.rs:551, 558, 575, 699, 739`.
- The game's slot 37 `0xA14B50`, the senders `0xA1C100` (4/70/71) and the damage router `0xB6A040`/`0xB6F4C0` (all call 0xA4A650, `nsession.rs:2972-2975`).

---

## 3. Can native 22/23 be the only decider? No.

**Why the forced layer was added (from the docs):**
- **Ownerless enemies.** `IMPLEMENTATION-NOTES.md:1248`, live 7.4: "200 alive enemies owned by nobody", host `owners: Com 10 / NetAI 222 / owned-here 2`. The game builds its candidates (`0xA4E520`) only from enemies updated on that machine. An enemy that is NetAI here has interval −1 and never becomes a candidate on either side, so it stays NetAI without records everywhere.
- **Flapping bosses.** v8.12 and §53 (`:1591`).
- **Dead host keeps the boss natively.** v8.57 (`:1739-1741`).
- **Deathblow contests.** §34 (`:1317-1340`): a prio-0xFFFF claim always won, and claims took up to 5 s (22) + 3 s (23).
- **Ownership moved too slowly.** v7.9 `settings.rs:470`: "deflect timing was off without" forced distribution.

**What native alone would break, with evidence:**
1. **Ownerless near enemies come back.** §31 shows it; test9 still shows up to 20 within 40 m even with ownfill (host log line 3126).
2. **No boss pin is possible.** Sekiro's events never call 2004[28] Forced (`WEB-PRIOR-ART.md:141`), and its handler is not traced (`RE-AI-TARGET.md:275`).
3. **A dead or spectating owner keeps the boss** (`game_owned=1`, test9 06:37:38 host line, v8.57).
4. **Re-evaluation is slow:** 3 s / 5 s timers, no hold, and the prio-0 quirk (`PLAN-MULTIPLAYER.md:187`) can give one netId to both records.
5. **Claims need hacks:** prio `0xFFFF`, stripping, and contests.

**Why not "host writes final lists into the native records"** (KB suggestion 1)? It would still be a host-authored table, carried by the game's timers and records instead of ours, with more unknowns:
- The host's own record is rewritten by `0xA1CED0` every 5 s.
- It is not known whether the host's local apply (`0xA19B40`) resets first.
- 23 goes only to chrs with `vtbl[53] != 0` (`SPEC-NATIVE-SESSION.md:201`).
- The receive side applies only on the game's tick.

The `IsOwnedHere` detour is already installed and proven live (forced enemies ran here and their records reached the friend). Making it the *only* local answer is the smallest, most orthodox change: one decider, a whole-table answer on each machine, which is the native structure. The only difference is the transport.

---

## 4. Target design

### 4.1 New pure module `crates/coop-dll/src/ownership.rs`

It has no game reads; register it with `mod ownership;` in `lib.rs`.

Constants. No ini keys; all are `pub const`.

| Constant | Value | Meaning |
|---|---|---|
| `DECIDE_EVERY` | 250 ms | Host decision tick (also run at once on a Claims, Ack, Status-paused or presence change) |
| `TAKE_M` | 40.0 | An unowned ordinary enemy gets an owner when a present player is within this (today's `NEAR_M`) |
| `KEEP_M` | 60.0 | The current owner stays a candidate up to this |
| `SWITCH_M` | 8.0 | Dead band: switch A -> B only if `d_B + SWITCH_M < d_A`. First assignment: host unless a client has `d_c + SWITCH_M < d_h` |
| `MIN_HOLD` | 4 s | No distance switch within this after the last change (not applied for an absent owner or a claim) |
| `RELEASE_AFTER` | 5 s | Out of range continuously this long before going to nobody |
| `BOSS_TAKE_M` | 150.0 | Boss assigned when a present player is within this |
| `BOSS_KEEP_M` | 170.0 | Boss owner kept up to this |
| `BOSS_BACK_M` | 130.0 | A client-held boss goes back to a present host within this (hysteresis) |
| `FIGHT_NEAR_M` | 20.0 | A claim is refused while another present player is within this of the enemy (same value as `enemies.rs:1494`, now judged on the host) |
| `CLAIM_HOLD` | 3 s | After a claim ends, the claimant keeps it at least this (plus `MIN_HOLD` from the last claimed tick) |
| `FULL_EVERY` | 5 s | Full resync |
| `ACK_RESEND` | 1 s | Waiting for an ack longer than this: re-send a full (at most 1/s) |
| `ACK_WARN` | 5 s | Waiting longer: WARN line every 5 s; the entry stays nobody, never double |
| `STATUS_EVERY` | 2 s | Client Status heartbeat |
| `UNLOAD_GRACE` | 1 s | Missing from the scan this long before the entry goes to nobody |

Types:

```rust
pub const NOBODY: u8 = 0xFF; pub const HOST: u8 = 0;
pub struct Player { pub slot: u8, pub present: bool, pub dist: Option<f32> }
pub enum Kind { Ordinary, Boss }
pub struct Entry { pub owner: u8, pub since_ms: u64, pub out_since_ms: Option<u64>, pub claim_until_ms: u64 }
pub enum Why { Nearer, OwnerAbsent, Claim, BossHost, BossHostAway, InRange, OutOfRange, Unloaded }
pub fn decide(kind: Kind, cur: &Entry, players: &[Player], claimants: &[u8], now_ms: u64) -> (u8 /*target*/, Why)
```

Rules of `decide`, in order. "Present" means `present && dist.is_some()`.

1. **Absent owner.** If `cur.owner` is not NOBODY and that player is not present, the target is re-chosen at once by rules 2-4, ignoring `MIN_HOLD`. Why = `OwnerAbsent`.
2. **Claim.** Applies only to `Kind::Ordinary`, a present claimant, and no other present player within `FIGHT_NEAR_M`. If several claim, the current owner keeps it when it is a claimant, else the lowest slot wins. Target = claimant, Why = `Claim`. The caller sets `claim_until = now + CLAIM_HOLD` and `since = now` each tick the claim holds. While `now < claim_until` the claimant keeps it.
3. **Boss.**
   - Host present and `d_h ≤ (cur==HOST ? BOSS_KEEP_M : cur is a client ? BOSS_BACK_M : BOSS_TAKE_M)`: target host (`BossHost`).
   - Else the nearest present client with `d ≤ (cur==it ? BOSS_KEEP_M : BOSS_TAKE_M)`: target that client (`BossHostAway`).
   - Else nobody once out of range for `RELEASE_AFTER` (`OutOfRange`).
4. **Ordinary.**
   - Candidates = present players with `d ≤ (slot==cur ? KEEP_M : TAKE_M)`.
   - None: nobody after `RELEASE_AFTER` (`OutOfRange`).
   - `cur` among the candidates: switch to the best other `b` only if `d_b + SWITCH_M < d_cur` (`Nearer`).
   - `cur` is nobody: host if the host is a candidate and no client beats it by `SWITCH_M`, else the nearest client (`InRange`).
5. **Hold.** A non-absent, non-claim change with `now - since < MIN_HOLD` keeps `cur`.

`Publisher` is the host's release-then-acquire step for each entry:

```rust
pub struct Pub { pub published: u8, pub seq: u32, pub wait: Option<(u8 /*old client*/, u32 /*seq it must ack*/, u64 /*since*/)> }
pub fn publish_step(p: &mut Pub, target: u8, acked: impl Fn(u8) -> u32, engaged: impl Fn(u8) -> bool, next_seq: &mut u32, now: u64) -> Option<Change>
```

- **target == published:** nothing.
- **published == HOST (or NOBODY) -> target:** publish the target at once (new seq). The host drops its own bit the same tick, so this is already release-then-acquire.
- **published == client X -> target != X:** publish NOBODY at once (seq s1) and set `wait = (X, s1)`. When `acked(X) >= s1` (wrap-safe), or X is no longer engaged, publish the target (seq s2).

`Mirror` is the receiver side (client), also used by the host for its own bits:

- `apply_delta(entries)`: an entry applies only if `newer(entry.seq, stored.seq)`.
- `apply_full(seq, entries)`: same per entry; every stored netId missing from the full whose `stored.seq <= full.seq` becomes NOBODY.
- `newer(a, b) = a != b && a.wrapping_sub(b) < 0x8000_0000`.
- It returns the transitions `(netId, old, new)` so the caller can set bits and arm the boss veto.

`DoubleWatch` is the host's cross-machine detector:

```rust
fn report(&mut self, slot, applied_seq, running: &[u16], host_running: &HashSet<u16>, change_seq: impl Fn(u16)->u32) -> Vec<u16>
```

It counts a netId when it is running on both, `applied_seq >= change_seq(n)` (so the change is not in flight), and the same holds in two consecutive Status reports (≥ 2 s, far more than an RTT).

### 4.2 Detour, the single local answer (rewrite of `nsession.rs:2982-3053` as `mod own`)

```rust
static ACTIVE: AtomicBool; static MINE: [AtomicU64; 1024];
extern "C" fn detour(w: usize, n: u16) -> u8 {
    if ACTIVE.load(Ordering::Acquire) && crate::mod_alive() && enemy_net_id(n) { return mine(n) as u8; }
    game_owned(w, n)
}
```

- `enemy_net_id` is `nsession.rs:3477`. Player netIds and `0xFFFF` pass through to the game.
- `ACTIVE` is set by the host on its first decision of an engagement, and by the client on its first full Assign. `teardown` (5039) clears it.
- `MINE` holds the bits from the local `Mirror`; on the host the source is its own `Pub.published == HOST`.

### 4.3 Host state machine per enemy

States: **Unowned -> Host | Client(c) -> (Releasing(c) -> Host | Client(d) | Unowned)**.

Each host tick:
1. Scan `enemies()`, deduplicated by netId (`OwnerTally` note at 2495).
2. Classify `is_boss = bosses::encounter_of(e).is_some() || livesync::multi_life(lives0_of(c))`, the same as 3251-3254, cached per netId per engagement.
3. Build `players`:
   - Host present = `f.player.hp() > 0 && !bossrules::spectating() && self.world_running()`. A paused host is absent.
   - Client c present = link `Engaged` && `f.remote_of(c)` exists with `hp > 0` && `world::down_slots() & 1<<c == 0` && its last Status is not paused.
   - Distance: host from `f.player.position()`, client from the native remote's position.
4. Dead enemies (hp ≤ 0): no change, the owner keeps them.
5. Not in the scan for `UNLOAD_GRACE`: target NOBODY, `Unloaded`.
6. Run `decide`, then `publish_step`.
7. Collect the changes: set the host's own `MINE` bits, queue one `Assign` delta to every engaged client, and log (§4.8).
8. A full resync every `FULL_EVERY`, and immediately when any wait exceeds `ACK_RESEND`.

### 4.4 Client state machine

The client has no decisions:
- **Before the first full Assign:** `ACTIVE` false, and the late chr-loop patch is held. Change 5623 to `served_23 && self.own.ready() && !self.late.is_empty()`. All enemies keep their own AI, as today before the first 23.
- **If there is no full Assign within `FIRST_23_WITHIN` (5 s) of `host_live_at`:** the same path as the 5602-5613 No23 failure, with why `"no ownership table within 5 s"`.
- **On Assign:** `Mirror` applies it, `MINE` bits are updated, an `Ack{seq}` is sent (at most once per frame), and transitions are logged (bosses and claims always; others counted).
- **Stealth claims:** when `CLAIMS` changes, send `Claims{ids}` (the full current set). The host keeps the last set until the next message or a disengage.
- **Status** every `STATUS_EVERY`, and at once when `PauseTracker` flips.

### 4.5 Messages (`PROTO` 5 -> 6, `nsession.rs:155`; the Hello mismatch check at 5292 already refuses mixed builds)

All go on the control lane (key 0, class 0, `queue_ctrl`, 1434). None is relayed.

| Id | Message | Direction | Layout after the id byte |
|---|---|---|---|
| 5 | `Status` (replaces `Owned`) | client -> host, every 2 s and on a paused change | `u32 generation, u8 origin, u32 applied_seq, u8 flags (bit0 paused), u16 n, u16 running[n]`. `running` = alive enemies whose ComManipulator is active here (`netai::active_kind(c) == 1`), cap `MAX_OWNED` (2048) |
| 9 | `Assign` | host -> each engaged client | `u32 generation, u8 full, u32 seq, u16 n, {u16 netId, u8 owner (0xFF = nobody), u32 entry_seq}[n]`, cap 2048 |
| 10 | `Claims` | client -> host, on change | `u32 generation, u8 origin, u16 n, u16 ids[n]`, cap 64 |
| 11 | `Ack` | client -> host, after applying an Assign | `u32 generation, u32 applied_seq` |

- Messages are accepted only under `accepts(link, generation)`, like Lives.
- The host's `seq` is one `u32` counter per process (it never resets). The client mirror resets on teardown.
- **Why per-entry seq plus a full resync is needed:** the lane skips a gap older than `GAP_SKIP` = 3 s (`nsession.rs:539`), so delivery is not guaranteed complete.

### 4.6 Events

- **Connect / engage:**
  - Host: the first decision runs in the same `engaged_common` frame, then `ACTIVE`, then a full Assign.
  - Client: see §4.4.
- **A client disengages** (leave, link down, elsewhere, no remote): it stops being engaged, so its entries are re-decided at once (absent owner, no ack wait) and its `Claims`/`Status` are dropped. The client's teardown clears `ACTIVE` and restores its patches (the existing own-sync fallback, unchanged).
- **Host disconnects, area load, rest, block change:** `disengage_all`, then `teardown` clears tables, bits and `ACTIVE`.
- **Player death, ordinary:** hp ≤ 0 means absent, so that player's enemies move to the other player until the reload disengages.
- **Death in a boss fight (EV_DOWN / spectating):** absent, so the boss goes to the living client. When the host is present again (resurrected or reloaded), the boss goes back through Releasing (ack) -> Host.
- **Wipe:** both absent, so no target changes (absent-owner re-decide finds no candidate: hold as is), then the reload disengages.
- **Boss start / end:** no event is needed. The boss is pinned by the `Kind::Boss` rules while the host is present and within 150/170 m. "Out of the arena" is the host beyond 150 m with the client within it (hysteresis back at 130 m). A dead boss keeps its owner.
- **Enemy death:** no change. Unload: nobody after `UNLOAD_GRACE`. Respawn happens only via a rest, which disengages.
- **Enemies outside both players' range:** nobody, frozen NetAI on both. That is DS3-native and today's behaviour (~200 "owned by nobody").
- **Boss fresh-AI veto:** whenever the local `Mirror` transition makes a boss ours (on either role), call `dblock::boss_taken(n, e)` (4000). `arm_take_veto` is deleted: the min hold removes the take/drop cycles it guarded against.

### 4.7 Claims (point 4 of the user's model)

- **A request to the host, no optimistic local claim.** The approach window is 15 m behind (+5 m hold; plunges up to 25 m above; hang/cover 12 m). That is seconds of lead time against a grant latency of RTT/2 + ≤ 1 frame.
- **Host → client grants are immediate** (the host releases at once). **Client → host takes go through the ack** (≈ RTT).
- **Throw and deathblow claims are dropped.** `ns_throw_claims` was default 0 (`settings.rs:472`). Deathblows on an enemy owned elsewhere already work through the native type 18 plus the `dblock` lock (§39). They are not needed.

### 4.8 Diagnostics (point 6 of the user's model)

**Host decision line, one per change.** Bosses and claims are never capped; ordinary lines are capped at 60 per 10 s:
`own: 0x5822 e2000279 [B] host -> peer1 (host absent) d host 4.1 / peer1 5.3 seq 812`
For a remote release, two lines:
- `own: 0x48ac e1700800 [B] peer1 -> none (to host, waiting for peer1's ack) seq 813`
- `own: 0x48ac e1700800 [B] none -> host (peer1 acked after 46 ms) seq 814`

**Host 10 s summary:**
`own (10 s): seq S; host H / peer1 C / waiting K / unowned-within-40m U; changes N (nearer a, absent b, claim c, range d, boss e, unloaded f); fulls F, ack resends R; doubles D`

**Client 10 s summary:**
`own (10 s): applied seq S (fulls F, deltas D, stale ignored X); mine M; disobeyed Y; claims sent C granted G (median T ms)`

**Two-brain detectors:**
- `DISOBEY`, both sides, every 1 s. An alive enemy whose ComManipulator is active while the table says not ours, with the entry unchanged for > 1 s. It also logs `IsOwnedHere` and `u8 chr+0x2040` (`RE-FINDINGS-2.md:748`) to classify game-forced local enemies:
  `own: DISOBEY 0x.. e.. table owner peer1 (seq 790, 1.4 s ago) but our AI runs it (IsOwnedHere 0, +0x2040 0)`
- `DOUBLE`, host, from `DoubleWatch`:
  `own: DOUBLE 0x.. e.. runs on the host and peer1 (peer1 applied seq 814 >= change 790, 2 reports)`
- The "claimed by both machines" clause of `ownerless_report` is removed. The report becomes host-only, from the table: unowned within 40 m.

**Claim lines.**
- Client: `stealth: claim on enemy E (0x..) granted after N ms`, or `not granted within 2 s`.
- Host: `own: claim by peer1 on 0x.. refused (boss | host within 20 m)`, at most once per netId per 5 s.

---

## 5. Implementation order (small commits, each testable)

0. **Pre-flight, read-only IDA check (not a commit).** List the xrefs that read the ownership array `B+0x18` or `0xA1BB10` other than through `0xA4A650`. If any decides Com/NetAI or sending, note it; the DISOBEY detector also catches it live.
1. **`ownership: pure owner decision, publisher, mirror, double watch` (+ tests).** New `ownership.rs` and `mod ownership;` in `lib.rs`. Unit tests (table-driven):
   - The dead band: (h 10, c 5) -> host; (h 20, c 5) -> client; alternating 39/41 m and ±7 m around equal gives no flips.
   - `MIN_HOLD` blocks at 3.9 s and allows at 4 s.
   - An absent owner bypasses the hold.
   - Out of range: nobody only after 5 s beyond 60 m.
   - Boss: host at 140 m with the client at 1 m -> host. Host down -> client. Host back at 140 m -> stays client (back needs ≤ 130 m). Host at 120 m -> host after the hold. Host at 160 m with the client at 10 m -> client.
   - Claims: granted with the host at 30 m; refused with the host at 15 m; refused on a boss; both claim -> current owner; after the claim ends, the hold applies.
   - Dead enemy: no change. Unloaded: nobody.
   - Publisher: host -> client in one step; client -> host gives NOBODY, then host only after ack ≥ s1; a disengaged client gives host at once; the resend flag after 1 s.
   - Mirror: stale delta ignored; full replaces; absent-from-full -> nobody; wrap-around compare.
   - DoubleWatch: an in-flight change is not counted; one report is not counted; two acked reports are counted.
2. **`nsession: Assign / Claims / Ack / Status messages, PROTO 6`.** Encode/decode with caps; extend `messages_roundtrip_and_reject_truncation_and_trailing_bytes` (6550). `on_control` stores them in `Book` (host: per-slot claims, ack, last two Status; client: an Assign queue). `Status` replaces `Owned` (id 5). Nothing consumes them yet.
3. **`nsession: the host's owner table drives IsOwnedHere on both machines`** (the core).
   - Rewrite `ownfill` as `own`: `ACTIVE`/`MINE` detour, host step, client apply, Ack, the gate at 5623, the "no ownership table" failure.
   - Delete `Fill` and its call sites, `BOSS_FORCED` and its helpers, the pause hand-over (`handed`, `handover_targets`, `OWNED_PAUSED_MARK`), and the spectating-boss filter.
   - `host_candidates` becomes a plain feed.
   - New caller of `dblock::boss_taken`; statelog and throwprobe show `table owner`.
   - Live-testable on its own: the claims still ride 22, which is harmless because native results are ignored now.
4. **`claims: a request to the host`.** Client `Claims` message on `CLAIMS` change; the host feeds its own `CLAIMS` plus the clients' sets into `decide`. Delete everything in layer C marked D, `force_due`/`FORCE_DISTRIBUTION`/`THROW_CLAIMS_ON`, and the ini keys (`settings.rs:471-472, 975-979`; unknown keys in old ini files are ignored, `settings.rs:12`).
5. **`own: diagnostics`.** Decision lines, summaries, DISOBEY, DOUBLE; rework `ownerless_report`; delete `OwnerTally.both`, `owned_lists`, `owned_ids` if unused, and the old Owned send block.
6. **Docs and version.**
   - `FORK.md`: test11 banner, checklist below, removed keys.
   - `IMPLEMENTATION-NOTES.md`: new section "host-authored enemy ownership".
   - `docs/kb/enemy-authority-handover.md`: implemented, and fix the "default off" line.
   - Bump `MOD_VERSION` to `-test11`.

---

## 6. Risks and what can only be checked live

- **A game path reads native ownership directly**, not through `0xA4A650`. Watch for `DISOBEY` lines with `IsOwnedHere 0`.
- **Game-forced local enemies** (`0x9F33F0` / `chr+0x2040`) run on both machines natively. `DISOBEY` with `+0x2040 1` classifies them; out of scope to fix.
- **Removing `ns_force_distribution` affects deflect timing.** v7.9 said "deflect timing was off without it". The cause was slow moves of ownership to the nearer player, and Assign is now immediate. Check parry timing on client-near enemies.
- **The ack wait gives a short ownerless gap** (~RTT) on client -> host moves. Check the boss returning to a resurrected host for snaps.
- **The host's view of the client lags** (native remote interpolation). Only decision quality suffers, never consistency.
- **Enemies loaded on the client but not on the host** (block border) have no owner and freeze. The engage netId check reduces this.
- **The host's own world pause** makes the host absent, so its enemies near the client move. Confirm that our frame runs while the world is paused (the existing pause code assumes it does).

**Co-op test checklist (test11, expected lines):**

1. **Engage.**
   - Client: `nsession: first 23 ...` then `own (10 s): applied seq ... (fulls 1 ...)`.
   - Host: `own (10 s): ... host H / peer1 C`.
   - No `DISOBEY` or `DOUBLE`.
2. **Group fight, both near.** Few `own: ... -> ... (nearer)` lines, none less than 4 s apart for the same netId. Summary `doubles 0`, client `disobeyed 0`.
3. **Client walks 40 m ahead into a new group.** Host `own: ... none -> peer1 (in range)`. The enemies move and attack on both screens.
4. **Client stealth deathblow.**
   - Client: `stealth: claiming enemy ...`, then `stealth: claim on enemy ... granted after <300 ms`, then the deathblow works.
   - Host stands next to the enemy: `own: claim by peer1 on ... refused (host within 20 m)`.
5. **Boss or miniboss (Guardian Ape, 2000279-type).**
   - `[B] ... (boss: host)` once; no further lines while both are alive.
   - Host dies to phase B: `[B] host -> peer1 (host absent)` plus the client's `deathblow: boss ... taken while ...` veto line.
   - Host back: two lines, `peer1 -> none (waiting for peer1's ack)` then `none -> host (peer1 acked after N ms)`.
   - No `DOUBLE` at any point.
6. **Host opens a menu that pauses its world.** `[..] host -> peer1 (host absent)` for the host's enemies near the client; after resume, only `nearer` moves after the hold.
7. **Disconnect and reconnect, load, rest.** Teardown, then a fresh full Assign on the next engage.
8. **2-hour soak.** `doubles` total 0 and `disobeyed` total 0 (or only `+0x2040 1` cases), against 54 "claimed by both" in test9.

---

## 7. Size, and what I would not do

**Size.** About −1,100 lines (layers B and C, the Owned path, ~350 lines of old tests) and +650 (`ownership.rs` ~300 with ~200 lines of tests, nsession glue ~180, messages ~70). Net ≈ −450.

**Hand-over smoothness.** The cheap measure that falls out is rarity: the 8 m dead band, the 4 s hold, the boss pin and acked releases make switches uncommon. Separate later steps:
- (a) switch at a commit boundary (KB #7);
- (b) carry the AI target into the new owner (KB #8);
- (c) prime the first records after a take (KB #9).

**Explicitly not done:**
- No change to AI target selection; `aitarget` stays observe-only.
- No calls to EMEVD 2004[28]/[34].
- No writing of final lists into the native records (`0xA1C6C0`).
- The native 22/23 feed is not removed (it stays as the liveness gate).
- No new ini keys; no N>2 tuning (the code stays slot-generic).
- `pickups` is not touched; `enemies.rs` claim heuristics are not changed; `dblock` changes only by the new caller.
- No migration or repair layers.

### Critical files for implementation
- E:\REPOS\sekiro-coop-wt\anton-test7\crates\coop-dll\src\nsession.rs (detour 2982-3053; layers B/C as mapped; messages 155-440 and 2735-2860; `engaged_common` 5674-5775; `host_candidates` 5871-5982; client gate 5586-5657; `teardown` 5039-5072)
- E:\REPOS\sekiro-coop-wt\anton-test7\crates\coop-dll\src\ownership.rs (new, pure, unit-tested)
- E:\REPOS\sekiro-coop-wt\anton-test7\crates\coop-dll\src\enemies.rs (`stealth_claims` 1614-1708, unchanged API, the input to Claims)
- E:\REPOS\sekiro-coop-wt\anton-test7\crates\coop-dll\src\settings.rs (delete 471-472 and template 975-979)
- E:\REPOS\sekiro-coop-wt\anton-test7\crates\coop-dll\src\statelog.rs (424-426: table owner instead of forced)
