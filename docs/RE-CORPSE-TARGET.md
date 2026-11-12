# RE: enemies keep on a dead player's corpse (Sekiro 1.06, test6)

Static analysis, Ghidra 12.2 (pyghidra headless) on the 2026-11-11 in-game dump
(`E:\RE\carve\sekiro_mem.exe`), private project copy `E:\RE\ghidra_corpse`, jobs and outputs
`E:\RE\work_corpse\o1..o13.txt`. Addresses are RVAs (VA - 0x140000000). Builds on
`RE-ENEMY-TARGETING.md` (IDA, upstream) and `RE-DEATH-RESET.md`.
Marks: **[VAR]** read in the decompiled code / bytes, **[ARV]** inferred, **[G]** guess.
Nothing here was checked in the running game.

## 1. How an enemy's AI picks and keeps a target

`SprjTargetingSystem::Update` `0x61FF40` -> `0x623CA0(ts, dt)` [VAR]:

```
0x626430(ts, dt)                 // awareness list ts+0x40..0x48, 40 B per entry
  for e in list: perception(e)   // 0xBD3E40 fills a record from the target's module +0x38,
                                 // 0x6172F0: "perceived" unless record+0xC3
  for e in list:
    0x612400(e, dt)              // awareness += (perceived ? rate : -decay)
    if !0x612320(e): remove      // target dead (0xBD5D40) or awareness <= 0
    best = max awareness >= threshold
  best -> 0x6285C0 / 0x629250    // target slot set, timer = NpcThinkParam +0x80
if search timer fired:
  0x6250A0 SearchHostile -> 0x62BF30(selfHandle, selfTeam, &hostile, &friend, mode)
                         -> 0x6244F0: perception 0x617280 per candidate -> awareness entry
battle target slot (type 1): if target HP < 1 -> 0x628AC0 (drop the type-0xC memory slot)
slots (type 1 NormalEnemy, 6 LastSightPos, 7 LastTarget, 9 CorpsePos, 0xC LastMemoryTargetPos):
  0x62D8C0 update -> 0x61DB10 (timer +0xC, stateInfo 332/333 and 334/335 pairs clear it),
  valid while 0x613840: target entity !0x9E5BB0 && !0x9E5C00 ("death state")
```

Checks that matter [VAR]:

| function | check |
|---|---|
| `0x62BF30` candidate filter | `chr+0x1A15 & 1`, `chr+0x1A15 >= 0x80`, `0x612B20(chr, mode)` (entity not 0x9E5BB0 / 0x9E5C00), not self, then `0xC09660(self team, 0x9F1510(chr))` hostile -> hostile list, else `0xC097F0` friendly -> friend list, else **dropped** |
| `0x612320` awareness entry kept | `!0xBD5D40(target data)` and awareness > 0 |
| `0xBD5D40` IsDead | `HP(+0x130) <= 0`, unless a SpEffect with conditionHp 0 and negative changeHpRate / changeHpPoint is on (`0xBFEF50` / `0xBFEEB0`; only row 900100 "pretending to be dead" in SpEffectParam) |
| `0xBD3E40` perception record | `+0xC3 = byte 0x143D7A36F` only when `0x9F3430(target)` (= `[[0x143D7A1E0]+0x88] == target`, the main player) |
| `0x617280` / `0x6172F0` | `+0xC3 != 0` -> not seen, not heard (returns 0) |
| `0x9F1510` team | `u32 chr+0x74`, overridden by SpEffect stateInfo 302 (owner's team) / 132 (team 15) |

```c
// 0x617280 (new candidate)                     // 0x6172F0 (known entry)
if (rec[0xC3] == 0) {                           if (rec[0xC3] == 0) return 0x61C2A0(rec) != 0;
    if (0x61BB70(rec, eyes)) return 2;          return 0;
    return 0x61C940(rec) ? 1 : 0;
}
return 0;
// 0xBD3E40 (tail)
if (0x9F3430(target)) rec[0xC3] = *(u8*)0x143D7A36F;
```

References to `0x143D7A36F` (whole image, RIP-relative scan): `0xBD4098` in `0xBD3E40` (read)
and `0xA5426B` in `0xA52B80` (debug menu registration, `lea`). Nothing else reads or writes
it [VAR]. (`0x11099C5` reads `0x143D7A370`, the next byte.)

Team matrices (`E:\RE\work_team\team_matrices.txt`) [VAR]: M2 `0x143B0CAA0` column 0 is the
not-hostile functor for every row; M1 `0x143B0A680` column 0 is the "D" functor (rows 0, 3,
5, 8-12, 23 are D everywhere). Team 0 is "Disabled" in the EMEVD enum. Enemy rows 6/7/13/24
are hostile to 1, 2 and 28: friendly fire's team 28 does NOT hide a player from enemies.

## 2. Why a corpse stays a target here

- In single player a death always reloads the map (`0xA8F7E0`), so a lasting "dead player in
  a living world" never happens in vanilla; the AI only has to stop for a few seconds.
- What the AI has for a dead target: HP <= 0 drops the awareness entries every frame
  (`0x612320`), the type-0xC memory slot goes (`0x628AC0`); the battle slot (type 1) stays
  until its timer (NpcThinkParam `+0x80`) runs out or the target's entity reports the death
  state `0x9E5C00` (`*(*(chr+0x18)+0x30)+0x8C & 1`). The candidate filter itself has no HP
  test. [VAR] What sets the death-state bit was not found statically [ARV: the vanilla death
  flow]; the mod's phase B spectate keeps the dead player in the world without the
  CSDeadStep for minutes.
- Under the mod every machine runs some enemies' AI (ownfill / the game's type-22 split);
  the other machine shows a NetAI copy following it. So whatever one AI does with the corpse
  shows on both screens.
- Live 2026-11-12 (client log 21:01:12-21:02:19, miniboss 1500490 netId 0x380d, host down):
  the client took the boss (`ownfill ... take why=wanted_boss`), dropped the forced bit 266 ms
  later when the game's own answer turned 1 (`drop_on_native`: true for every client, so the
  "(sticky: kept)" text was wrong), and re-took it when the answer fell back to 0 (the downed
  host is out of `boss_rivals`). Three moves in 45 s; one second after the last take the
  client did a stealth deathblow (throw 59972; the posture deathblow is 59952) at 73 % HP,
  posture not broken: the AI that had just moved had no "noticed the player" state. [VAR log
  + code, ARV for the fresh AI]
- Root cause, as far as offline analysis goes [ARV]: the game's own dead-target handling is
  short-lived and only per AI instance; nothing in vanilla stops a living-world AI from
  coming back to a dead player's position (CorpsePos / LastSightPos slots) or keeping a
  long battle-slot timer, and the ownership see-saw restarted the AI on alternating machines.
  Not proven which of these dominates; see the live test.

## 3. Mechanism (corpse.rs, `[GAMEPLAY] dead_players_untargetable = 1`)

Candidates evaluated:
- (a) team switch: the replica gets team 0 while its owner is dead. Uses `0x62BF30` (neither
  hostile nor friendly -> not a candidate), and lock-on / the idol enemy finder (same M2).
  Does not clear an awareness entry by itself (no team re-check in `0x612320`), but the
  copy's HP 0 does. Chosen for the friend's copy.
- (b) a per-chr flag the AI respects: `+0xC3` "not perceived" exists, but its source is a
  single global byte that applies to the main player only. Chosen for our own dead player.
  The entity death-state bit `0x9E5C00` would invalidate slots at once, but it is read by the
  chr update loop, the CSDeadStep predicate `0xA740D0`, the kill `0x9EE7E0` and the
  force-local rule `0x9F33F0`: too many side effects, not used.
- (c) clearing enemies' target slots directly: no clean entry point found (slot lists are
  internal to each TargetingSystem), not used.

Apply / restore (each machine, every frame, only from what it already knows):

| who | applied when | restored when |
|---|---|---|
| our player | HP <= 0, link up, setting on, some peer alive (snapshot visible, HP > 0): byte `0x143D7A36F` 0 -> 1 | HP > 0 (Resurrect), no living peer (wipe), no link, setting off, loading screen, player re-created, hub reset (panic), mod shutdown |
| a peer's native remote | its visible snapshot HP <= 0, link up, setting on, we or another peer alive: `chr+0x74` -> 0 (original remembered) | snapshot HP > 0 or no snapshot, nobody else alive, no link, setting off, hub reset, shutdown: team 2 (pvp sets 28 again itself); a removed / recreated remote is forgotten without writing (loading screen, respawn, `check_revive`) |

Not host-synced: each machine changes only what its own enemies' AI reads; a mismatch only
means the other machine's AI behaves as before.

Friendly fire (pvp.rs) skips team reads / writes on a held remote (its "cannot die" bit still
follows the owner's HP, so the copy can reach HP 0).

## 4. Ownership see-saw and the fresh-take veto (nsession.rs, separate commit)

- `ownfill::drop_on_native(me, is_boss, rival_down)`: a boss is not dropped on the game's
  answer while another engaged player waits dead (`world::down_slots`); host bosses as before.
  The log says "forced bit kept" / "forced bit dropped: the game's own answer runs it here now".
- `dblock::boss_taken`: a boss taken while a rival is down arms a 10 s window
  (`TAKE_VETO_MS`); the throw defender gate `0xBE3810` refuses our player's throws on it
  unless `justified(hp, posture)`: posture `<= 0` (broken, it counts down) or HP `<= 1`
  (the "cannot die" floor of an enemy with lives left). Unknown values are allowed.

## 5. Live test (expected log lines)

1. Miniboss, A dies (phase B spectate), B lives. A's log: `corpse: our player is dead and
   another player lives: enemies on this PC no longer perceive it (hide flag 0x143d7a36f = 1)`.
   B's log: `corpse: peer N's native remote 0x...: is dead (snapshot HP 0): team 2 -> 0`.
   Expected: within a few seconds the boss turns to B or walks off; it does not keep hitting A.
2. A resurrects (Sekiro's own): `corpse: our player is targetable again (... alive again or
   no fresh state)` and on B `team 0 -> 2 restored (alive again or no fresh state)`;
   enemies notice A again (they may need to see / hear A first: vanilla stealth).
3. A respawns at the idol: A logs the clear on the loading screen; B logs `was replaced or
   removed: nothing to restore` (or `loading screen, ... forgotten` if B loads).
4. Both die: no `corpse:` apply line for the second death, and the first one's lines show
   `nobody else alive (wipe ...)`.
5. Host down, client fights the boss: `ownfill: BOSS ... take why=wanted_boss`, then at most
   `native owner answer 0->1 while forced (forced bit kept)`, no `given back ... (the game
   gives it to us now)` until the host is back. `deathblow: boss E netId ... taken while
   another player is down: ...` once. A stealth deathblow attempt in those 10 s:
   `deathblow: our attempt on 0x... refused: taken from a downed player's machine ...`;
   a posture-break deathblow in the same window goes through.
6. `dead_players_untargetable = 0`: no `corpse:` lines, behaviour as test5.

Open [I]: whether the battle slot of a boss times out quickly once the corpse is not
perceived (NpcThinkParam `+0x80` per boss); whether team 0 on a remote player has an effect
outside the AI search, lock-on and hit matrices (none found in the callers read here).

## 6. What gates a stealth deathblow (second incident, elite 1500495)

- The player's throw search (`0xBE0E50` -> finder `0xBDF930` -> `0xBDE5C0` -> candidates
  `0xBE2A30` -> defender gate `0xBE3810`, RE-BACKSTAB.md) has no awareness or AI-state test
  of its own [VAR, upstream IDA]. The defender side is the enemy's own throwable state
  (ThrowModule `+0x7A` from TAE event 932, else `+0x7B` from HKS act 161) checked against the
  kind's category (`0xBE0A40`, table `0x2A8DE90`), plus `defAliveCheck`.
- ThrowParam rows for the PC stealth kinds 0 / 100 / 10000 / 10100 need no SpEffect on either
  side (`additionalConditionSpecialEffect_forAtk/forDef` = -1 on all 98 rows) [VAR, param CSV].
- So "unaware" is decided by the enemy's behaviour script / animations, which are not in the
  executable. The AI state the script can query is per AI, not per target: Lua
  `IsBattleState` = `[AiThink+0x7B30]+0x158 == 3`, `IsFindState` == 2 (`0x5E4F40`,
  `0x5E5370`); the state lives on the TargetingSystem (`0x61FF40` writes `+0x158`). Its
  battle target is the NormalEnemy slot (§1). Whether the scripts open the "behind" window
  while in battle with someone else could not be read [ARV: they do in vanilla when enemies
  fight NPCs; the incident fits "AI fought the friend's copy"].
- Implemented as an option only, `[GAMEPLAY] stealth_deathblow_needs_no_combat = 0`
  (dblock `stealth_vetoed`): a stealth kind, AI running here, state 3, a player as the
  battle target, posture > 0 and HP > 1 -> refused at `0xBE3810`. Trade-off: it also ends
  "one player distracts, the other deathblows from behind". Posture (category 4) and
  Ninjutsu-after-posture kinds (41201-41203, category 4) are never touched.
- Stealth kinds (`dblock::STEALTH_CATEGORIES`, also the test6 fresh-take veto), from the PC
  rows' names in ThrowKindParam.csv: 0 behind (incl. roof rows and the stealth Ninjutsu
  201-203), 1 crouching behind, 6 plunging, 7 unrestricted fall, 9 / 10 wall hug left /
  right, 11 ledge hang, 17 behind in the 老化 state [ARV]. Not stealth: 2 deflect, 3 Mikiri,
  4 posture break, 5 / 12 special / event states, 8 anti-air, 13 posture-break fall, 14 kick
  break, 15 wire break, 16 finishing, 18 / 19 aging break, 20 break from behind, 21
  underwater break. The "unaware" condition itself lives in the enemy's TAE / HKS, not here.
- Live check: the new `throwprobe: +0 s ...` line shows `posture p/max` and
  `AI here|elsewhere state N (battle|find|other) target 0x.. (us|a peer's copy|not a
  player)` at every deathblow; a stealth deathblow with `state 3 ... (a peer's copy)` is the
  incident's pattern.