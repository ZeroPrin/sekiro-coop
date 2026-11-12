# RE: how Sekiro enemies pick and switch targets (1.06), and how the mod can steer it

Static analysis only. Ghidra 12.2 (pyghidra headless) on the 2026-11-11 in-game dump
`E:\RE\carve\sekiro_mem.exe`, private project copy `E:\RE\ghidra_ai`, jobs and outputs in
`E:\RE\work_ai_re` (`f\f_<rva>.c` = one decompiled function each, `luareg.txt` = all 884 Lua
bindings, `npcthink_offsets.txt`, `speffect_offsets.txt`, `lua\dec\` = all 673 AI scripts
decompiled with DSLuaDecompiler, extracted read-only from the game's Data*.bdt).
Addresses are RVAs (VA - 0x140000000). Marks: **[VAR]** read in code, **[ARV]** inferred,
**[G]** guess. Nothing was checked in the running game.
Builds on `RE-ENEMY-TARGETING.md` (upstream, IDA) and `test6:docs/RE-CORPSE-TARGET.md`, and
answers `docs/kb/ai-targeting.md` §6.5.

## 0. Short answer

- The engine, not Lua, picks the target. Every AI keeps a list of **NormalEnemy target
  records**, one per hostile character it currently knows (perceived, heard, hit by, forced).
  `TARGET_ENE_0` = the record with the highest **score** (`0x627EE0`). Scripts can only
  read it, clear the list (`ClearEnemyTarget`) or request a replan (`Replanning`). [VAR]
- Score per record (`0x61DE40`), recomputed every frame while it is <= 100 [VAR]:

  ```
  score = targetPriority(target)                       // sum of SpEffectParam +0xF8 on the target
        + (1 - dist/1000) * hitF * topF                // dist in m, clamp >= 0
        + (isTop && !replanningThisFrame ? 50 : 0)     // stickiness of the current target
  hitF = 1 + (hitTimer/10) * targetSys_DmgEffectRate/100   // hitTimer = 10 s after each hit, decays
  topF = 1 + (topTimer/14) * 0.1                           // topTimer = 14 s after becoming top, decays
  ```

- The user's three observations, explained:
  1. **Sticks to one player**: +50 for the current top dwarfs everything else (the distance term is
     at most 1, a hit doubles it at most). Bosses also never forget a perceived player
     (`SightTargetForgetTime` 99,999 s). [VAR]
  2. **Switches to the nearest only after a long time**: the +50 is dropped only on the frame the
     AI replans its goals (`TS+0x34`, set by the replanning controller `0x603690`). Then the raw
     terms decide. A new top is protected by `topF` (+10 %, which equals a 100 m distance lead) for
     14 s, so the nearer player wins only at the first replan after that, and only if it is
     perceived (has a record). [VAR formula, ARV for "when the AI replans"]
  3. **Switches at once to whoever hits it**: a hit sets the attacker's `hitF` to 2 (default
     `targetSys_DmgEffectRate = 100` on 769/771 rows), and Sekiro's hit reactions break the current
     goal so the AI replans right away. At that replan 2.0 beats ~1.1. [VAR formula, ARV replan timing]
- There is only one enemy slot exposed to scripts (`TARGET_ENE_0`). No "lowest HP / random /
  farthest" code exists for enemies; the only multi-target logic is this score. [VAR]
- Recommended route: hook the score function `0x61DE40` on the owning machine and add a
  DLL-computed bias in [-45, +45]. That gives per-enemy, per-player hate. Choices land on the
  engine's own replan frames, so the AI never turns mid-combo. Only targets the AI already
  has a record for are seen, so stealth is preserved. The hook is small (one function), and the
  risk is low to moderate. Details in §6.

## 1. Data model

### 1.1 Objects and how to reach them [VAR]

```
ChrIns (enemy) --vt+0x170--> ComManipulator (vt 0x29D2FB8)
ComManipulator +0x340 -> SprjAiThink ("AiThink")
AiThink +0x28    think-param wrapper {?, NpcThinkParam row* at +8}   (owner vt+0x58 = 0x60E8C0 returns it)
AiThink +0x68    u64 interrupt bitmask (Lua IsInterupt(n) = bit n)       (0x5E5CC0)
AiThink +0x7454  f32 damage accumulated this frame (Lua GetDamageLastFrame) (0x607540)
AiThink +0x7B30  SprjTargetingSystem* "TS" (vt 0x29635C0)
AiThink +0xAE78  CSIAiReplanningCtrlImpl (Lua Replanning sets +0xAE89 = ctrl+0x11)
```

`SprjTargetingSystem` (TS):

| off | type | meaning |
|---|---|---|
| +0x08 | ptr | owner `SprjAiTargetingSystemOwner` (vt 0x2961690); owner+0x08 = AiThink, owner+0x10 = ChrIns |
| +0x10 | list | slot list object; +0x18 = head node |
| +0x28 | f32 | search timer; when <= 0 the hostile search runs (`0x625F30`), refilled with rand*0.2+0.2 s (consts 0x143AFB488/48C) |
| +0x31 / +0x32 | u8 | request vt+0x18 (full reset) / vt+0x20 (clear enemies) on the next update |
| +0x34 | u8 | **replanning this frame**: suppresses the +50 top bonus; cleared at the end of `0x623CA0` |
| +0x35 | u8 | "first sight done" (LastSightPos/CorpsePos path `0x624850`) |
| +0x40..+0x48 | vec | peripheral awareness list, 40 B entries (§1.3) |
| +0x150 | u32 | flags (0x10000 = ShiftCurrTargetBattleState request, 0x200000 = notify sent, 0x800, 0x100000) |
| +0x158 | i32 | AI state: 0 normal, 1 caution, 2 find, 3 battle (Lua IsBattleState/IsFindState; written in `0x61FF40`) |
| +0x160 | i32 | current target type (Lua `GetCurrTargetType`, `0x5E2A60`) |
| +0x170 | u32 | handle of the current battle target while state 3, else -1 (`0x61FF40`) |

### 1.2 Slot list and the NormalEnemy record [VAR]

Slot nodes are a doubly linked list: node+0x00 vt, +0x08 u16 index, +0x0A u8 alive (0 = remove
in `0x62A510`), +0x10 next, +0x18 prev, **+0x20 record**, +0x104 reason (NormalEnemy), +0x110 owner.
Type ids (`vt+0x10` of each `TargetSearchSysSlot_*`): 1 NormalEnemy, 2 NormalFriend, 3 SoundPos,
4 SoulCoin, 5 Hallucination, 6 LastSightPos, 7 LastTarget, 8 LowHpFriend, 9 CorpsePos,
0xA FixedPos, 0xB PredictionPos, 0xC LastMemoryTargetPos, 0xD IndicationPos.
**Several NormalEnemy nodes coexist**, one per known hostile, unlimited (heap nodes, 0x120 B).

Target record (node+0x20; ctor `0x61C9F0`, clear `0x61DD20`, per-frame update `0x61DB10`):

| off | type | meaning |
|---|---|---|
| +0x00 | u32 | per-target state 0..3 (state machine table 0x143AFB560; 3 = battle with this target; state 2->3 when dist < `BattleStartDist` (+0x48)). A hit or a forced target sets 3 (`0x62BDA0`, `0x62BBE0`) |
| +0x08 | u32 | kind: 1 chr (accessor at +0x28), 2, 3 pos/sound (+0x50), 4 (+0xB0), 5 (+0x80) |
| +0x0C | f32 | forget timer: starts at `SightTargetForgetTime` when not perceived this search tick, cleared when perceived; at 0 the record is cleared |
| +0x10 | f32 | **score**; recomputed each frame only while <= 100 (> 100 = frozen until cleared) |
| +0x14 | f32 | top-protection timer: set to 14 s when the record becomes top (`0x61DA80`), decays |
| +0x18 | f32 | hit timer: 10 s on each hit by this target, decays |
| +0x1C | f32 | hit weight = `targetSys_DmgEffectRate`/100 |
| +0x20 | u8 | perceived/refreshed this tick |
| +0x21 | u8 | is current top (written by `0x629720` every frame) |
| +0x28 | accessor | `SprjChrInsHandleTargetAccessor` (vt 0x2962030): vt+0xA0 handle, vt+0x70 pos, vt+0x18 valid (`0x613840`: entity not dead-state), vt+0xB8 targetPriority (`0x612D70`) |

### 1.3 Peripheral awareness list (the 100-point meter) [VAR]

`TS+0x40..0x48`, 40 B entries `{f32 points, u8 notPerceived, accessor +8, ...}`.
`0x626430` (every update): perception per entry (`0xBD3E40`, `0x6172F0`), then `0x612400`:
`points += rate * dt * mult` when perceived, else `-= 20*dt`; rate = `aroundTargetIncrementPoint`
(NpcThinkParam +0x68), or `IndicationTargetIncrementPoint` (+0x84) while state == 1 caution
(owner vt+0x100 = `0x60E930`); mult from the target's SpEffects (`0xBFC050`, field
`aroundSightPointAddRate` +0x100) and relative position (`0x616F50`). Entries with target HP <= 0
or points <= 0 are dropped (`0x612320`). The best entry with >= 100 points:
not in caution -> IndicationPos slot (`0x6285C0`, forget = +0x80), the AI walks over to look;
in caution -> a NormalEnemy record with reason 7 (`0x629250`). The list is then cleared (`0x626CD0`).
So the meter is the stealth "noticing" gauge, not the in-battle target choice. The upstream
claim "switch at awareness >= 100, highest wins" describes this gauge only.
Bosses have rate 0 (e.g. Genichiro 71000000, Butterfly 50900000, Owl 50600000): they only
get targets from direct perception, sound, damage or events.

## 2. Who adds or changes NormalEnemy records (all writers) [VAR]

Setters: `0x629250(slotList, ChrHandleRef*, reason, owner)` (by handle) and
`0x629430(slotList, idx, ref, reason, owner)` (by index); the reason picks the record template in
`0x62A710` and is stored at node+0x104. A new record starts at score 0; an existing one is
merged (`0x61CE10`: keeps the larger hit timer).

| reason | template | caller and trigger |
|---|---|---|
| 0 | plain (`0x61CFF0`) | `0x6250A0` hostile search every ~0.2 s: candidates `0x62BF30` -> perceived (sight=2/hear=1, `0x6244F0`/`0x617280`, NpcThinkParam eye/ear "perceive") -> `0x62A5E0` -> one record per perceived hostile; non-perceived records start their forget timers (`0x6298B0`, `SightTargetForgetTime` +0x1C) |
| 1 | `0x61D0A0` | `0x61E680(TS, chr)` if team-hostile: `0x60CF10` (EventChrEntity vt slot 43 -> `0x74F130`, entity id resolved by `0x6D5B20`) and ComManipulator::InitAi `0x7E5000` (init field +0xA0) |
| 2 | `0x61D030`: hit timer 10 s, weight = +0x4E/100 | **damage**: ComManipulator vt slot 13 `0x7E4D20(manip, dmg)` -> `0x607540(AiThink, attacker, dmg)` -> `0x61F790(TS, &irq, attackerChr, dmg)`: team-hostile check, `0x624320` (reason 2), and `0x629910` -> `0x61DA40` (state 3, hit timer 10 s) |
| 3 | score = FLT_MAX | not seen called for chr targets [ARV: sound/special] |
| 4 | `0x61D140`: state 3 | `0x624260`: `0x61EE00(TS, handle)` (clears all NormalEnemy first, `0x629830`), `0x61EF10`, `0x6222B0` (Lua `AddFriendAttackedTarget` `0x5E12F0`: a friend's attacker) |
| 5 | score 150, top timer 14 | not seen called [ARV] |
| 6 | | `0x629430` from `0x623CA0` (team-record path) and `0x625420` |
| 7 | plain | peripheral gauge reached 100 while in caution (§1.3) |

The damage path [VAR]: `0x61F790` returns interrupt bits that are OR-ed into AiThink+0x68:
bit 3 `INTERUPT_Damaged_Stranger` = the attacker had **no NormalEnemy record yet**
(`0x6236F0`/`0x629660`), bit 2 `INTERUPT_Damaged` (when dmg+0x164 == 0 and dmg+0xD9 != 2), and
others for dmg+0x1C4 / +0x230. So "Stranger" = "hit by someone I did not know", not "not my
current target" (KB §6.5 Q7). There is **no threshold**: every hostile hit makes a record with
`hitF` 2.0. Whether the attacker becomes `TARGET_ENE_0` depends on the score (§3).

Removal: forget timer reaching 0 (`0x61DB10` -> `0x61DD20`), accessor invalid (target in death
state, `0x613840`), stateInfo 332/333 or 334/335 pairs (`0x61DB10`), `ClearEnemyTarget`.

## 3. Choosing `TARGET_ENE_0` [VAR]

- `0x623240` (current target): a Hallucination/FixedPos override first (`0x62A190`, `0x6284B0`),
  else **the NormalEnemy record with the highest score** (`0x627EE0`), else LowHpFriend/Sound/... .
- `0x629720` (every frame) marks the top record (+0x21) and arms its 14 s protection.
- Score `0x61DE40(rec, ctx)` (ctx from `0x62BA70`: +0x00 own pos, +0x10 think wrapper, +0x28 owner):

  ```c
  f_dist = d <= 1000 ? 1 - d/1000 : 0;                    // 0x62BC30, 1000.0 @0x1432893A4
  f_hit  = rec->hitTimer / 10.0 * rec->hitWeight + 1;     // 0x62BC10, 10.0 @0x1432892A0
  f_top  = rec->topTimer / 14.0 * 0.1 + 1;                // 0x62BC80, 14.0 @0x14296327C, 0.1 @0x1432890CC
  bonus  = (rec->isTop && !TS->replanning) ? 50.0 : 0;    // 0x62BCA0, 50.0 @0x1432892F0, owner vt+0xD8 = 0x60EAC0 reads TS+0x34
  return accessor->targetPriority() + f_dist*f_hit*f_top + bonus;   // vt+0xB8 = 0x612D70
  ```

- "Nearest" is literally a distance term, but it is almost flat (1 % per 10 m).
- `TS+0x34` is written only by `CSIAiReplanningCtrlImpl` vt slot 0 `0x603690`, the only byte
  write to `[reg+0x34]` in the AI code range [VAR scan]. Lua `Replanning()` (`0x5E7610`) sets the
  request at AiThink+0xAE89. That the controller also runs when the goal stack ends or is
  broken by a hit reaction is [ARV]. Only 14 of 234 scripts that test `INTERUPT_Damaged` call
  `Replanning` right after it; most switch through the generic goal end/replan.
- `targetPriority` (`0x612D70`): sum of SpEffectParam +0xF8 over the target's active effects
  whose state flags `& 0x800C0003 == 0`. **No session or multiplayer check** (KB Q3). Vanilla
  rows: only -1 (220690/220695 event hide, 3720100 "Prince hate test", 3720800).

## 4. NpcThinkParam fields used (offsets from Paramdex SDT, confirmed by the accessors)

| off | field | used by | effect |
|---|---|---|---|
| +0x1C | SightTargetForgetTime | `0x1410C6FD0` in `0x6250A0`, `0x623CA0` (LastSightPos), `0x6224E0` | forget time of an unperceived NormalEnemy record |
| +0x48 | BattleStartDist | `0x1410C6A00` in record state `0x62BE40` | per-target state 2 -> 3 |
| +0x4E | targetSys_DmgEffectRate | inline in `0x62A710` case 2 | hit weight /100 |
| +0x68 | aroundTargetIncrementPoint | `0x1410C6960` in `0x612400` | gauge points/s (not caution) |
| +0x80 | IndicationTargetForgetTime | `0x1410C6C60` in `0x626430` | IndicationPos slot time |
| +0x84 | IndicationTargetIncrementPoint | `0x1410C6C80` in `0x612400` | gauge points/s in caution |
| +0x9C | MemoryTargetForgetTime | `0x1410C6CC0` (LastMemoryTargetPos `0x628860`) | memory slot |
| +0x44 | SoundTargetForgetTime | `0x1410C6FF0` in `0x624B70` | sound slots |

The earlier note "battle slot timer = NpcThinkParam +0x80" (RE-CORPSE-TARGET §1) is wrong: +0x80 is
the IndicationPos time; NormalEnemy records use +0x1C.

The accessors read `[wrapper+8]` (a row pointer kept in AiThink+0x28) on every call, so a
runtime patch of the param row in memory takes effect at once, with no respawn needed
(KB Q5). [VAR pointer read, ARV that the pointer targets the shared param file memory]

Values (CSV): `targetSys_DmgEffectRate` = 100 on 769 rows, 0 on 2.
`SightTargetForgetTime`: 15 s (317 rows), 10 (126), 99,999 (95, nearly every boss and mini-boss),
3,600 (69). Examples:

| row | who | DmgRate | SightForget | around/indic pts | eye_perceive | BattleStartDist |
|---|---|---|---|---|---|---|
| 10100000 | ashigaru-type sword | 100 | 15 | 25/25 | 40 | 12 |
| 10200000 | Samurai General | 100 | 3600 | 25/25 | 40 | 15 |
| 50900000 | Lady Butterfly | 100 | 99999 | 0/0 | 140 | 60 |
| 50600000 | Owl (true Shinobi chief) | 100 | 99999 | 0/0 | 120 | 25 |
| 51000000 | Guardian Ape | 100 | 99999 | 10/10 | 40 | 50 |
| 54000000 | Sword Saint (revived) | 100 | 99999 | 0/0 | 120 | 25 |
| 71000000 | Genichiro ("rival") | 100 | 99999 | 0/0 | 120 | 25 |

## 5. Lua interface (884 bindings, `E:\RE\work_ai_re\luareg.txt`) [VAR]

Registrars: `lea rax,wrapper; mov [g],rax; lea rax,fn; mov [g+8],rax; lea r9/r8, name; call [vt+0x58]`.
Nothing that picks a specific target. Target-related bindings:

| name | native | does |
|---|---|---|
| ClearEnemyTarget | 0x5E1E50 | TS vt+0x20 = `0x6228D0(ts,1)`: mark all NormalEnemy dead, remove, **search timer = 0**, so a new search runs on the next update (KB Q8) |
| Replanning | 0x5E7610 | AiThink+0xAE89 = 1 -> replanning ctrl -> TS+0x34 (one frame without stickiness) |
| ShiftCurrTargetBattleState | 0x5E85E0 | TS+0x150 \|= 0x10000 -> `0x61FF40` puts the top record in state 3 (unused by scripts) |
| AddFriendAttackedTarget | 0x5E12F0 | reason-4 record for the attacker of a friend (unused by scripts) |
| GetTopNormalEnemyForgettingTime | 0x5E49F0 | SightTargetForgetTime - top record forget timer (`common_logic_func`) |
| GetCurrTargetType / GetCurrTargetState | 0x5E2A60 / 0x5E2A40 | TS+0x160 / state |
| IsInterupt | 0x5E5CC0 | AiThink+0x68 bit n (`INTERUPT_Damaged`=2, `_Stranger`=3, ai_define.lua) |
| GetDamageLastFrame | 0x5E2A80 | AiThink+0x7454 |
| GetDist, IsBattleState, IsFindState, IsSearchTarget | 0x5E2C10, 0x5E4F40, 0x5E5370, 0x5E6270 | reads |

Script usage (673 decompiled files): `Replanning` in 218 files, `ClearEnemyTarget` in 33
(`table_ai_common.lua`: blood smoke / hide in blood / "enemy turn" SpEffects; `common_logic_func.lua`:
clear when the target is 10 m beyond the home range and the top record has been unseen >= 5 s).
`TARGET_ENE_0` and `TARGET_FRI_0` are the only enemy/friend target constants. [VAR]

## 6. Multi-player arbitration from the DLL

### 6.1 Candidate interception points

**A. Score hook `0x61DE40` (recommended).**
- `float __fastcall score(TargetRecord* rec /*rcx*/, ScoreCtx* ctx /*rdx*/)`, Win64 ABI,
  result in xmm0. Called from `0x61DB10` (record update, from slot vt+8 `0x62D8C0`, from
  `0x62A510` in `0x623CA0`, TS update) every frame per NormalEnemy record whose score <= 100,
  on the game thread in the AI update of the **owning machine only** (NetAI copies do not run
  the TS). Check the prologue bytes at runtime before patching.
- Inputs available: target handle `rec+0x28` accessor vt+0xA0 (or `[rec+0x30]` [G]); our AI:
  `owner = *(ctx+0x28)`, `aiChr = *(owner+0x10)`, `ts = *(*(owner+0x08)+0x7B30)`; `rec+0x20`
  (perceived this tick), `+0x0C` (forget timer: unseen if > 0), `+0x18` (seconds since last hit
  = 10 - value), `+0x21` (is top), `ts+0x34` (replanning frame), `ts+0x158` (state).
- Contract: return `orig + bias`, **bias in [-45, +45]**, and keep the result <= 100 (above
  100 the score freezes and becomes a hard lock). Bias < 50 means a change of mind lands only
  on a replanning frame, when the engine drops the +50 itself. That gives a free
  "never switch mid-combo" without reading goals or animations. To force a switch mid-goal
  (rarely wanted), return a value > orig_top + 50 or use C.
- Side effects: none beyond `TARGET_ENE_0`. No goal reset and no replanning, and the current
  goal finishes against the old target. Only records that exist are scored, so a player the AI
  never perceived (stealth) cannot be chosen.
- Effort: one detour plus a per-enemy state table. Medium-low risk.

**B. SpEffect `targetPriority` (no hook).** It is added to the score for every AI that targets the
bearer, has no multiplayer gate, and the param row is read live. The mod can patch an unused
SpEffect row's +0xF8 at runtime and apply or remove it on a player. Limits: it is global per
bearer, not per enemy; values must stay small (|x| <= 45) or the 100-freeze kicks in; it must
be applied on the owning machine's view of the character (the replica on the other PC)
[ARV: SpEffects on our own player replicate through the native session; check live]. It is good
for "taunt" or "low-profile" items, not for hate juggling.

**C. Hard force `0x60D0A0(ChrIns* aiChr, ChrIns* target)` (ChrIns-level, no entity id).**
Clears all NormalEnemy records (`0x629830`), adds the target with reason 4 (state 3). With target
NULL it sets TS+0x31 (full TS reset next update). If the chr has no Com AI yet
(`0x9F2A80`), it stores the handle at `chr[4]+0x94`, which `ComManipulator::InitAi 0x7E5000`
consumes when the AI is created. Callers: `SprjEzStateTalkEvent` (talk ESD) `0x11C63A0` and
`0x11C94D0`. The other perceived players are re-added within ~0.2 s by the search, then the +50
keeps the forced one. Use for hand-over (§6.4) and for scripted moments, not per frame.
The soft variant `0x61E680(TS*, ChrIns*)` (team-hostility gated, reason 1, no clearing) is what
`0x74F130` (EventChrEntity vt slot 43) calls.
The EMEVD 2004[65] ForceCharacterTarget / 2004[16] ClearCharactersAITarget handlers were not
located (the EMEVD dispatch was not traced). `0x74F130` and `0x60D0A0` are the ChrIns-level
callees that matter.

**D. Damage hook `0x607540(AiThink*, attacker, dmgInfo*)`** (or ComManipulator vt slot 13
`0x7E4D20`): attacker in rdx, damage int at `dmg+0x1E0`. Use it read-only to feed the hate model
(per-attacker damage, posture would need the dmg struct [G]). Do not suppress it: it also raises
the interrupts the scripts use for reactions.

Resets: TS vt+0x18 `0x61E710` (full: slots, state, target type) and TS vt+0x20 `0x6228D0`
(enemies only, immediate re-search) are the "clear AI target" primitives.

### 6.2 Arbitration model (owner machine, deterministic, tunable)

Per enemy E and per player P with a NormalEnemy record in E (that is, E perceives P or was hit by P):

```
threat(P) = wD * dmgShare(P, last T s)          // from hook D, T = 8 s
          + wPo* postureShare(P, last T s)      // [G] needs dmg struct field
          + wN * max(0, 1 - dist(E,P)/Rnear)    // Rnear = 8 m
          + wO * opening(P)                     // P healing / using an item / in a long recovery anim
          + wB * behind(E,P)                    // P outside E's front 120 deg cone
          + wS * min(1, secondsSinceTop(P)/S)   // starvation: P not targeted for S = 20 s
          - wL * (P is current top ? lockDecay : 0)
bias(P) = clamp(K * (threat(P) - mean threat), -45, +45)
```

- Hysteresis: a switch only lands on replan frames (engine +50 rule). Add a DLL cooldown: after a
  switch, raise the new top's bias floor for `minHold` s (trash 4 s, elite 8 s, boss 12 s).
- Commit lock for free: bias < 50 never overrides the +50 mid-goal.
- Presets (scale K, weights): trash = high wN and wD, low wS (they "brawl"); elites = balanced;
  bosses = high wS and wB, moderate wD ("juggles", punishes the healer, turns to the flanker).
  Detect class by NpcThinkParam `SightTargetForgetTime` >= 99999 or the boss SpEffect the scripts
  use (`COMMON_SP_EFFECT_BOSS`).
- Pause arbitration (bias 0) while E is in a scripted event (Lua event requests; `TS+0x31/0x32`
  pending; boss phase transitions per SPEC-BOSS-PHASES) [ARV].
- Determinism: only the owner computes. Inputs come from the owner's own memory, so nothing
  needs syncing; the other PC sees the result through the NetAI copy.

### 6.3 Co-op traps

- **Ownership hand-over** recreates the AI (`0x7E5000`): empty records, state 0, peripheral gauge 0,
  which is the observed stealth-deathblow window. Fix: before taking or giving an enemy, set
  the incoming owner's init force target with `0x60D0A0(chr, lastTargetChr)` (it writes `chr[4]+0x94`
  when no Com AI exists; InitAi applies it as a reason-4 record, state 3). Needs the last target
  sent with the hand-over message. [VAR mechanism, untested]
- **Dead players** (test6 corpse.rs): team 0 on the replica stops the search and hostility check
  for new records. Existing records die when the accessor reports death state or HP <= 0. Records
  survive `SightTargetForgetTime` (99,999 s on bosses) only while the accessor stays valid. The
  score hook should return -45 for a dead or hidden player anyway.
- **Friendly fire team 28**: enemy teams 6/7/13/24 stay hostile to 28 (matrix), so records are
  created as for team 2. The hostility check in `0x61F790` / `0x61E680` uses the same matrix. [VAR]
- **Stealth**: the score hook only ever sees existing records, so it never reveals a hidden player.
  Do not use C on a player E has no record for.
- **Friend's hits on the owner's AI**: whether the owner's `0x7E4D20` receives the friend's
  replica as attacker when the friend hits his copy is not known. The user's "switches on hit"
  suggests yes. Log it.
- **Freeze above 100**: any bias that pushes a score > 100 locks that target until the record is
  cleared. Clamp.

## 7. Quick wins (runtime patches, no hooks)

| what | where | effect | risk |
|---|---|---|---|
| Top-protection duration 14 s | f32 @ `0x14296327C` (only 2 users: `0x61DA80`, `0x61DE40`) | e.g. 5 s: the nearest/attacker wins at the next replan sooner; more switching | low (.rdata, VirtualProtect) |
| `targetSys_DmgEffectRate` | NpcThinkParam +0x4E (u16), live | 300 = a hit counts 4x for 10 s (still only on replans); 0 = hits ignored, pure distance | low |
| `SightTargetForgetTime` for bosses | +0x1C | e.g. 99999 -> 20 s: a player who breaks line of sight drops out, so the boss turns to the visible one | low; changes stealth/boss leash |
| targetPriority on players | SpEffect +0xF8 of a spare row, applied per player | global taunt / low profile, ±45 max | low-medium (replication) |
| +50 stickiness | 50.0 @ `0x1432892F0` | do NOT patch: shared by 31 sites | - |

## 8. Answers to `kb/ai-targeting.md` §6.5

1. Formula: §3 (score) and §1.3 (gauge: points/s from +0x68/+0x84, -20/s decay, no damage term).
   Damage enters only the score (hitF), with no distance falloff beyond 1000 m; the +50 hold is the bonus.
2. Damage enters through `0x7E4D20` -> `0x607540` -> `0x61F790` (engine), not through Lua. No
   "last attacker = target" rule exists; the attacker wins by score at the next replan.
3. `targetPriority` is read in `0x612D70`, with no multiplayer gate.
4. EMEVD handlers not located. ChrIns-level callees: `0x60D0A0` (hard, clears) and `0x61E680` (soft).
   Neither has a timer; a forced record lives like any other (forget timer, death).
5. Live row pointer (AiThink+0x28 wrapper), so patches apply at once.
6. ShiftCurrTargetBattleState / AddFriendAttackedTarget: §5. MemoryRelativeTarget and HasLastTarget
   not decompiled. None of them can name an arbitrary chr.
7. Damaged_Stranger = hit by a chr with no NormalEnemy record.
8. ClearEnemyTarget: immediate re-search (search timer 0).
9. Same hostility matrix `0xC09660` for candidates and the damage path. The score has no team term,
   and `chr+0x1A15` only gates candidacy.

On the KB's hybrid design: agreed in spirit, with three corrections. (a) The main lever should be
the score hook (A), not targetPriority: targetPriority is per bearer and freezes above 100.
(b) The "ForceCharacterTarget callee" is `0x60D0A0`; use it for hand-over and scripted moments,
not as the routine switch, because it wipes other records. (c) No special pause is needed for
mid-attack: the engine's replan-only +50 drop already provides it.

## 9. Not verified / needs a live test

1. Replanning controller `0x603690` runs on goal end / hit-reaction break (not only on Lua `Replanning`). Log `TS+0x34` frames per enemy.
2. Record layout offsets +0x0C..+0x28 at runtime (read one boss's slot list: head `ts+0x18`, node+0x20).
3. The owner's `0x7E4D20` sees the friend's replica as attacker.
4. The NpcThinkParam row pointer in AiThink+0x28 points into the shared param memory (patch one row, observe).
5. SpEffect targetPriority on a replica: apply on the owner side, check the score.
6. `0x60D0A0` before InitAi (chr[4]+0x94) on hand-over.
7. EMEVD 2004[65]/[16] handler addresses (not traced).
8. Reasons 3/5/6 producers (`0x625420` has no static callers; vtable or jump target).
