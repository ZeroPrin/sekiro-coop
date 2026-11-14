# Plan: native enemy ownership following LukeYui's model (branch `anton/test14-native-ownership`, from `anton/test11-ownership` c3b359b)

## 0. Summary

- **What the series does.** It takes our ownership logic out. Ownership goes back to the game's own 22/23 distribution, which is what LukeYui's Sekiro Online and DS3 Seamless do. The baseline also adopts the three cheap things his relay does that we don't: the lives byte on relayed hits, type‑70 checks before the game parses a packet, and victim-decided hits on the local player.
- **What it does not do.** The test11 owner table is deleted, not kept behind a switch. Nothing in the baseline writes ownership priorities, sync-table periods or claims.
- **Everything Yui does not do comes later.** Boss pin, shorter timers, dead-owner handling, claims and target seeding are each a separate optional step in §5. Each one has the symptom from our logs that would justify it and the log line that would show the need.
- **Deathblows are the first-class case (§3).** Natively, a deathblow on an enemy owned by the other machine needs no change of ownership. The test12 symptom "enemies stand back up" is a lives-sync ordering gap, and the lives byte (commit 3) is the fix. It is not a reason for claims.

Evidence tags: **[C]** decompiled or code I read in this pass (`E:\RE\work_net\f\*.c`, the worktree). **[KB]** from the gap map or the two studies. **[U]** unknown. Where Yui's behaviour is unknown, I say so.

---

## 1. Current state: what the baseline removes or changes (anchors at c3b359b)

All paths are under `crates/coop-dll/src/`.

| Piece | Anchor | Fate in step 1 |
|---|---|---|
| Pure owner-table logic (decide, publisher, mirror, KindCache, DoubleWatch, LinkOut, client-only rows) | `ownership.rs` (all 1349 lines), `lib.rs:59` `mod ownership;` | Delete |
| `IsOwnedHere` detour and table store (`ACTIVE`, `OWNER[0x10000]`, `detour`, `game_owned`, `install`) | `nsession.rs:2811-2897` | Delete. No hook on `0xA4A650`; the game answers |
| `own::State` (host_step, client_step, double_check, disobey_check, summary, send, warn_waits, near_report, track_claims) | `nsession.rs:2919-3642` | Delete |
| `table_owner`, `game_owned_here` | `nsession.rs:2780-2790`; readers `statelog.rs:424-426`, throwprobe `nsession.rs:7598` | Delete. statelog prints `owned_here` only; throwprobe drops `table=` |
| Hook install call `own::install()` | `nsession.rs:2092-2094` | Delete |
| Messages Status (5), Assign (9), Claims (10), Ack (11); `MAX_CLAIMS`, `STATUS_PAUSED`; Book `assigns` / `origins.status/claims/ack`; `AssignIn`, `StatusIn` | `nsession.rs:164-180`, `249-283`, encode 284+, decode 416+, `on_control` 2551+, Book 2488-2524 | Delete. Id 5 becomes `OwnView` (§4.1); ids 9/10/11 retired |
| Claims: `CLAIMS`, `CLAIMS_DIRTY`, `set_stealth_claims`, `take_claims`, `claims_wanted` | `nsession.rs:1680-1721`, use at 5789 | Delete |
| `enemies.rs` native-session claim path (`stealth_claims`, `CLAIM_LINGER`, `fought_by_another`, `FIGHT_NEAR_M`) | `enemies.rs:382`, `1410`, `1486-1494`, `1613-1708` | Delete the native path. Mode‑4 `throw_window` and `stealth_window_m` stay for the own-sync fallback; check what is still shared before deleting |
| Engine field `own`, its calls in `engaged_common`, `link_gone` in `disengage`, `reset` in `teardown`, `chr_loop_in` | `nsession.rs:5042`, `5786-5807`, `5175-5178`, `5153`, `5717` | Delete |
| Client chr-loop gate `table = self.own.ready() \|\| !own::installed()` | `nsession.rs:5667-5705` | Back to the pre-test11 gate: first 23 only. The failure line goes back to "no ownership list (23) within 5 s" |
| `dblock::boss_taken` callers (table transitions) | `nsession.rs:3212-3215`, `3543-3545` | Rewire (commit 1): call it when a boss's **native** `owned_here` turns 0→1 here (not on first sight). This keeps the fresh-AI stealth veto for bosses that move between machines natively |
| `host_candidates` | `nsession.rs:5820-5858` | Keep as is: plain feed plus the first-22 forced 23. It also caches each client's last parsed 22 (netId→prio) for diagnostics |
| `PauseTracker` | `nsession.rs:2350-2378` | Keep only for the existing "world stopped" log lines. It no longer drives presence |
| `settings.rs:468-470` comment; `PROTO` 6 | `settings.rs`, `nsession.rs:157` | Fix the comment; `PROTO` = 7 |
| Tests: `owner_table_answers_only_while_active` (7030), message round-trip entries (6363-6400), all of `ownership.rs` tests | | Delete or adjust |

**Kept as is** (not ownership deciders, all evidence-based):
- the call-site role fakes, the fake member vector and the 23 recipient wraps;
- `feed_22`, `clear_record`, the pnet rewrite;
- the `dblock` deathblow lock (§39), `livesync`, `persist`, `bossrules`, `bossgate`;
- the engage HP check;
- the one-shot countdown nudges (`make_due`, `nsession.rs:5592-5602`, `5835-5843`, `6123`).

The nudges are kept on purpose and are the one place we differ from "nothing more than Yui". They write 0 to a **countdown** (`T+0x39C` / `T+0x3A4`), not to a period. Our session re-engages after every load, and the 5 s first‑23 rule would otherwise fail against the native 5 s + 3 s pipeline (live 7.1, IMPL §18b). Yui keeps one session across loads, so he never needs them.

**Not touched:** pickups, aitarget (observe only), wardrobe/outfits, Karma.

---

## 2. Baseline (step 1): Yui's pieces, and how ours compare

### 2.1 Role answers: Sekiro Online hook table vs ours (`nsession.rs:1212-1250`)

| Purpose | Sekiro site | Yui | Ours | Verdict |
|---|---|---|---|---|
| InSession in 4/5/70/71 send and receive | `0xA1AF9C 0xA1B36D 0xA1BE41 0xA1C18B 0xA1C8A3 0xA1CA03 0xA1D918 0xA1DB08` | yes | yes (TRUE5) | same |
| InSession, type‑5 full send | `0xA1CCCA` | not listed | yes | keep (live-proven) |
| InSession in the item-lot drop code | `0x91870B` | yes | no | not needed for enemy sync; our global InSession is false, so drops work [KB ds3 §3: `0x918090` needs it 0] |
| IsHost 22 / 5 changed / HP write | `0xA1CF85 0xA1D4B0 0xA4B8A4` | yes | yes | same |
| IsHost 23 | `0xA1DDC6` | his "+1 unresolved" is likely this [U] | yes | same in effect |
| IsHost type‑5 full | `0xA1D42A` | no | yes | keep |
| IsClient read 5 / HP read | `0xA1D506 0xA4BA81` | yes | yes | same |
| IsClient | `0xA6F351` | yes | no | world-state / flag bank area (`0xA6F404`); not enemy sync |
| Client HP offline test | `0xA4BA93` | no | yes when GameMan+0xBA6 ≠ 0 | keep |
| Member count, chr loop | `0xA4DC38` | always | yes; the client applies it late, after its first 23 | keep ours (live 7.1: frozen enemies before the first 23) |
| Member count in `IsOwnedHere(chr)` | `0xA4A6CB` | no | yes | keep: makes the one bypass `0xA4A680` consistent with the native array (gap map §1.2) |
| Member count | `0xA734D8` | yes | no | **[U] purpose.** Read the function before deciding. Not added in the baseline |
| Member count, ObjAct | `0x6BC504` | yes | stage 4 | same |
| Inline counts 22 / 23 / candidates | `0xA1CF13 0xA1DD5E 0xA4E5BE` (mid-hooks) | yes | patched with `mov ecx,0x70` + nop (`0xA1CF13/24`, `0xA1DD5E/65`, `0xA4E5BE/C5`) | same effect |
| Offline slot selector in the world-state loaders | `0x76F10B 0xF0E6B7 0x89A689 0xA4CF79` | yes | no | join snapshot only; deviation D7 |
| Global IsHost / IsClient (`0x8E4790` / `0x8E44C0`) | — | not faked | not faked | same. Both machines read "offline/host" at the remaining callers |
| EMEVD multiplayer conditions | `0x69D44C..` ×6, `0x6A2F8E` | forced false | not patched; the false globals give the same single-player branch | same outcome |
| ESD env ids 60‑67, 3980 | `0x8522B0` | answered | not answered; bossgate handles fog/boss start | not needed for ownership |

**Verdict.** For enemy sync, distribution, damage and the HP table we already do what he does, plus a few proven extras. Nothing in the role layer needs to change for this series.

The global-role callers that matter only later are the damage router action 2 (`0xB6A040`), the SpEffect gate (`0x9E9D10` and siblings), corpse force-local (`0x9F4130`) and the event death branch (`0x69D310` / `0x68AA80`). They are deviation D8. Both mods leave them unfaked.

### 2.2 Receive loops over the empty member list

He NOPs the loop and empty-list jumps: 70 at `0xA1B035`/`0xA1B250`, 4 at `0xA1BEB3`/`0xA1C09D`, 71 at `0xA1D946`/`0xA1DA65`.

We do the equivalent differently. A fake `S` with one empty 56‑byte member (`LEA_SITES` `0xA1AFB1 0xA1AFF4 0xA1BE56 0xA1BE91`, stub at `VEC_CALL_SITE 0xA1D936`) makes each loop run once. Our `0x8E6B50` detour then serves our queue.

The result is the same: one pass per call, and any peer's packet is served. Ours is live-proven. **Keep ours.**

### 2.3 Session-leave guard on loads (`0xA76B62 → 0x832180`)

This is not applicable as such. We do not run the native SessionManager, so there is no native session to lose on a load. Our session ends on purpose at every area load (`DISENGAGED (area load)`) and re-engages after settle, handshake and steady (about 6 s or more).

That re-engage window, when every enemy runs locally on both machines, is the cost of our design compared with his. Changing the session lifetime is deviation D6. It is out of scope here; commit 2 measures it.

### 2.4 Hit relay

- **Lives byte, adopt (commit 3).**
  - He prepends the attacker-side lives (`data+0x25C`, `data = *(*(chr+0x1FF8)+0x18)`) to each relayed 21.
  - The receiver, before the hit is parsed: if its copy has lives > 0 and the byte is non-zero and different, it sets lives = byte, HP = max and posture = max. The hit then applies on top.
  - We sync lives with the Lives message (a 2 s full resend plus changes), a min-merge, and `Plan::Lower` / `Plan::Kill` with `KILL_GRACE` (`livesync.rs:574-679`).
  - In-order delivery is exactly what the deathblow case lacks (§3).
  - **Ours, with one deviation:** apply only when byte < ours, so the stricter min-merge stays authoritative. Livesync stays.
- **Hits aimed at the local player, guard (commit 5).** He drops received 21s whose victim is the local player (victim-decided damage). Natively, a replica's hits are reaction-only (action 0), so such a 21 should not occur. But `pvp.rs` changes damage-table cells (`pvp.rs:821`), so a blind drop could break friendly fire.
  - Count and log them first.
  - Drop only when the victim is our own player and pvp friendly fire is off. Decide after reading `pvp.rs` around `DMG_TABLES`.
- **Type‑70 validation, adopt (commit 4).** These checks run before the game parses: block index `netId>>11 < *(T+8)`, block pointer at `T+0x10+8*idx` non-null, record count below the block capacity (`+0x10`), each record ≤ 0x138 bytes and the total ≤ 0x5700, every netId resolves.
  - The per-record size comes from the game's size function, called from `0xA1AF40`. Read `E:\RE\work_net\f\f_a1af40.c` to get its RVA [U until read].
  - Without it, check the total, the count and the netIds only.
  - This is pure crash insurance; we have none today.
- **HP.** We keep native type 5 (host-authoritative, live-proven). For his build the study found a type‑5 receive hook but no sender [U]; nothing to copy.

### 2.5 What "native ownership" means after step 1 (for reviewers)

- The host arbitrates.
  - **Client record:** filled only by our `feed_22`, every 5 s, when the client's 22 arrives.
  - **Host record:** rewritten **every frame** from the host's own candidates, but only when that list is non-empty [C `f_a1ced0.c`: host branch `if (0 < count) 0xA1C6C0(...)`].
- Every 3 s, `0xA1D550` prunes records whose player handle is gone. `0xA1C590` then removes the loser's entry: the higher prio wins; on a tie, the record with fewer entries wins (the `count <= count` test), and the stored list is mutated [C].
- 23 goes to each member. The receiver resets its whole table and marks only its own netIds.
- **Freezes from "nobody" windows cannot happen structurally.** A hand-over is a single 23. At most there is a one-way-latency overlap or gap.
- **Ownerless far enemies are by design** (gap map §3.6).
- **Enemies loaded only on the client** (test11 "client-only rows") are handled natively. The client nominates them, nobody contests, and the host's 23 lists them for the client from the client's record.

---

## 3. First-class case: deathblow or throw on an enemy owned by the other machine

Setup: player A, enemy E owned by B. No ownership change is involved anywhere.

1. **Gate on A.** A's copy of E is NetAI.
   - A's hits on it are router action 3: applied locally and broadcast as 21 [KB IMPL §18b].
   - So A's copy takes HP and posture damage locally, and the posture break shows on A's copy.
   - The throw candidate check `0xBE3810` evaluates A's copy. Our `dblock` detour (§39) turns "yes" into "no" when B holds the lock.
2. **Start.** A's game sends type 18 (`+0x20` defender, `+0x22` attacker; pnet rewrite). `dblock::on_sent` takes A's lock. On B, `dblock::on_received` / `first_come` settle a collision. Then `0x7FAB00` places both chrs and plays the throw with A's remote as attacker; that hit is reaction-only on B.
3. **Damage and death.**
   - On A the throw's hit applies to A's copy and goes out as 21 (action 3).
   - B applies the 21 through `0xA4F940`; the host's type 5 then makes HP agree.
   - **Single-life enemy:** B's copy dies from the HP.
     - Backstop: `livesync` `Plan::Kill` (ForceDead) after `KILL_GRACE`.
     - Boss: deferred while the throw anim plays (v8.29a).
   - **Multi-life enemy:** A's copy consumes a life locally. B's copy learns it only from the Lives message, through `Plan::Lower` with a refill (`livesync.rs:609-628`).
     - Until then B's AI (the owner) drives E's records, so on A's screen the enemy "gets up" with the old life.
     - §34 recorded this: "the friend's copy kept its life ... until livesync ForceDead it 2 s later".
4. **Why test12 showed "stand back up" more.**
   - With the table, a claim was refused whenever the other player stood within 20 m. So the deathblow nearly always ran on the non-owner.
   - Hand-overs sat at NOBODY, so nobody's AI produced records and the copies froze or re-stood.
   - The native baseline removes the NOBODY windows. The lives byte removes the ordering gap: B's copy drops the life, refilled, in the same packet as the hit, before the hit applies.
   - Whether the deathblow's kill on a single-life enemy actually travels in a 21 is **[U]**: the ninsatsu kill could be applied by the throw and not by the router. The commit‑6 trace answers it.
5. **Stealth deathblows on a non-owned enemy.**
   - The throw chain has no ownership check (RE-BACKSTAB §b).
   - The "unaware" throwable state comes from TAE/HKS. On a native NetAI copy it plays from the owner's replicated behaviour (70). §27.2's "only the local AI produces it" came from mode‑4 evidence [U for native].
   - The owner's AI perceives the remote player without its stealth SpEffects (`0x9E9D10` acts only for the local player), so it notices a sneaking partner earlier than solo. Yui accepts this.
   - **Baseline: no claims.** A boost is only justified if commit 6 shows stealth markers missing on non-owned unaware enemies (deviation D4).

---

## 4. Diagnostics in the baseline

### 4.1 One minimal message: `OwnView` (id 5, PROTO 7)

- Client → host, every 2 s: `{u32 generation, u8 origin, u16 n, u16 running[n]}`.
- `running` = alive enemies whose ComManipulator is active here (`netai::active_kind == 1`), sorted, capped at 2048.
- It goes on the control lane, is not relayed, and is accepted under `accepts()`.
- Pure detector in a new module `ownview.rs` (about 80 lines plus tests; no game reads): `fn step(prev: &Report, cur: &Report, host_running: &Set, near: &[(u16, f32)]) -> (doubles, ownerless)`. A netId counts only when it holds in two consecutive reports (≥ 2 s apart, far longer than a 23 in flight).

### 4.2 Log lines (prefix `own:`)

- **`own: DOUBLE 0x.. e.. runs on host and peer1 (2 reports)`** (host).
  - Adds the host copy's force-local inputs: `+0x2040`, `U+0x20`, `0x9E5C00(U)`, module0 `+0x54`.
  - These are the gap map's only non-prio double causes, plus the unreachable prio‑0 quirk.
- **`own: OWNERLESS-NEAR 0x.. e.. d host H / peer1 C`** (host).
  - The enemy is alive, within 40 m of a living player, and running on neither machine in two reports.
  - Classified by gap map §3.1: interval `chr+0x110`, `chr+0x1A10 & 8`, `U+0x10/+0x14/+0x20`, host prio `chr+0x1FC8`, and the client's prio from its last 22 (cached in `host_candidates`, "none" = not nominated).
  - Capped at 10 lines per 10 s, all counted.
- **`own: BOSS 0x.. e.. owner here|elsewhere (native 23), prio here P, peer P2`.**
  - Both roles, on every change of the native answer for a boss or multi-life enemy (`is_boss`, `nsession.rs:2794`). Never capped.
  - The boss_taken veto line follows on a 0→1 change.
- **`own: FLAP 0x.. N owner changes in 10 s`.** Both roles, ≥ 3 changes of the native answer.
- **`own: SLOW 0x.. peer1 nearer by D m for T s, still the host's (prio host P / peer P2)`** (host).
  - Fires for an enemy within 10 m of the client where the client is ≥ 8 m nearer for > 4 s.
  - This is the v7.9 deflect-timing evidence.
- **`own: OWNER-DOWN N enemies (B bosses) owned on a dead/spectating machine within 40 m of the living player for > 5 s`** (host).
  - Uses `world::down_slots`, `bossrules::spectating`, the remote's HP and OwnView.
  - This is the v8.55/v8.57 evidence.
- **`own: HOST-SILENT T s: K enemies near us owned by the host stand still`** (client).
  - The host's 4/5/70/71 have stopped while engaged (host world paused).
- **10 s summaries.**
  - Host: `own (10 s): host H / peer1 C (OwnView), doubles D, ownerless-near U [..], flaps F, slow S, owner-down O; 22 rx R, 23 tx T; prio hist host [0 / <256 / <1024 / <1792 / >=1792], peer [...]`.
  - Client: `own (10 s): owned here H, 23 rx T, flaps F, host-silent s`.
- **Re-engage cost** (commit 2, D6 evidence): `nsession: load -> ENGAGED in T ms (enemies local on both meanwhile)`.

### 4.3 Deathblow trace (commit 6, both roles)

- On our own 18 as attacker, start an entry: victim netId, `owned_here`, lives, HP.
- After 3 s, one line: `deathblow: TRACE 0x.. owned here|elsewhere, lives a->b HP h->h2 here, 21 sent k for it, the friend's lives (livesync) r, outcome died|life lost|stood up`.
- Receiver side, one line per received 18 on an enemy owned here, with the 21s that followed and the lives byte applied or not.
- If `dblock`'s gate saw a stealth-category "yes" for our player on a non-owned enemy, note it.
- Answers §3.4 [U] and the D4 need.

---

## 5. Deviations: optional later steps, each only with evidence

Common rule for every engine write below:
- Read and verify the expected value first. `chr+0x1A1C` must be 0 or the last value we wrote to that chr; `T+0x398` must be 3.0 or ours; `T+0x3A0` must be 5.0 or ours.
- On a mismatch, log once and skip.
- Track what we wrote per chr pointer + netId and per T pointer. On teardown, solo, or leaving the step, restore 0 or the original where the value is still ours and the chr or T still exists.
- Game thread only (our FrameBegin).
- New tables come with every load, so no restore is needed after an area load.

**D1. Boss pin.**
- Symptom that justifies it: boss owner flips mid-fight (v8.12/§53, test9). Evidence: `own: BOSS ... owner` changes or `FLAP` on a boss in commit‑2 logs.
- Lever: host only, `chr+0x1A1C = 4095` while the host is present and within 150 m of the boss (unpin past 170 m); 0 otherwise. Clients never write it.
- Mechanism [C `f_9f0230.c`, `f_a4e520.c`]: a signed i32 added to prio (clamped 0..0x7FFF), and `>0` also makes the boss a candidate at any LOD.
  - The host record wins every 23 (natural max ≈ 1791).
  - The client's entry is pruned each resolve and comes back with its next 22.
- The engine's own path is a direct `mov [chr+0x1A1C]` (`0x9F4340` via 2004[28]).
- Yui: Sekiro Online pins nothing (bosses are distance-distributed). Which machine simulates a boss in DS3 Seamless is **unknown** without the DS3 binary.

**D2. Dead or absent owner.**
- Symptom: Divine Dragon immortal for 165 s while the dead host owned it (v8.55/v8.57). Evidence: `OWNER-DOWN` with B > 0 in a boss fight.
- Yui does nothing here: the dead player's PlayerIns and its share of the distribution stay.
- Levers, matching the native disconnect behaviour (`0xA1D550` drops a gone player's record):
  - **Client absent:** the host calls the existing `clear_record(slot)` (`nsession.rs:6108`) and feeds an empty list in place of that client's 22s while it is absent. Host only, no chr writes.
  - **Host absent:** set `chr+0x1A1C = -0x10000` on every host chr with a netId (`all_chrs()`, not only `enemies()`, because the candidate builder walks every chr set). The host's prio becomes 0, so the host nominates nothing (`0xA4E83C` skips prio 0).
    - Also clear the host's own record once with `0xA1D7E0` + `0xA1C6C0(rec, 0)`. The host branch does not overwrite its record while its list is empty [C], so a stale list would win otherwise.
    - Then make 23 due.
    - On return, restore and make 23 due.
  - **Nobody present (wipe):** change nothing.
- **Host world paused:** no 23 can run, so nothing native helps. `HOST-SILENT` measures it first.

**D3. Shorter timers.**
- Symptom: v7.9 deflect timing. Evidence: `SLOW` lines.
- Lever: host `T+0x398` 3.0 → 1.0 s; client `T+0x3A0` 5.0 → 1.0 s.
- Cost: 22/23 are a few hundred bytes per second, and the resolve is cheap with two records.
- Native hysteresis is none [C]. If `FLAP` rises, add a hold bias of +32 on `chr+0x1A1C` (about 8 m of the distance term, under one 256 LOD tier). Apply it only to enemies owned here that are natively relevant (interval −1..4 or `1A10 & 8`), so the candidate gate is unchanged.
- `chr+0x1A20` is unknown, written by `0x8057C5` [KB], so do not use it.

**D4. Stealth claim as a boost.**
- Symptom: no stealth marker, or an early notice, on non-owned unaware enemies. Evidence: the commit‑6 trace or gate lines.
- Lever: `+2048` on `chr+0x1A1C` of the **claimant's own copy** (beats a natural ≤ 1823), plus `make_due(T_DUE_22)` on a client claim. The host forces 23 when a received 22 carries an entry with prio ≥ 2048.
- Why this avoids §34's problem:
  - Pre-test11 injected `0xFFFF` into the client's wire 22. That is above the 0x7FFF clamp the host's own prio can reach, so a client claim always won. The host could express its claims only by stripping and contests, and latency was 5 s + 3 s.
  - A boost on the claimant's copy goes through the same clamped formula on both machines. Two claims cancel, and natural prio decides. No message and no contest code are needed.
- Never on bosses.

**D5. AI target seeding after a native take.** Outline only.
- The frame after the switch to Com, after `0x7E54D0` has wiped the AI targets: take the NetAI copy's `manip+0x420/+0x440` captured the tick before, then `0x630C50(slots, 0, chr)` or `0x674640(EventMan, enemyEnt, playerEnt)`.
- Never for a target with no prior record.

**D6. Session across loads.** The analog of Yui's `0xA76B62` guard. Evidence: the `load -> ENGAGED` time from commit 2. Out of scope.

**D7. Join `"CHR "` snapshot** (`0xA4D0A0` / `0xA4CF60`). Ours is the persist dead set. Follow-up only.

**D8. Global role call sites.** Corpse force-local (`0x9F4130`), SpEffect gate (`0x9E9D10`), router action 2 on the host. Per-site fakes, only with a symptom.

**Alignment with his designs (no change in this series):**
- Our phase A spectate (`bossrules`: reload held while the partner lives, camera on them, phase B off since §54) is his "death → spectate" state machine.
- Wipe → reload is aligned (persist wipe guard).
- Bosses as session-global events (fog and reward relayed) are covered by bossgate / bossrules / keyitems.

---

## 6. Commits on `anton/test14-native-ownership`

Each commit builds, tests and is live-testable.

1. **`own: delete the host-authored owner table; native distribution decides`**
   - Everything marked Delete in §1; the claims path in `enemies.rs` and nsession.
   - `boss_taken` rewired to native 0→1 transitions.
   - Gate back to the first 23. `PROTO` = 7.
   - Message tests adjusted.
   - About −2,600 lines (about −1,350 `ownership.rs`, −900 `own`, about −250 messages, claims and tests) and +30 lines.
   - Shippable on its own.
2. **`own: passive owner view (OwnView, DOUBLE / OWNERLESS-NEAR / BOSS / FLAP / SLOW / OWNER-DOWN / HOST-SILENT)`**
   - Pure `ownview.rs`, unit-tested: two-report rule, ownerless classification input, flap window.
   - Message round-trip and truncation test.
   - Also the `load -> ENGAGED` line.
   - About +350 lines.
3. **`nsession: lives byte on relayed hits (21)`**
   - Send (`route_send`): append 1 byte = the victim enemy's `data+0x25C`. Look the chr up from a netId→chr map cached by the last livesync scan; 0 = unknown.
   - Appended at the end so the pnet offsets `+0x00/+0x0C` are unchanged; the host relays it untouched.
   - Receive (`serve`, game thread, before the copy to the game buffer): strip it. If our copy's lives > 0, byte > 0 and byte < ours, use livesync's `set_lives` + `refill` (lives, max HP, max posture), then pass the hit.
   - Pure `lives_byte_action(ours, byte) -> Option<u8>` unit-tested.
   - Log: `hit: lives byte b for 0x.. (ours a): lowered and refilled before the hit`.
   - About +120 lines.
4. **`nsession: validate type 70 before the game parses it`**
   - Pure checker over `(bytes, block_count, capacity_of)`, unit-tested with crafted packets.
   - Read `f_a1af40.c` for the layout and the size-function RVA.
   - Drops counted in the `types` line.
   - About +100 lines.
5. **`nsession: count hits aimed at our own player`**
   - Counter and line. Drop only when friendly fire is off and the victim is our player, after reading `pvp.rs`.
   - About +40 lines.
6. **`deathblow: trace`** (§4.3). About +120 lines.
7. **`docs + version`**
   - `IMPLEMENTATION-NOTES` test14 section.
   - Mark `PLAN-OWNERSHIP.md` superseded; banner in `enemy-authority-handover.md`.
   - FORK.md checklist.
   - `MOD_VERSION` (`ui.rs:162`) → `1.0.9-pickups.3-test14`.

Net about −1,900 lines. D1–D5 are each a separate later commit with a pure, unit-tested `bias()` / guard function.

---

## 7. Historical symptoms: expected outcome and the evidence that answers each

| Symptom | Expected with the baseline | Log evidence |
|---|---|---|
| §31 ownerless near enemies | Mostly solved: the gap map says the causes were mod-side. Ownerless far enemies stay, by design | `OWNERLESS-NEAR` count, plus the clause that rejects each one |
| §53 / v8.12 flapping bosses | Bosses move only on 3 s 23s and only when LOD/distance flips. Possible | `BOSS` / `FLAP` on boss netIds → D1 |
| v7.9 deflect timing | May come back, since a hand-over takes up to 8 s with the native timers | `SLOW` → D3 |
| §34 deathblow contests | Gone (no claims). Collisions are settled by `dblock` | `deathblow:` lock and collision lines; TRACE |
| test12 freezes | Gone (no NOBODY windows) | `OWNERLESS-NEAR` near 0 while both are near |
| test12 stand-up after deathblow | Fixed by the lives byte (multi-life); single-life is [U] | TRACE outcome, `hit: lives byte` lines |
| v8.57 dead host keeps the boss | Comes back (Yui does nothing here either) | `OWNER-DOWN` with a boss → D2 |
| Host world pause | Native cannot help | `HOST-SILENT` |

---

## 8. Risks, and what only a live test can show

- **The `0xA4A650` hook is gone.** Every reader goes back to the native answer, including `livesync` and the router. This is the pre-test11 7.4 behaviour minus ownfill.
- **The first owner after a load** comes only with the first 23 (the gate holds the client's chr-loop patch until then).
- **Stealth through a NetAI copy** is [U] (§3.5).
- **Lives-byte timing:** whether the attacker's lives already dropped when the throw's 21 is sent is [U]. TRACE shows it, and the byte < ours rule makes a stale byte harmless.
- **`0xA734D8`** is unread.
- **Prio values** have never been seen live. The histogram in the summary shows them.

## 9. Co-op checklist (both PCs on test14, `native_session_stage = 3`)

1. **Engage.**
   - Client: `the host's sync runs`, then `first 23 ... members chr loop patched`.
   - Both: `own (10 s): ...` summaries with `doubles 0`. No `table` lines anywhere.
2. **Group fight, both near.** `ownerless-near 0`, `doubles 0`, few `FLAP`.
3. **Client goes 40 m ahead into a new group.** The enemies fight on both screens. The host's summary shows peer1 owning them within about 8 s, and `SLOW` lines if slower.
4. **Posture deathblow on an enemy the other machine owns** (host, then client).
   - Multi-life: one orb gone on both. On the owner the received type 18 takes the life and the byte is skipped (`hit: lives byte ... skipped: a deathblow on it applies here ...`, review 2026-11-15: applying it too took two lives); on the owner's own deathblow the non-owner logs `hit: lives byte ... lowered and refilled before the hit`.
   - TRACE: `owned elsewhere ... outcome life lost`, no stand-up.
   - Single-life: `outcome died`.
5. **Both press deathblow together.** One animation. `dblock` collision lines as in §39.
6. **Stealth backstab on an enemy the other machine owns.** The TRACE gate result decides D4.
7. **Boss / miniboss (Guardian Ape, the giant serpent encounter).** Watch `BOSS` lines; any `FLAP` on the boss is D1 evidence.
8. **Host dies in a boss fight and spectates.** `OWNER-DOWN ... B 1` is D2 evidence. Check whether the boss can still be finished.
9. **Load, rest, reconnect.** `load -> ENGAGED in T ms`; patches restored and re-applied.
10. **2-hour soak.** `doubles` total near 0, with every exception classified by its `+0x2040` / force-local fields.

## 10. Explicitly not done

- No `IsOwnedHere` override, no priority, period or claim writes in the baseline, no ini mode switch.
- No migration code; the old code lives in git on `anton/test11-ownership`.
- No global role fakes.
- No change to the 22/23 feed beyond caching the client's list for diagnostics.
- No N > 2 tuning.
- pickups, aitarget, wardrobe and Karma are untouched.

### Critical Files for Implementation
- E:\REPOS\sekiro-coop-wt\anton-test7\crates\coop-dll\src\nsession.rs
- E:\REPOS\sekiro-coop-wt\anton-test7\crates\coop-dll\src\ownership.rs (deleted)
- E:\REPOS\sekiro-coop-wt\anton-test7\crates\coop-dll\src\livesync.rs
- E:\REPOS\sekiro-coop-wt\anton-test7\crates\coop-dll\src\enemies.rs
- E:\RE\work_net\f\f_a1ced0.c, f_a1af40.c, f_9f0230.c (RE inputs for commits 3–4 and D1–D3)
