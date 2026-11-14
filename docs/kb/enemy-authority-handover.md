# Enemy AI authority and hand-over: FromSoftware native vs. Sekiro Together

Research note, 2026-11-14. Read-only research: nothing was built or run. Question: in FromSoftware's
native multiplayer, which machine simulates an enemy's or boss's AI, and how does authority move
between players? Does it follow the enemy's current target? What makes the hand-over look smooth?
And how does our mod compare?

Evidence tags:
- **[CODE]**: confirmed by reverse engineering or by game data. This is our own IDA work on
  `sekiro.exe` 1.06, which still contains DS3's enemy-sync code, plus event scripts and struct dumps.
- **[COMM]**: community reports, wiki pages and forum posts.
- **[SPEC]**: speculation or inference.

---

## 1. Short answer

1. **Authority does not follow the target. [CODE, high for Sekiro/DS3 lineage]** The game's own
   ownership request carries a *network update priority* computed from the enemy's **distance to
   that machine's main player**:
   - type 22 = `netId | prio<<16`, with `prio = 0x9F0230(chr)`, written into `chr+0x1FC8`;
   - sources: `docs/SPEC-NATIVE-SESSION.md:222`, `docs/RE-ENEMY-TARGETING.md:103`.

   The AI's target slot (`TARGET_ENE_0`, the `SprjTargetingSystem` records) plays no part in who
   owns the enemy. The target is only *replicated*: behaviour record `+0x148` holds the AI target
   mode and the target netId (`docs/RE-FINDINGS-2.md:781`), so the NetAI copy turns toward the
   owner's target.
2. **The host is the only arbiter. [CODE]**
   - Every member sends its candidate list (type 22) to the host every 5 s (`T+0x3A0`).
   - Every 3 s (`T+0x398`) the host settles conflicts with `0xA1D550`/`0xA1C590`: the higher prio
     wins, and on a tie the member with fewer candidates wins.
   - The host then sends each member its owner list (type 23).
   - A receiver resets its whole table to −1 and marks only the listed netIds as owned
     (`docs/RE-SEKIRO-ONLINE.md:143-149`, `docs/PLAN-MULTIPLAYER.md:182-187`,
     `docs/IMPLEMENTATION-NOTES.md:1039`).
   - The effect is **proximity-based ownership, re-evaluated about every 3 s, decided by one
     machine.** Phantoms never decide; they only propose.
3. **Bosses are pinned to the host by script in DS3 and ER. [CODE]** Boss start events call
   `SetNetworkUpdateRate(boss, true, AlwaysUpdate)` and `SetNetworkUpdateAuthority(boss, Forced=4095)`.
   That second call sits behind a "host world only" gate:
   - DS3, Dancer-area boss 3000899: `GotoIfPlayerIsNotInOwnWorldExcludesArena(LABEL4)` jumps over
     it for phantoms. Source: `E:\RE\kb-sources\ai\SoulsRandomizers\dist\Base\events.txt:1059,1064,1066`.
   - DS3, Dragonslayer Armour 3010800-802: `events.txt:1489,1493-1496`. Here the `Forced` lines are
     skipped when there are 0 invaders.
   - ER: `EndIfPlayerIsInWorldType(End, OtherWorld)` comes right before `SetNetworkUpdateAuthority(..., Forced)`
     (`...\diste\Base\events.txt:1576-1577`). ER uses it 11 times; it also appears unconditionally
     in a presetup at `:9180`.
   - Sekiro's vanilla events do not use 2004[28] (`docs/WEB-PRIOR-ART.md:141`; it is absent from the
     randomizer's Sekiro events).
   - So in FromSoft's design a boss is **not** handed over during the fight at all. Ordinary
     enemies are distance-distributed.
4. **ER exposes the same model in its struct names. [CODE, names from the community RE]**
   - `ChrIns.network_authority: u32`.
   - `NetChrSyncFlags` bit 5 `distance_based_network_update_authority` ("When set, the character
     will use distance-based network update authority").
   - `net_position_synchronized`: "set by NetAIManipulator after receiving a position update".
   - `EnemyIns { com_manipulator, net_ai_manipulator, ride_manipulator }`.
   - Source: `E:\RE\kb-sources\er\fromsoftware-rs\crates\eldenring\src\cs\chr_ins.rs:236,292-293,498,1012-1014`.
   - This is the same two-manipulator design as Sekiro's `chr+0x2000` (Com) and `chr+0x2008` (NetAI)
     (`docs/RE-FINDINGS-2.md:739-749`).
5. **DS1 (DSR) is looser. [CODE-docs]** `WorldChrSyncLocation` is "continually sent from every
   player, guest or host"; "the person responsible for handling an enemy's position" changes
   "depending on currently unknown variables". AI state flows both ways: packet 20
   `WorldChrSyncRemoteToHostEnemyAI` and packet 21 `WorldChrSyncHostToRemoteEnemyAI`. There is also
   a join snapshot, `EnemyInit` (39). Source: https://soulsmodding.com/doku.php?id=topic%3Adsr_gamedata_communication
6. **Community view. [COMM, weak]** Nothing public states "the AI runs on the player it targets".
   - Posts say "a boss' attention goes to the player who hit it last", or the one who entered the
     fog first. That is *targeting*, not authority (`docs/kb/ai-targeting.md:235-238`).
   - Players widely report that enemies and bosses "snap" or teleport and that parries miss in
     co-op. That fits remote authority with interpolated copies. Examples:
     https://steamcommunity.com/app/1245620/discussions/0/3802778195625723680 and
     https://www.gearupbooster.com/en/blog/fix-elden-ring-nightreign-lag.html
   - No authoritative public write-up was found for DS3, ER or Bloodborne. ds3os is a server
     emulator and has no in-game P2P sync (`docs/WEB-OPEN-SOURCE-COOP-2.md:67`).
   - BBSC (Bloodborne) treats "Enemy / boss AI: Host" as a hypothesis (`docs/WEB-OPEN-SOURCE-COOP-2.md:66`).
   - The local Seamless Co-op and ds3os clones under `E:\RE\kb-sources` are placeholder files
     (1-byte), so they hold no evidence.

### What makes the native hand-over look acceptable [CODE unless tagged]

| Mechanism | Detail | Source |
|---|---|---|
| Continuous replication, not a hand-over transfer | The owner writes per-netId records every frame and sends them. **4** = movement, 28 B: position + Euler rotation; 8 enemies per packet, round-robin cursor `T+0x3A8`. **70** = behaviour, 344 B: hkb sync events, up to 32 hkb variables, blend mask and AI target; refilled only when the sync state changes. **71** = action, 48 B: the AI's "pad" action inputs. **5** = HP, host → client. | `RE-FINDINGS-2.md:752-783`, `RE-SEKIRO-ONLINE.md:137-144` |
| Interpolation on the NetAI copy | Velocity = (target − current) / (time since the last record × 1.1), with angular velocity as well. The move goes through ChrCtrl/physics, so collision and the locomotion blend are kept. **Snap** when the implied speed exceeds `manip+0xF0`, when there was no record for more than 2 s, or when the yaw error is over 90°. | `RE-FINDINGS-2.md:787-790` (ER: `ChrManipulator.network_warp_distance`, `WEB-OPEN-SOURCE-COOP-2.md:53`) |
| Switch at the current pose | Slot 37 `0xA14B50` activates the new manipulator through its slot 6 with the current pos/rot and rebinds ChrCtrl. Nothing is teleported at the moment of the switch. | `RE-FINDINGS-2.md:748` |
| Slow, coarse re-evaluation | 22 is sent every 5 s and 23 every 3 s, so ownership changes at most every few seconds and only when the distance ranking changes. No explicit hold/cooldown exists beyond these timers. | `IMPLEMENTATION-NOTES.md:702` |
| Bosses never move | Forced authority on the host, plus AlwaysUpdate (no update-rate thinning). | events.txt (above) |
| Damage is router-based, not owner-based | The attacker's machine evaluates its hit; a hit on an enemy owned elsewhere is applied locally **and** broadcast as 21; enemy hits on a player are computed on the victim's machine. HP and posture never need to be "handed over". | `IMPLEMENTATION-NOTES.md:687` |
| AI state on the new owner | The ComManipulator object persists, but its AI was not ticked while it was NetAI. RE-AI-TARGET says take-over runs InitAi `0x7E5000` (empty target records, state 0, gauge 0). Our live logs agree: an enemy just moved to a new machine allows stealth deathblows ("fresh AI", `RE-CORPSE-TARGET.md:77-84`). The engine sends **no AI goal stack or target memory** on hand-over. Only the replicated `+0x148` target exists on the receiving side. | `RE-AI-TARGET.md:316-320` [CODE mechanism, untested live] |
| Known native quirk | In `0xA1C590`, if `b.prio == 0`, **both records keep the netId**, so 23 can make two machines owners. Whether `0x9F0230` ever yields 0 for listed enemies is unchecked. | `PLAN-MULTIPLAYER.md:187` [CODE quirk, SPEC on frequency] |

[SPEC] FromSoft's hand-over is not smooth by design. It is *rare*: proximity changes slowly, the
timers are seconds, and bosses are pinned. Wherever it happens it relies on the NetAI interpolation
plus a fresh AI that re-acquires its target within about 0.2 s through perception. That is cheap
for trash mobs, and that is why bosses are excluded.

---

## 2. What our mod does today (native session, stage ≥ 2)

Our mod has **three layers**, all of which can say "owned here".

**Layer A. The game's own distribution (22/23).**
- Driven by the game's timers; we feed the friend's 22 into the host's tables (`0xA1D7E0`, `0xA1C6C0`).
- On engage, `T+0x39C`/`T+0x3A4` are forced to 0.
- `ns_force_distribution` (default **off**, `nsession.rs:1618`) would force an extra 22/23 at most
  once per second (`:1625-1643`).

**Layer B. Local forced bits ("ownfill").**
- `IsOwnedHere 0xA4A650` is detoured (`nsession.rs:3026-3031`) to return 1 for netIds in `FORCED`.
  **Each machine decides this locally** in `ownfill::Fill::step` every 250 ms (`:3228-3421`).
- Ordinary enemies, take:
  - an enemy that nobody owns (not natively here, not in any peer's Owned list), within
    `NEAR_M` = 40 m (`:2990`);
  - and we are the nearest player (the host gets 5 m in its favour, `TIE_M`) (`wanted`, `:3075`).
- Ordinary enemies, give back after `GIVE_BACK_AFTER` = 5 s (`:2996`) when one of these holds:
  - past `keep` (48 m / tie, `:3120`);
  - the game's native answer says it is ours now (`:3301`);
  - another peer lists it (`conflict_drop`, `:3134`): the host drops a non-boss at once, a client
    drops at once if a lower slot owns it.
- After a conflict drop there is no retake for `RETAKE_COOLDOWN` = 10 s (`:3142`, `:3350`).
- Bosses and multi-life minibosses (`is_boss`, `:3251`):
  - The host takes and keeps them within `BOSS_HOST_M` 150/170 m (`:3083`, `:3097-3107`) and never
    drops one in a conflict (`:3309`).
  - A client takes one only if the host is beyond 150 m, is down, or is spectating (`boss_rivals`,
    `:3113`). The fresh-take veto is in `dblock::boss_taken` (`:3388`).
  - The host publishes `BOSS_FORCED` so it can strip those netIds from client 22s (`:3428-3445`).

**Layer C. Claims (stealth, throw, deathblow).**
- Who claims:
  - `enemies.rs::stealth_claims` (`enemies.rs:1614-1708`): behind/above or hang/cover within the
    stealth window. Never a boss/miniboss (`:1650`). Never an enemy another player stands within
    `FIGHT_NEAR_M` = 20 m of (`:1494`, `:1657`). Lingers `CLAIM_LINGER`.
  - Our throw, for `THROW_CLAIM_MS` = 4 s (`nsession.rs:3471`), only if `ns_throw_claims` is on (`:1621`).
- Client side: claims ride in our 22 with prio `0xFFFF` (`claim_22`, `:1731`).
- Host side: it strips the friend's entries for its own claims (`strip_22`, `:1756`). When both
  claim, `host_wins_contest` (`:1688`) decides: the earlier throw first, otherwise the nearer
  player with 1.5 m hysteresis (`:1708`).
- A client drops its claim when a lower slot has also owned the enemy for `DOUBLE_CLAIM_AFTER`
  2.5 s (`double_claim_drop`, `:2491`; `resolve_double_claims`, `:5821-5864`; this is the
  "claimed by both machines … our claim is dropped" log line at `:5850`).

**Coordination.**
- Every 2 s each machine sends an `Owned{generation, netIds}` list (`OWNED_EVERY`, `:4646`). It is
  sent earlier, at most every 250 ms, when the list changes.
- The "claimed by both machines" tally (`OwnerTally`, `:2498-2519`; line `:2589`) compares our
  live answer with the peer's **last** Owned list. That list can be up to 2 s old.
- On a paused world, `handover_targets` (`:2473`) drops the enemies a peer would take from our list
  and adds the marker `0xFFFE`.

**State on hand-over.** No hand-over message carries anything. The new owner's ComManipulator
starts with whatever AI state it has (a fresh one per RE-AI-TARGET §6.3). The old owner's copy
turns NetAI and follows the new owner's records once they arrive. Everything else is just the
continuous 4/70/71 stream plus 5/21: HP and posture are never part of a hand-over.

### Why "claimed by both machines" still happens (from the code above)

1. **Two arbiters.** Layer B (each machine's forced bits) and layer A (the host's 23, applied on
   the other machine) are independent. A forced bit on machine X and a native 23 entry on machine
   Y overlap until X sees Y's Owned list (≤ 2 s plus the RTT) and then waits out `conflict_drop`.
   For client bosses, `HOST_CONFLICT_DROP` is 3 s. [CODE]
2. **Snapshot lag in detection.** The tally and `conflict_drop` work on Owned lists sent every 2 s.
   A clean hand-over in which each side moves within the same 2 s window is briefly counted as
   "both". This inflates the counter and can trigger needless drops (flapping: "taken 127, given
   back 112"; ordinary enemies changed owner every ~1 s at 11-16 m,
   `IMPLEMENTATION-NOTES.md:1587`). [CODE + SPEC]
3. **The client's game table only changes on 23.**
   - The host can strip a boss from the client's 22, but the client keeps its last 23 until the
     next distribution (3 s).
   - The host's `0xFFFF` claims and contests have the same lag unless `ns_force_distribution` is on
     (it is off by default).
   - [CODE]
4. **The prio-0 quirk** in `0xA1C590` would natively give one netId to both records. This is
   unverified in live use. [CODE quirk / SPEC]
5. **Local, asymmetric distance inputs.** Each side measures its distances to the *remote replica*
   of the other player, whose position lags. Both can therefore believe they are nearest for a
   moment. The `TIE_M` and keep hysteresis reduce this but cannot remove it, because there is no
   single decider. [SPEC, high]

### Why the hand-over hitches

- On the new owner's screen, the enemy jumps from replicated motion to a fresh local AI:
  - it loses its target and battle state, so it turns, plays an idle, or re-acquires the target;
  - a running attack is cut off, because the action inputs (71) that drove it stop;
  - this is the "fresh AI" in `RE-CORPSE-TARGET.md:83` and `nsession.rs:3153`.
- On the old owner's screen, the copy switches from its own simulation to the remote's records:
  - the first 4/70 record can be late: movement is round-robin, 8 per packet, and behaviour is sent
    only on change;
  - a big position or yaw error snaps (> 90° yaw, too fast, or > 2 s without a record).
- Flapping multiplies both effects.

---

## 3. Comparison table

| Aspect | FromSoft native (DS3/ER lineage, as in sekiro.exe) | Our mod now |
|---|---|---|
| Who decides | The host only (22 → host, 23 → members) | Host 23 **plus** each machine's local forced bits (ownfill) **plus** claim contests on the host |
| Ordinary enemy rule | Higher distance-based network prio (`0x9F0230`) wins; a tie goes to fewer candidates | Nearest within 40 m, host +5 m, keep to 48 m / 5 s; the native distance rule still runs underneath |
| Boss rule | `SetNetworkUpdateAuthority(Forced)` + AlwaysUpdate, run only in the host world (DS3/ER), so it is never handed over | Host-sticky within 150/170 m. A client may take it if the host is beyond 150 m, down or spectating; there are also fresh-take vetoes |
| Follows target? | No (the target is only replicated in behaviour `+0x148`) | No (distance and claims; a throw/stealth claim is "follows the attacker" for 3-4 s) |
| Trigger cadence | 22 every 5 s, 23 every 3 s | ownfill every 250 ms; Owned lists every 2 s (early on change ≤ 250 ms); 23 still every 3 s unless forced |
| State moved on hand-over | Nothing explicit. Continuous 4/70/71 (+5 HP host → client); the new owner's AI starts fresh | Same. No target or AI state is sent |
| Double-ownership protection | Structural: one arbiter, a receiver resets its whole table on each 23 (except the prio-0 quirk) | Reactive: conflict detection on 2 s-old Owned lists, then drop (host at once / client by slot / 3 s), 10 s retake cooldown, `DOUBLE_CLAIM_AFTER` 2.5 s, `BOSS_FORCED` stripping |
| Smoothing | NetAI velocity interpolation with snap limits; switch at the current pose; rare switches | Same engine interpolation; no hold at animation boundaries; switches can be frequent |

---

## 4. Suggestions, in priority order

### (1) Eliminate double ownership

1. **Make the host the single arbiter, with no local forced bits on clients.** Effort M-L, risk M.
   - The host already writes every player's candidate record directly (`0xA1D7E0`/`0xA1C6C0`,
     `strip_22`, `contest`).
   - Move ownfill's decision (nearest within 40 m, boss rules, claims) into the host. The host
     writes the *final* list into each player's record, so `0xA1C590` has nothing left to settle,
     then forces 23 (`T+0x39C = 0`) when the assignment changes, at most every 250-500 ms.
   - Clients keep the `IsOwnedHere` detour only as a fallback for "no 23 yet". Their wishes
     (stealth, throw) travel as `0xFFFF` claims, as today.
   - This copies the native structure (one decider, and each 23 resets the receiver's whole
     table), so overlap is limited to one 23 in flight.
   - It also fixes §31's ownerless problem natively. An enemy that is NetAI everywhere is not in
     any 22, but the host can still put its netId into the chosen player's record.
   - Risk: the 22/23 path becomes latency-critical, so it should sit in the lane's top priority
     class (it already does).
2. **Hand over as release-then-acquire.** Effort M, risk L-M.
   - If layer B stays, a taker sets its forced bit only after the current owner has published a
     release, or after the host's 23 no longer lists the enemy for the old owner.
   - Use a per-netId epoch, so a stale Owned list cannot re-grant.
   - A short "nobody owns" gap (≤ 1 RTT) is harmless: both NetAI copies extrapolate for up to 2 s
     before snapping. Two AIs at once is the bug.
   - The repo's unwired `sekiro-coop-authority` handoff/ack idea is the same pattern
     (`docs/WEB-OPEN-SOURCE-COOP-2.md:105`).
3. **Event-based Owned deltas with sequence numbers instead of 2 s snapshots.** Effort S, risk L.
   - Send `{seq, netId, owned|released}` on every change and keep the 2 s full list as the
     heartbeat.
   - Detection drops to about the RTT, and the "claimed by both" tally stops counting legitimate
     in-flight hand-overs. Count only overlaps longer than RTT + 250 ms.
4. **Neutralise the prio-0 quirk.** Effort S, risk L.
   - First log a histogram of prio values in the host's received and own 22s.
   - If 0 occurs, the host rewrites prio 0 to 1 before `0xA1C6C0` (both our feed and its own list),
     so `0xA1C590` always picks one owner.
5. **Pin bosses for the whole encounter, the way FromSoft does.** Effort S, risk L.
   - The boss encounter goes to the host from activation until the end of the encounter.
   - A client may take it only when the host is down or spectating. Do not allow the "host beyond
     150 m" case mid-fight.
   - Hand it back only at an animation boundary (see 7).
   - [SPEC] Option: try calling the EMEVD 2004[28] Forced and 2004[34] AlwaysUpdate handlers on
     the host, if they can be located (handler not traced, `RE-AI-TARGET.md:275`). This is lower
     priority than doing it in our own rules.
6. **Minimum lease.** Effort S, risk L. Once assigned, an ordinary enemy stays put for at least
   4 s, except for deathblow or throw claims. This matches the native ~3 s cadence and stops the
   observed ~1 s flips.

### (2) Smooth the visual hitch

7. **Switch only at a commit boundary.** Effort S-M, risk L.
   - Delay the actual flip of a non-claim hand-over until the enemy is between actions: the last
     71 action record shows no attack request, or the TS replan has happened (the engine itself
     only changes targets at replans, the +50 rule in `RE-AI-TARGET.md:302-304`).
   - Use a timeout of about 1.5 s.
   - This removes the "attack cut off / fresh idle" hitch on the new owner's screen.
8. **Carry the target into the new AI.** Effort M, risk M; the mechanism is from RE but untested live.
   - On take, read the last replicated AI target from our NetAI copy's behaviour record `+0x148`
     (mode 1 = chr netId) and map it to our chr: our player, or the remote replica.
   - Then call `0x60D0A0(chr, targetChr)`. If no Com AI exists yet, it is stored in `chr[4]+0x94`
     and InitAi applies it as a battle-state record (`RE-AI-TARGET.md:266-272, 316-320`).
   - This turns a "fresh, unaware" AI into one already fighting the same player. It removes the
     stealth-deathblow window after moves, and the turn/idle hitch.
   - Never use it on a player the enemy has no record for (stealth rule, `RE-AI-TARGET.md:327`).
9. **Prime the first records after a take.** Effort M, risk M.
   - When we take a netId, put it first in the next type-4 batch (the cursor at `T+0x3A8`) and mark
     its behaviour dirty (`bm+0xB20/0xB21`), so the old owner's NetAI copy gets position plus full
     hkb state within one send.
   - Today a behaviour record is sent only on change, and movement is round-robin with 8 per packet.
   - This avoids the >2 s or >90° snap path in `0x7EB2A0` on the old owner's screen.
10. **Optional blend on the old owner's side.** Effort M, risk M.
    - If the first record lands more than about 1.5 m away, ease the NetAI target over 150-250 ms
      instead of letting `0x7EB2A0` snap. This is the same easing pattern as the remote pin in
      `IMPLEMENTATION-NOTES.md:703`.
    - Do this only if 7-9 are not enough.

**Recommended order:** 3 → 6 → 4 → 1 (or 2 if 1 is too big) → 5 for double ownership, then
7 → 8 → 9 for the hitch.

## 5. Open questions and live checks

- The prio values `0x9F0230` actually produces, and whether 0 occurs (log them).
- Whether take-over really calls InitAi `0x7E5000` (fresh records) or resumes stale Com state. Log
  the TS state (`nsession.rs:7984` offsets) in the frame before and after a take.
- That `+0x148` in the received behaviour record carries a usable target netId for enemies
  targeting the remote player (the netId rewrite for player ids applies).
- The EMEVD 2004[28]/[34] handler addresses in Sekiro (not traced).

## Sources

- Repo: `docs/RE-FINDINGS-2.md` §Enemy AI authority (734-793); `docs/SPEC-NATIVE-SESSION.md` §4 (218-242);
  `docs/RE-SEKIRO-ONLINE.md` §4 (133-160); `docs/RE-ENEMY-TARGETING.md` (92-124);
  `docs/RE-AI-TARGET.md` §6 (260-378); `docs/PLAN-MULTIPLAYER.md` (33-42, 180-187);
  `docs/IMPLEMENTATION-NOTES.md` §18/18b (660-706), §34 (1037-1051), §31 (1206-1256), §46 (1522-1529),
  §53 (1589-1591), v8.30b (1672); `docs/RE-CORPSE-TARGET.md` (70-89); `docs/WEB-OPEN-SOURCE-COOP-2.md` (50-67, 105);
  `docs/WEB-PRIOR-ART.md` (132-146); `docs/kb/ai-targeting.md` §4-5.
- Code: `crates/coop-dll/src/nsession.rs` (lines cited above), `crates/coop-dll/src/enemies.rs:1486-1708`.
- Local third-party sources:
  - `E:\RE\kb-sources\er\fromsoftware-rs\crates\eldenring\src\cs\chr_ins.rs:236,292,498,1012-1014`;
  - `E:\RE\kb-sources\ai\SoulsRandomizers\dist\Base\events.txt:1059-1066,1489-1496` (DS3);
  - `E:\RE\kb-sources\ai\SoulsRandomizers\diste\Base\events.txt:1576-1577,9180` (ER).
- Web:
  - https://soulsmodding.com/doku.php?id=topic%3Adsr_gamedata_communication (DSR packet list);
  - https://steamcommunity.com/app/1245620/discussions/0/3802778195625723680 (ER co-op enemy desync, anecdotal);
  - https://www.gearupbooster.com/en/blog/fix-elden-ring-nightreign-lag.html (boss "snaps", anecdotal);
  - https://www.gamesradar.com/after-12-years-dark-souls-speedruns-have-reached-the-point-of-frame-perfect-attacks-that-make-bosses-forget-you-exist (DS1 target-list rebuild, single-player).
  - Searches for a FromSoft "AI runs on the targeted player" statement found nothing.
