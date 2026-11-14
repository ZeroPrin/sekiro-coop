# Sekiro netcode gap map: what the engine already does for enemy ownership (2026-11-14)

Read-only RE on `sekiro.exe` 1.06 (process dump `E:\RE\dumps\sekiro-1.06-ingame-2026-11-11.dmp`, carved
image `E:\RE\carve\sekiro_mem.exe`). Ghidra project copy `E:\RE\ghidra_net\sekiro_quick` (functions are
created and decompiled on demand; the full auto-analysis of `ghidra_full` had died with an OOM, so it holds
nothing). Scripts and outputs: `E:\RE\work_net\` (`run.py` runner, `j_decf.py` decompile to `f\f_<rva>.c`,
`j_disp.py` disp32 operand scanner, `j_pk.py` packet-type scanner, `xref.py` / `riprefs.py` byte-scan
xrefs, `realfn.py` .pdata chunk -> owning function, `rd1.py` heap reads from the dump). All RVAs are
relative to `0x140000000`.

Tags: **[C]** confirmed in disassembly/decompilation this pass; **[I]** inferred from code that was read;
**[U]** unknown / needs a live check or a DS3 reference.

> Dump caveat [C]: the dump was taken with the mod loaded at the title screen. Its entry detours are in the
> image (`E9 -> 0x13FCxxxxx` + `CC` padding) at `0xA4A650`, `0x8E7210`, `0x8E7310`, `0x8E6FA0`,
> `0x8E6B50`, `0x8E6D20`, `0x8D77B0`, `0x8D7320`, `0x8D7290`, `0x8D7D70`, `0x8D8260`, `0x8D8750`,
> `0xBE3810`, `0x6D7070`, `0xBD64E0`, `0x9F6270`, `0x9FFF50`, `0xA8F7E0` and ~25 more (UI, items). The
> call-site patches of the native session were **not** applied (e.g. `0xA4DC38` still `E8 23 62 E9 FF`).
> Decompiling a detoured function needs its original prologue restored in a private project.

---

## 0. Summary of the answers

| Q | Answer in one line | Confidence |
|---|---|---|
| Q1 | Only one function reads the native ownership array without `0xA4A650`: `0xA4A680` (IsOwnedHere by chr), used once by the chr update `0xA02A10` (NPC SpEffect-equip/area-effect bookkeeping). Every AI/sync/damage decision goes through `0xA4A650`. But three **other** inputs decide Com vs NetAI or local behaviour without asking ownership: the chr-loop force-local predicate `0x9F33F0`, the EnemyIns byte `chr+0x2040`, and the unfaked **global** IsHost/IsClient (`0x8E4790` / `0x8E44C0`) | C |
| Q2 | 2004[28] = `ChrCmd_SetNetworkUpdateValue(entity, value)` -> EventChrEntity slot 56 (`0x750760`) -> **`chr+0x1A1C = value`**. It is a bias added to the machine's type-22 priority and makes the chr a candidate even when its LOD is far. Not `chr+0x2040`, not `0x9F33F0`. Pinning a boss natively = write 4095 to `chr+0x1A1C` **on the host only** | C (mechanism), I (that DS3's "Forced" works the same) |
| Q3 | The candidate filter has **no ownership or manipulator term**: it is LOD/region relevance on the local machine. "NetAI here -> never a candidate" (§31) is not what the code does. Nothing in the 22/23 path is stubbed. Natively, an enemy relevant to nobody's LOD (roughly > 20 m and not visible / not in the player's area mask, or in a non-normal chr-set state) has no owner by design | C (filter), I (live cause of §31) |
| Q4 | No native owner-death or pause handling. The candidate builder never looks at the local player's HP; a paused world update stops the whole table. Disconnect is handled implicitly: the host prunes a record whose player chr handle disappeared (`0xA1D550`) and the next 23 redistributes | C |
| Q5 | Periods are fields of the sync table `T`, loaded from immediates in the T ctor `0xA1A100`: 23 = `T+0x398` 3.0 s, 22 = `T+0x3A0` 5.0 s, type 4 = `T+0x3B4` 0.25 s (8 records/packet `T+0x3BC`), type 5 = `T+0x3D0` 0.5 s. Writable at run time (one float per field). No hysteresis, no hold | C |
| Q6 | On a switch to Com, ComManipulator slot 6 (`0x7E54D0`) **wipes** the AI's TargetingSystem records and target slots 0/1. The replicated target (behaviour record `+0x148/+0x14C`) is kept only inside the NetAIManipulator (`manip+0x420` mode, `+0x440` netId, `+0x430` pos). No native path seeds the new owner's AI from it | C |
| Q7 | ~60 P2P types located (table §7). Everything on the enemy path is present and real (no stubs found). The mod already routes the core set; notable unrouted siblings: 48 / 56 (same consumer as throw 18), 31 (sent from the chr update), 41 / 55 (join snapshot), 29, 47 | C (sites), I (meanings) |

---

## 1. Q1: reads of ownership that bypass `0xA4A650`

### 1.1 The ownership store [C]

- `T = *(WCM+0x630)`; block `B = T+0x10 + 8*(netId>>11)`; `B+0x18` = `i16[]`. Values are only **-1**
  (reset, `0xA19A50`) or **1** (`0xA19B40`); there is no slot index in it.
- Readers of the array: `0xA19B10` (`own[i] >= 0`) only, called only from `0xA1BB10`, which is called only by
  **`0xA4A650`** (`IsOwnedHere(WCM, netId)`) and **`0xA4A680`** (`IsOwnedHere(WCM, chr)`).
- Writers: `0xA19A50` (whole block -1; callers `0xA19850` block ctor, `0xA1B672` = `0xA1B660` all blocks,
  `0xA1DD00` twice), `0xA19B40` (set 1; callers `0xA1C680`, `0xA1DD00` twice).
- `0xA1B660` (reset all) + `0xA1C680` (set) are called by the candidate builder `0xA4E520` **in its
  offline branch** (member count < 2): every frame it marks every active enemy owned here. With the mod's
  count patch (`0xA4E5BE/C5`) that branch never runs.

### 1.2 `0xA4A680` is the one direct bypass [C]

`0xA4A680(WCM, chr)`: returns 0 unless `0x9F34B0(chr)` (`chr+0x1A14 & 0x10` or `0x9F2D50(chr)`); returns 1
if `chr+0x2040` (slot 175) or member count `<= 0` (site `0xA4A6CB`, patched to 2 by the mod); else
`0xA1BB10(T, chr+0x78)` **directly**. Only caller: `0xA02A10` (ChrIns/EnemyIns vtable slot 44, the per-frame
chr update): when "owned here" (or the main player) it runs the NPC SpEffect-equip / area-effect bookkeeping
on `chr[0x23A]` (`chr+0x1A34`, `chr+0x1A38`).
Under test11 the native array is still written by the native 22/23, so this one path follows the
**native** owner, not the host table. Impact: small (area SpEffect state on the copy); see §8 item 6.

### 1.3 Every caller of `0xA4A650` [C] and what it decides

| Caller (function) | Site | Decides | Consistent with a "not owned" detour? |
|---|---|---|---|
| `0xA14B50` EnemyIns slot 37 | `0xA14BBD` | Com vs NetAI (with forceLocal and slot 175) | Yes, except forceLocal / `+0x2040` (1.4) |
| `0xA2F260` PlayerIns / ReplayGhostIns slot 37 | `0xA2F2FD` | Com vs NetAI for PlayerIns whose slot 54 is true | Yes |
| `0xA1C100` type-4 sender | `0xA1C303` | send a movement record (also needs flag `0x02`, written only by Com slot 11) | Yes |
| `0xB6F4C0` damage class | 4 sites | classes 3/4/5/6 (owned here / elsewhere) for the router table | Yes |
| `0xB6A040` damage router (no-attacker path) | `0xB6A1BF` | apply locally vs remote / send 21 | Yes, but see global IsHost/IsClient (1.4) |
| `0xB97B30` (bullet/attack hit, calls `0x8DBA20` = 21 to host) | `0xB97C19` | local vs "send to host" | Yes |
| `0x9E9D10` AddSpEffect, `0x9F54A0` / `0xA01720` (variants) | `0x9E9EC9`, `0x9F565F`, `0xA018DF` | apply a SpEffect on a networked chr (`chr+0x1A14&0x10` or `0x9F2D50`) only when self-applied and owned here; non-networked chrs use the `WCM+0x3130` list instead | Yes; the branch also asks global IsClient |
| `0xA00A10` | `0xA00A7D` | "not networked, or not owned here" predicate (caller `0x9F736D`) | Yes |
| `0x9F6320` set position (26 callers incl. warps) | `0x9F6375` | owned here -> `chr+0x1B94 = 1.0` | Yes |
| `0x9F8100` ChrIns/EnemyIns slot 211 (teleport) | `0x9F81BA` | same `+0x1B94` | Yes |
| `0x9F5820` attach / ride | `0x9F5C06` | owned here -> `chr+0x1B98 = 1` | Yes |
| `0xA02120` (ground snap, caller `0x7DE620`) | `0xA02517` | NOT owned and `chr+0x1BC4 > 0` -> raycast, snap to ground, force interval 0 | Yes (intended for NetAI copies) |
| `0x9EB0A0` slot 230 (debug draw) | `0x9EB632` | colour of the debug overlay | n/a |
| `0xA13080` (debug menu text) | `0xA133FE` | text colour of the NetAI target line | n/a |

No other function references the block array (`xref` of `0xA19B10`, `0xA1BB10`, `0xA19B40`, `0xA19A50`
is complete as listed; a disp32 scan for `+0x630` found no other `T` walkers that read `B+0x18`).

### 1.4 Deciders that do not ask ownership at all [C]

1. **Chr-loop force-local, `0xA4DB20`.** `slot37(chr, forceLocal = (members < 2) || 0x9F33F0(chr))`, only
   for chr sets with index `< WCM+0xC0` (4 in the dump). `0x9F33F0(chr)` = `0x9E5C00(U)` (U = `chr+0x18`,
   chr-set entry; `*(U+0x30)+0x8C` bit 0) **or** `U+0x20 & 2` **or** `(**(chr+0x1FF8))+0x54 & 1`.
   The same predicate makes ComManipulator slot 11 (`0x7E54F0`) skip writing records, so a force-local
   enemy runs its own AI on every machine and sends nothing. Natively `U+0x20 bit 1` is set by the death
   handler `0x9F4130` **only when global IsClient**: in DS3 a corpse on a guest runs locally. The other two
   inputs are [U].
2. **`chr+0x2040` (EnemyIns slot 174 setter `0xA14A20`, slot 175 getter `0xA13D50`).** Zeroed in the
   EnemyIns ctor (`0xA11FE6`); **no static caller of slot 174** (no `call/jmp [reg+0x570]`, no load of
   `[reg+0x570]` into a register; the only data ref is the vtable). Readers: slot 37 (force Com),
   `0xA4E520` (excluded from candidates), `0x7E54F0` (no record writes), `0xA4A680` (owned), `0xB6A040`.
   So in vanilla it is a dormant "force local" switch. **It is not set by `0x9F33F0`** (the brief's premise).
3. **Global role, `0x8E4790` IsHost (`S+0x184 == 3`) and `0x8E44C0` IsClient (`== 6`).** The mod fakes call
   sites, not these globals, so both read false on both machines. Chr-relevant callers: `0xB6A040`
   (router action 2: "not IsHost -> send 21 to the host"), `0x9E9D10` / `0x9F54A0` / `0xA01720`
   (SpEffect gate), `0x9F4130` (corpse force-local, above), `0x9ACE90`, `0x9B2D00`, `0x8DA220` (join
   snapshot sender), `0x69D310` / `0x68AA80` (event script death branch; SPEC §5). Each of these behaves as
   offline/host on both machines.
4. **Event scripts (EMEVD) and HKS.** EMEVD runs on each machine for its own world. The bank-2004 handlers
   read here (28, 34, 16, 65) go straight from the instruction to `EventEntityCommandUtil::ChrCmd_*`
   functors over the entity group (`0x756AE0`) with no ownership test. On a NetAI copy their effect is
   overwritten by the owner's records; on the owner it is real. Lua AI (AiThink) runs only inside
   ComManipulator, so it follows slot 37.
5. **HP is host-authoritative, not owner-authoritative.** `0xA4B830` writes every enemy's HP into the type-5
   table on the host (`IsHost` site `0xA4B8A4`) and applies it on the client (`IsClient` `0xA4BA81`),
   independent of who owns the enemy. Client-owned damage reaches the host as 21.

**Verdict for test11 [I].** Answering "not owned" at `0xA4A650` is enough for AI, record sending, damage
classes, SpEffects and the position flags. It is **not** enough for: force-local chrs (`0x9F33F0` true or
`+0x2040` set: run on every machine), the `0xA4A680` area-effect path, and anything keyed on global
IsHost/IsClient. The DISOBEY detector should log `0x9F33F0(chr)` (U flags, `0x9E5C00(U)`, module0 `+0x54`),
not only `+0x2040`.

---

## 2. Q2: 2004[28] and 2004[34] in Sekiro

### 2.1 Dispatch [C]

- EMEVD bank switch `0x900D10`: bank 2003 -> `0x904EF0`, **bank 2004 -> `0x9017D0`**, bank 4 (chr
  conditions) -> `0x903C20`, bank 2000 -> `0x90D0C0`, ...
- Bank 2004 instruction 28 (`case 0x1C`): `0x67B4C0(EventMan 0x143D55600, entity, value)` -> functor
  `SetNetworkUpdateValueCmd` (RTTI: `ChrCmd_SetNetworkUpdateValue@EventEntityCommandUtil`, vt `0x29AC7F8`)
  -> `0x756AE0` (for each chr of the entity/group) -> functor slot 0 `0x754E10` -> **EventChrEntity
  slot 56 (`+0x1C0`) = `0x750760`: `rcx = [rcx+0x10]` (chr), `jmp 0x9F4340` = `mov [chr+0x1A1C], edx`**.
- Bank 2004 instruction 34 (`case 0x22`): `0x6763E0(EventMan, entity, bool fixed, sbyte level)` -> functor
  `ForceSetOmissionLevelCmd` (`ChrCmd_ForceSetOmissionLevel`, vt `0x29AC838`) -> slot 0 `0x7543E0` ->
  EventChrEntity slot 60 = `0x74FAE0`: if `chr+0x1FF0 == -2` (no debug override): `chr+0x1A10` bit 0 =
  fixed, **`chr+0x110` = level** (the update interval / omission level).
- Also located: 2004[65] -> `0x674640` (`ChrCmd_AddTarget`, functor `SetEventTargetCmd` vt `0x29AC738`),
  2004[16] -> `0x675BE0` (`ChrCmd_ClearTarget`, vt `0x29AC788`). Both are real handlers.
- Neither 28 nor 34 is a stub. The handlers are fully reachable; Sekiro's own events only never emit 28.

### 2.2 What `chr+0x1A1C` does [C]

- ChrIns ctor `0x9E7A60` writes `qword [chr+0x1A18] = 0x3F800000` (f32 1.0 at `+0x1A18`, **`+0x1A1C = 0`**).
- Writers: only `0x9F4340` (via 2004[28]) and the debug menu `0x9FA430` case `0x13C` (range 0..0xFFFF).
- Readers: only two.
  - Candidate gate in `0xA4E520` (`0xA4E81B`): `chr+0x1A1C > 0` makes an enemy a candidate even when its
    LOD says "far" (see §3.2).
  - Priority `0x9F0230` (`0x9F03B3`): `prio += chr+0x1A1C` (clamped to `0..0x7FFF`).
- So it is a per-machine **authority bias**, the analogue of ER's `ChrIns.network_authority`. "Forced = 4095"
  on one machine beats any distance priority on the other (max natural prio is about `6*256 + 255 +
  chr+0x1A20` = ~1791), and the host resolves the conflict for the biased machine at every 23.
- Set on **both** machines it cancels out (both get +4095; ties go to the member with fewer candidates).
  That is why DS3/ER gate the call with "host world only".
- `chr+0x1A20` is a second additive term (zeroed by `0x9F4350`, called from `0x8057C5`); source of non-zero
  values [U].

### 2.3 What 2004[34] does [C]

`chr+0x110` = interval (0 every frame, 2, 5, 30 = every n-th frame, -1 / -2 = not updated), `chr+0x1A10`
bit 0 = "fixed": the LOD controller `0xA4E8E0` skips fixed chrs. Sekiro's events already use 34 on most maps
(`docs/kb/ds3-netcode.md:261`). Interval 0 also gives LOD level 6 (the highest base priority, §3.3).

### 2.4 Interaction with 22/23 and `0xA4A650` [C mechanism, I outcome]

- Host only, `chr+0x1A1C = 4095`: the host's own record lists the boss with prio >= 4095; a client's 22 with
  the boss loses in `0xA1C590`; the host's local apply marks it owned; the client's 23 omits it, so the
  client's array is -1 and `0xA4A650` answers 0 there. Every 23 cycle re-confirms it, so nothing drifts.
- It does **not** follow the host's death: the bias stays while the host's PlayerIns exists (§4).
- **What each machine would need to do:** host: write `chr+0x1A1C = 4095` (directly, or call
  `0x67B4C0(EventMan, entityId, 4095)` on the game thread) when the boss activates; clear it to 0 when the
  host is absent (dead/spectating/paused) so the client's own distance prio can win, and re-set on return.
  Client: nothing (keep 0). In test11 the detour ignores the native answer, so this only makes the native
  layer **agree** with the table (useful for the `0xA4A680` path and for diagnostics); it would be the whole
  mechanism only if the detour deferred to the native answer for bosses.

---

## 3. Q3: candidates, priorities, and the ownerless state

### 3.1 Who builds what [C]

- Update order in WorldChrManImp's update task `0xA509F0`: ... `0xA4E8E0` (LOD / update interval) ->
  ... `0xA4F940` (peer damage) -> **`0xA4E520`** (candidates, then `0xA1CED0` = 22) -> **`0xA1D2D0(T, dt,
  playerBlock)`** (timers, `0xA1DD00` = 23, join snapshot `0xA1E140`/`0xA1E9F0`, 4/70/5/71 send and
  receive) -> tail-jump to the chr loop **`0xA4DB20`** (slot 37 for every chr).
- `0xA4E520` with member count >= 2 iterates chr sets `0 .. WCM+0x1C8-2` and calls `0xA1ABC0(T, netId,
  prio)` (append `netId | prio<<16` to `T+0x380..0x388`) when **all** hold:
  1. `netId != 0xFFFF` and slot 175 (`chr+0x2040`) is 0;
  2. relevance: `U.flags(+0x20)` bit 0 set, or (bit 1 set and `!0x9E5C00(U)`), or interval `chr+0x110`
     in **-1..4**, or `chr+0x1A10 & 8` (area mask hit, below), or `chr+0x1A1C > 0`;
  3. `U+0x14 == 4` and `U+0x10 == 4` (chr-set entry loaded/active);
  4. `prio = 0x9F0230(chr) != 0`.
- **There is no ownership, NetAI or `chr+0x58` term anywhere in the gate**, and none in the LOD controller
  that feeds it (`0xA4E8E0` reads distance, budgets, draw/visibility bits `0x9F2F60`/`0x9F1C60`, area masks;
  [C] no `0xA4A650`, no manipulator).

### 3.2 LOD inputs (values from the dump's config object `*(0x143D7A388)`) [C code, values from one dump]

- Within 10 m (`100.0` squared): nearest **10** chrs -> interval 0 (`cfg+0x2C`), next **15** -> 2
  (`cfg+0x30`), others 5.
- Beyond: chrs flagged visible (`chr+0x1A10 & 0x10`, set by a frustum/occlusion test) get 2/5 out to
  ~50 m (`cfg+0x3C`), others 30; the 10/20/40 m ring adjustments (`cfg+0x40/0x44/0x48`) push 20-40 m to 5
  and > 40 m to 30.
- `chr+0x1A10 & 8` = "this chr's 256-bit area mask (`chr+0x1A68`) intersects the chr set's / player's mask"
  (`0x85D8C0` AND of two masks). Any hit makes the enemy a candidate regardless of interval.
- -1 = the chr's set entry is not in the normal state (U flags) and not fixed; -2 = outside the LOD list.

### 3.3 Priority `0x9F0230` [C]

```
if (U normal && interval < -1) || 0x9E5C00(U): return 0           // not a candidate
level = interval -1 -> 5 ; 0 -> 6 ; 2 -> 4 ; 5 with (0x1A10 & 8) -> 4 ; anything else -> 0
        (+1 already included; chr+0x70 == 4 forces level 1)
prio  = level*256 + 255*(1 - min(dist(localPlayer, chr)/64 m, 1)) + chr+0x1A20 + chr+0x1A1C
chr+0x1FC8 = clamp(prio, 0, 0x7FFF); return it (0 if negative)
```
Constants: `0x143B06354` = 64.0, `0x14328919C` = 1.0, `0x143289490` = -255.0. The level (the local LOD tier)
dominates; distance only orders within a tier.

### 3.4 Resolution [C]

`0xA1D550`: drop records whose player handle no longer resolves (`0xA4A050`); then for every pair of records
`0xA1C590(a, b)`: for each netId in both, if `b.prio != 0`: remove from `a` when `a.prio < b.prio`, or on a
tie when `b.count <= a.count`; else remove from `b`. **If `b.prio == 0` both keep it** (the quirk). Because
`0xA4E520` never appends prio 0 (`test eax,eax; jz` at `0xA4E83C`), the quirk is **unreachable natively**;
only a foreign 22 with a 0 entry could trigger it.

### 3.5 First owner of a freshly loaded enemy [C]

New block -> `0xA19850` -> all entries -1. With the count patched to 2, the chr loop's `forceLocal` is false,
so slot 37 picks **NetAI on both host and guest** until a 23 (host: its own next distribution, <= 3 s;
guest: its 22 every 5 s + the host's 23 every 3 s, so <= 8 s). In DS3's design the first owner comes from
the candidate lists: every machine nominates every **relevant** enemy whatever its current owner, so
relevance on any machine is enough.

### 3.6 Verdict on §31's "~200 ownerless" [I]

- The §31 root cause ("built only from enemies updated here; NetAI here never becomes a candidate") does not
  match the code: interval -1 is inside the accepted range, and nothing asks for Com.
- What does leave an enemy without an owner natively:
  - (a) it is relevant to nobody: LOD 5/30 on every machine and no area-mask hit. These are roughly the
    enemies > 20 m from both players that are not on screen. **By design** they stay NetAI with no data
    (frozen). In DS3 that is acceptable because phantoms stay near the host.
  - (b) its chr-set entry is not 4/4, or `0x9E5C00(U)` is set (prio 0);
  - (c) the 5 s + 3 s pipeline after load or after a move.
- The 200 of §31 were mostly (a): "3 of them within 40 m". The near ones fit (b) or (c).
- Nothing in Sekiro is missing or stubbed in this path.
- Mod-side contributors at the time (all since removed or changed):
  - engaging with a frozen table (§18 live 7.1);
  - the client's 5 s No23 rule and the delayed chr-loop patch;
  - feeding the friend's 22 only while the native remote handle exists (`0xA1D550` drops the record
    otherwise);
  - `activation.rs` changing intervals.
- Without the mod's patches the engine would treat every enemy as owned locally (offline branch), which is
  "two AIs" rather than "no AI".

---

## 4. Q4: owner dies, leaves or pauses [C]

- **Death.** `0xA4E520` only requires a main player chr (`WCM+0x88 != 0` or `cfg+0x160`). It never reads
  HP or death state, and the prio is the distance from the (dead) body. The host's own record is keyed by
  its PlayerIns handle (`0xA1CED0`), which survives death until the reload. So a dead host keeps winning
  its candidates. This is the native reason for v8.57's `game_owned=1`.
  - In DS3 a host's death ends the session, and a dead phantom's PlayerIns is destroyed, so no
    "owner died" rule was ever needed [I].
- **Pause.** `0xA509F0` does not run while the world is paused, so the timers, candidates and 23 all stop.
  The paused owner keeps what it owns.
- **Leave / disconnect.** `0xA1D550` deletes a record whose handle no longer resolves; the next 23 (<= 3 s)
  hands its enemies to whoever else nominated them. Member join/leave handlers `0xA1AC70` / `0xA1AD80`
  (host) only queue join-snapshot work (`T+0x3F0` / `T+0x408`, consumed by `0xA1E140` (41) /
  `0xA1E9F0` (55)); no reassignment code exists. When the session ends (count < 2), `0xA4E520` takes the
  offline branch and marks everything local, and the chr loop forces Com.

---

## 5. Q5: timers and hold [C]

| Field of `T` | Init (`0xA1A100`) | Meaning |
|---|---|---|
| `+0x398` / `+0x39C` | 3.0 / 3.0 | 23 period (host) / countdown |
| `+0x3A0` / `+0x3A4` | 5.0 / 5.0 | 22 period (client) / countdown |
| `+0x3A8`, `+0x3AC`, `+0x3B0` | -1, 0, 0 | type-4 block cursor (the local player's block), entry cursor, "round due" |
| `+0x3B4` / `+0x3B8` | 0.25 / 0.25 | type-4 round period / countdown |
| `+0x3BC` | 8 | movement records per type-4 packet |
| `+0x3C4..+0x3CC` | -1, 0, 0 | type-70 cursors |
| `+0x3D0` / `+0x3D4` | 0.5 / 0.5 | type-5 full-rotation period / countdown |
| `+0x3D8` | 5 | [U] (type-5 entries per round?) |
| `+0x3DC`, `+0x3E0`, `+0x3E4` | -1, 0, 10 | type-71 cursors / count [I] |

- All periods are reloaded from these fields every cycle (`0xA1DD00`: `+0x39C = +0x398`), so one float
  write per field after the WCM exists retunes them for the whole game.
- `0xA1D2D0` decrements `+0x39C`/`+0x3A4` by `dt`.
- 4/70/71 are sent **only for the sender's current block** (`param_3` of `0xA1D2D0`).
- No hysteresis or hold exists: each 23 recomputes from the latest lists. The only stickiness is that LOD
  tiers change in steps (10 / 15 budgets, rings).

---

## 6. Q6: state on hand-over [C]

- Slot 37 switch: the old NetAI gets slot 31 (`0x7E3CF0`, empty); the new manipulator gets slot 6 with the
  current position and rotation; then `0x7DE3D0` rebinds ChrCtrl.
- **Com slot 6 = `0x7E54D0`** with `ai = Com+0x340` (AiThink, created once by InitAi slot 3 `0x7E5000`, not
  re-created) and `ts = ai+0x7B30` (TargetingSystem):
  - `0x62B930(ts+0x10)` frees every target record;
  - `ts+0x28 = 0`;
  - `0x630C50(slots, 0, null)` and `0x61E470(slots+0x140, empty)` clear target slots 0 and 1.
  - Goal and Lua state are not reset here [I].
  - So the "fresh AI" after a take is **a native wipe at the switch**, not InitAi (correcting
    `RE-AI-TARGET.md` §6.3).
- **NetAI slot 6 = `0x7EA400`:** sets the interpolation targets `+0x350`/`+0x360` to the current pose and
  zeroes the velocities and the two action objects.
- **Replicated target:** NetAI's tick `0x7EA5C0` copies behaviour record `+0x148` (mode) to `manip+0x420`,
  and `+0x14C` to `manip+0x440` (u16 netId, mode 1) or `manip+0x430` (position, mode 2; `+0x43C = 1.0`).
  It is used only for NetAI turning. No code copies it into ComManipulator/AiThink.
- **Seeding primitives that exist** (none is called on hand-over natively):
  - `0x630C50(slots, 0, chr)` sets target slot 0 to a chr;
  - `0x60D0A0(chr, target)`, the hard force (`RE-AI-TARGET.md` §6.1C);
  - EMEVD 2004[65] `ChrCmd_AddTarget` `0x674640(EventMan, chrEntity, targetEntity)`, the soft add through
    EventChrEntity.
  - They must run **after** `0x7E54D0`, i.e. after the switch frame, or they are wiped.

---

## 7. Q7: gap map

### 7.1 Enemy-sync functions (DS3 lineage)

Status column: **R** present and reachable in our session; **N** present, never called in our session; **S**
stub; **A** absent.

| Function | RVA | Role | Status / note |
|---|---|---|---|
| Sync table ctor | `0xA1A100` | T, timers (§5) | R |
| Block add / ctor | `0xA1BB50`, `0xA19850` | per map block arrays | R |
| Candidate builder | `0xA4E520` | §3 | R (session branch only with the count patch) |
| Network priority | `0x9F0230` | §3.3 | R |
| LOD / update interval | `0xA4E8E0` + setter `0x9F6270` | feeds candidacy | R (setter detoured by `activation.rs`) |
| 22 send / host collect | `0xA1CED0` | | R (host member loop replaced by our feed) |
| Record find / set list | `0xA1D7E0`, `0xA1C6C0` | | R |
| Resolve | `0xA1D550`, `0xA1C590` | | R |
| 23 send / receive | `0xA1DD00`, `0xA1D240`, `0xA29DD0` | | R |
| Ownership query | `0xA4A650` | | R (test11: answers from the host table) |
| Ownership query by chr | `0xA4A680` | | R, **bypass** (§1.2) |
| Manipulator select | `0xA14B50` (EnemyIns 37), `0xA2F260` (PlayerIns 37) | | R |
| Force-local predicate | `0x9F33F0` | | R (independent of ownership) |
| Force-local byte | `chr+0x2040` via `0xA14A20`/`0xA13D50` | | N (no caller of the setter) |
| Authority bias | `chr+0x1A1C` via 2004[28] `0x67B4C0` -> `0x750760` -> `0x9F4340` | | N in vanilla Sekiro (no event emits it); callable |
| Fixed update rate | 2004[34] `0x6763E0` -> `0x74FAE0` | | R (vanilla events use it) |
| Com record writer | Com slot 11 `0x7E54F0` | 4/70/71 records + dirty lists | R |
| NetAI apply | NetAI slot 9 `0x7EA5C0`, `0x7EB2A0` | | R |
| 4 / 70 / 71 send | `0xA1C100` / `0xA1B320` / `0xA1DAC0` | | R |
| 4 / 70 / 71 recv | `0xA1BDE0` / `0xA1AF40` / `0xA1D8D0` | | R (fake member vector) |
| 5 HP send / recv / apply | `0xA1C9C0`, `0xA1CC70` / `0xA1C860` / `0xA4B830` | host-authoritative HP | R |
| Join snapshot | `0xA1E140` (41), `0xA1E9F0` (55), queues `0xA1AC70` / `0xA1AD80` | | N (needs a real member join) |
| Owner-death / pause handling | none | | A |
| Hysteresis / hold | none | | A |
| AI-target seeding on take | none | | A (primitives exist, §6) |

### 7.2 P2P packet types located in this pass

Senders go through `0x8E7210` (broadcast), `0x8E7310` (to host) or `0x8E6FA0` (to one member);
receivers through `0x8E6B50` (from a member) or `0x8E6D20` (from the host). Type numbers were taken from the
immediate or from `lea edx,[r9+k]` in the wrapper ([C]). Meanings are [I] unless marked. "Mod" = what
nsession routes today (SPEC §2.4, IMPLEMENTATION-NOTES §18/18b).

| Type | Producer (wrapper <- caller) | Consumer (wrapper <- caller) | Meaning | Mod |
|---|---|---|---|---|
| 4 | `0xA1C100` | `0xA1BDE0` | enemy movement, 8/packet [C] | routed |
| 5 | `0xA1C9C0`, `0xA1CC70` (host) | `0xA1C860` <- `0xA1D2D0` | enemy HP [C] | routed |
| 22 | `0xA1CED0` -> to host | `0xA1CED0` (host loop) | ownership candidates [C] | routed |
| 23 | `0xA1DD00` -> to member | `0xA1DD00` (client) | owner list [C] | routed (liveness only in test11) |
| 70 | `0xA1B320` | `0xA1AF40` | behaviour sync [C] | routed |
| 71 | `0xA1DAC0` | `0xA1D8D0` | AI action inputs [C] | routed |
| 41 | `0xA1E140` -> to member | `0xA1E140` (from host) | join snapshot | not routed |
| 55 | `0xA1E9F0` (bcast + to member) | `0xA1E9F0` (from host) | join snapshot | not routed |
| 21 | `0x8DA200` <- `0xB6A040`; `0x8DBA20` (to host) <- `0xB6A040`, `0xB97B30` | `0x8D77B0` <- `0xA4F940` | hit on a chr [C] | routed |
| 28 | `0x8D9470` <- `0xB6C460` | `0x8D7320` <- `0xA4F940` | second hit record | routed |
| 52 | `0x8D9450` <- `0xA05060` | `0x8D7290` <- `0xA1B770` | chr state by entity id | routed |
| 18 | `0x8D9E60` <- `0x7FDF00` | `0x8D7D70` <- `0x7FAB00` | throw / deathblow start [C] | routed |
| **56** | `0x8DA1E0` <- `0x7FA980` | `0x8D83B0` <- `0x7FAB00` | throw family (same consumer as 18) | **not routed** |
| **48** | `0x8DA4C0` <- `0x7FB340` | `0x8D84B0` <- `0x7FAB00` | throw family | **not routed** |
| 36 | `0x8D9E40` <- `0xA284D0`, `0xA28570` | `0x8D7CE0` <- `0xA4FDD0` | player -> WCM event [U] | not routed |
| **31** | `0x8D98F0` <- `0xA02A10` (chr update), `0x8DA3F0` | `0x8D76C0` <- `0x8DD030` | chr update notice | not routed |
| 13 / 19 | `0x8D95A0`, `0x8DB640` / `0x8D9760`, `0x8DB700` <- `0xA37560`, `0xA28160` | `0x8D73B0` / `0x8D7530` | remote-player state | not routed (PlayerSession seam carries the player) |
| 37 / 38 / 20 / 39 / 23 (PlayerSession) | `0x1061D50` / `0x1061D70` / `0x1061D30` / `0x1061CE0` | `0x8D7C40` <- `0xA35290`, `0x8D7BB0` <- `0xA37560`, `0x8D7B10` <- `0xB72A80` | player channel | carried by native.rs |
| 26 | `0x8D9ED0` <- `0x6C4890`, `0x904EF0` | `0x8D7E90` <- `0x838B50` | event flag [C SPEC] | world.rs instead |
| 14 / 15 | `0x8D9E80` <- `0x68CC90` / `0x8DA490` <- `0x688EC0` | `0x8D7E00`, `0x8D8440` <- `0x68E370` | event-system | not routed |
| 42 / 43 | `0x8DA130` <- `0x88E390` / `0x8DA110` <- `0x88DD10` | `0x8D8260` <- `0x8975E0` / `0x8D81D0` <- `0x8974C0` | map object | 42 routed, 43 not |
| 45 / 46 / 47 | `0x8DB7E0` <- `0x6BC1B0` / `0x8DB8C0` <- `0x6B9E20`.. / `0x8DA640` <- `0x6BC1B0`, `0x6BE2D0` | `0x8D8750` <- `0x6BEBB0` / `0x8D92C0`, `0x8D8540` <- `0x6BE740` | ObjAct | 45 stage 4; 46/47 not |
| 29 | `0x8DA150` (to member) <- `0x69D310` | `0x8D82F0` <- `0x8E77E0` | death/return notice (event script) | not routed |
| 27 | `0x8DA1B0` (to host) <- Lua event conditions `0x697070`.. | [U] | event request to host | not routed |
| 59 / 62 / 64 / 65 | `0xB1D600`, `0xB1BC10` | `0xB1D600` | bullets | artfx.rs owns bullets |
| 44, 58 | `0x91D700`..`0x91DBA0`, `0x916B20`, `0x91A440` | `0x91C6E0`, `0x916940` | session manager / summon flow | not routed |
| 49, 50, 51, 57 | `0x65F340`, `0x89CD80`, `0x89BB30`, `0xEC9010` | `0x662B50`, `0xA7A080`, `0xEC9010` | world / area / misc | not routed |
| 7-12, 16, 17, 24, 25, 30, 60, 61, 69, 72-76, 34 | `0x8DA220` (join world state, sent on request 10), `0x8E4F60`, `0x8E5230`, `0x8E77E0`, `0x8DD030` | `0x8DD030`, `0x8E77E0` | session join / housekeeping | not routed |

- No receiver was found for 24, 60, 61 or 68 (68 = `0x8DA180` <- `0xA2C2B0`); they may be consumed by the
  two session dispatchers through another path [U].
- No enemy-path handler was a stub. The packet layer has no type->handler table: each consumer polls its
  own type.
- Netcode RTTI classes: `NetAIManipulator`, `NetworkManipulator`, `ComManipulator`,
  `SprjChrBehaviorSyncModule`, `PlayerNetworkSession`, `PlayerSession`, `PlayerNullSession`,
  `PlayerDebugSession`, `SprjSessionManagerImp`, `FrpgSessionManagerImp`, `SprjGameSessionManager`,
  `PacketStorage`, `SprjPacketStorage`, `CSPacketBase`, `FrpgNetManImp`, `FNMultiplay`, `FNBreakIn`,
  `FNVisitor`, `EcTestMultiplayState`, `EcTestChrSosInvadeType`, `ModuleNetworkHostTestStep`,
  `ModuleNetworkClientTestStep`, `ModuleSyncBehaviorTestStep`, plus the `EventEntityCommandUtil` functors
  above.
- The sync table `T` itself has no RTTI (plain struct). Full list: `E:\RE\work_net\o_rtti.txt`.

### 7.3 What the mod reimplements that the engine has [I]

| Engine has | Mod (test11) | Note |
|---|---|---|
| Boss pin via authority bias `chr+0x1A1C` | host table boss rule | native bias works only while the host's chr exists; our absence rule is still needed |
| Relevance-based candidacy + one arbiter (22/23) | host table by distance | native relevance is LOD-based (10/15 budgets, visibility, area mask); ours is 40/60 m |
| Fixed update rate (2004[34]) | `activation.rs` interval override | vanilla events already pin bosses |
| Corpse runs locally on a guest (`0x9F4130` via global IsClient) | puppet/type-5 guards (v8.60) | global IsClient is false in our session, so a guest's corpses stay NetAI |
| Host-authoritative HP (type 5) | livesync / persist | already native and routed |
| Throw family 18 + 48 + 56 | 18 routed, `dblock` | 48 / 56 not routed |
| Phantom role (global IsHost/IsClient) | per-call-site fakes | the unfaked globals drive the damage router's action 2, SpEffect gates, corpse handling |

---

## 8. What the mod could hand back to the engine (ranked by value / risk)

1. **Pin bosses with the native bias, host-only.** Value high, risk low.
   - Host writes `chr+0x1A1C = 4095` on boss activation and 0 while absent; clients never write it.
   - This makes native 22/23 agree with the host table, extends candidacy to range fights (Divine Dragon at
     60-85 m), and gives the `0xA4A680` path the same answer.
   - Live checks: the host's `chr+0x1FC8` is >= 4095 for the boss; the client's 23 omits it; no DISOBEY.
2. **Use native relevance for ordinary enemies instead of 40/60 m.** Value high, risk medium.
   - Option A: the host table accepts a client's native candidate list (the 22 it already receives) as the
     "near" report, so its rows follow the game's own LOD and area masks.
   - Option B: drop the override for ordinary enemies and let `0xA4A650` answer natively, keeping only
     bosses, claims and the absence rule in the detour.
   - Live checks: log, per ownerless enemy within 40 m, `chr+0x110`, `chr+0x1A10`, `U+0x10/+0x14/+0x20`,
     `0x9E5C00(U)` and `0x9F0230(chr)`, to see which clause of §3.1 rejects it.
3. **Shorten the native timers rather than adding transport.** Value medium, risk low.
   - After WCM creation, write `T+0x398` (23) and `T+0x3A0` (22) to e.g. 0.5 / 0.5 s; type 4 `T+0x3B4`
     0.25 -> 0.1 s.
   - Live checks: lane bandwidth (22 is <= 4 KB), host CPU (`0xA1D550` is O(records² · n²) on linked
     lists), deflect timing on client-near enemies.
   - Add our own hold in `decide` or in a 22 filter; the engine has none.
4. **Seed the new owner's AI target on a take.** Value medium (removes the stealth window and the
   turn/idle hitch), risk medium.
   - On the frame **after** the switch to Com (after `0x7E54D0` has wiped the records), read the NetAI
     copy's `manip+0x420/+0x440` (mode 1 netId), map it to our chr, and call `0x630C50(slots, 0, chr)`, or
     `0x674640(EventMan, enemyEntity, playerEntity)` (soft, team-gated).
   - Never for a target with no prior record on a stealthy approach.
   - Live checks: TS records before/after; does the enemy keep attacking the same player.
5. **Route 48 and 56 with 18.** Value unknown, risk low.
   - Same consumer `0x7FAB00`; check in a log whether they appear in deathblows or grabs and need the
     player-netId rewrite.
6. **Make DISOBEY classify the real force-local causes** (`0x9F33F0` inputs). Optionally hook `0xA4A680`
   to the same table as `0xA4A650` (tiny function, one caller). Value low/medium, risk low.
7. **Consider faking global IsClient on the client** only for `0x9F4130` (corpse force-local), so a client's
   corpses stop following the host's corpse records natively. Value low/medium (puppet and corpse edge
   cases), risk medium (`0x8E44C0` has ~30 callers: patch the one call site instead).

## 9. Not determinable from Sekiro alone (needs DS3 / ER reference or a live session)

- Whether DS3's 2004[28] handler is the same "add to prio" bias. Sekiro has the handler and the field, and
  ER names it `network_authority`, but the DS3 binary was not checked.
- What sets the chr-set entry flags `U+0x20` bits 0/1 and `U+0x14` besides `0x9F4130`, what
  `0x9E5C00(U)` (`*(U+0x30)+0x8C` bit 0) and module0 `+0x54` bit 0 mean, and therefore which enemies are
  force-local or "not normal" (interval -1) in practice.
- The DS3 default LOD budgets (Sekiro dump: 10 / 15, rings 10/20/40/50 m) and whether DS3 hosts see frozen
  far enemies with phantoms spread out.
- The meanings of types 24, 27, 31, 36, 46, 47, 48, 56, 60, 61, 68 and the session-internal ones.
- `chr+0x1A20` producers, and whether anything other than the debug menu changes `chr+0x1FF0`.
- Whether DS3 resets AI goals (not only targets) on a manipulator switch; Sekiro clears TS records and
  target slots only.
