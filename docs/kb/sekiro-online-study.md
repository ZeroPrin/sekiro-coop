# Sekiro Online v2.05 (LukeYui): how it synchronises enemies — static study

Date: 2026-11-14. Static reverse engineering only, for learning which **engine mechanisms** the
author relies on. Nothing was executed, loaded or installed; the game was not started. No code
from the mod is reproduced here beyond a few illustrative bytes; all logic below is described in
our own words. Nothing from this study goes into our source tree.

Previous study of an earlier copy: `docs/RE-SEKIRO-ONLINE.md` (Finnish, IDA, 2026-10-23). This
note re-derives the hook map on the official Nexus build, confirms most of it, corrects two
details and adds the enemy-specific findings that were missing (sections 4-7).

Evidence tags: **[DIS]** confirmed in disassembly/decompilation (ours, of his DLL and of the 1.06
dump); **[INF]** inferred from structure/context; **[UNK]** not determined. Game addresses are
RVAs on 1.06 (image base `0x140000000`).

---

## 1. Triage

Archive `Sekiro-Online-v2.05-577-2-05-1623599819.zip`, extracted to `E:\RE\work_sekiro_online\src\`.

| File | Size | SHA-256 |
|---|---|---|
| `dinput8.dll` | 424 960 | `B40DD9C34B4192F546E65B2306959FFE276AF2991D551B2FABC46CC0BDA7DAD1` |
| `readme.txt` | 3 514 | `B35A95A90AB32183E971A4172DBB9EFCC90BF7B7503080E2886574547E20F91F` |
| `SekiroOnlineFont.ttf` | 5 577 580 | `9978D2B1A39886631A9F2D95E3B7AB3F29BB0960C9A6101242D615BBF6E8DEDD` |

The DLL hash differs from the copy in `RE-SEKIRO-ONLINE.md` (`880bea34…`), same size and same
PDB path; the hook tables resolve to the same game addresses, so it is the same release line.

**`dinput8.dll`** (x64 PE32+, image base `0x180000000`, entry RVA `0x4E500`):
- Timestamp 2021-06-10 19:48:00 UTC. PDB
  `H:\Development\workspace\soulsmodding\Sekiro-Online-Release\x64\Release\dinput8.pdb`.
- Sections: `.text` entropy 6.46, `.rdata` 5.78, `.data` 3.75 (raw 0x2000, virtual 0x107EB8),
  `.pdata` 5.75, `.reloc` 5.20. No overlay. Ordinary MSVC section names.
- Export: `DirectInput8Create` only (proxy; the real `dinput8.dll` is loaded from the system dir).
- Imports (198): KERNEL32 (incl. Thread32First/Next, Suspend/ResumeThread, Get/SetThreadContext,
  VirtualProtect, FlushInstructionCache: a MinHook-style hooker), USER32, `steam_api64.dll`
  (only callback registration + `GetHSteamPipe/User`; interfaces are taken from
  `steamclient64.dll` by version string), WS2_32 (socket/connect/send/recv), WINHTTP, d3d11,
  D3DCOMPILER_47, IMM32 (Dear ImGui overlay), CRT.
- TLS directory present, callback list empty. `IsDebuggerPresent` is only the CRT's use.
- RTTI class names visible: `CHooks`, `CSigScan`, `CNetworking`, `CPlayer`, `CNetPlayer`,
  `CNetWorldChr`, `CPacket`, `CProtoGame`, `CProtoServer`, `CYuiKeyNexus`, `CSummonSignManager`,
  `CLogManager`, `CGraphics`, `CInternal`.
- Strings: Steam interface versions `SteamNetworkingSockets009`, `SteamNetworkingUtils003`,
  `SteamMatchMaking009`, `SteamFriends017`, `SteamUser020`, `SteamNetworking005`; item names used
  as online items; a handful of small XOR-obfuscated strings (decoded examples: "You cannot rest
  while there are spirits in your world", "NetPackingVector: WorldState = %u").
- **Verdict: not packed, not virtualised, no commercial protector.** Only light string XOR and a
  custom packet hash/cipher ("YuiKey"). Analysis continued.

`SekiroOnlineFont.ttf`: a plain TrueType font (21 standard tables). `readme.txt`: feature list
and usage; it contains **no licence/EULA text and no terms about analysis**. Relevant claims in
it: "The mod itself runs purely on P2P connections. There is a server, however this is only used
for the summon signs"; "PvE combat … enemy movement and actions are much smoother, and uses a
degree of client prediction"; "World sync is complete … it'll load the area and enemies
correctly"; "Events in your world no longer synchronise with the host's world"; "Players will be
scaled up/down … PvP damage is normalised"; capped at 6 players.

**Target version: Sekiro 1.06** [DIS]. The DLL holds two AOB tables (entries
`{pattern*, mask*, u32}` every 0x18 bytes, `.data` 0x63538-0x64640). 190 of 197 patterns match
the 1.06 in-memory image (`E:\RE\carve\sekiro_mem.exe`) uniquely, and the hard-coded globals
match our 1.06 docs (WorldChrMan `0x3D7A1E0`, GameDataMan `0x3D5AAC0`, EventFlagMan `0x3D55FE8`,
GameMan `0x3D5C0D0`, SessionManager `0x3D948E8`, FrpgSessionManager `0x3D67C90`).

Tooling and private working files (not for redistribution): `E:\RE\work_sekiro_online\tools\`
(`pe_triage.py`, `aobtab.py`, `fieldmap.py`, `hooktab.py`, `gq2.py`; outputs `aob.tsv`,
`fields.tsv`, `hooks.tsv`), Ghidra projects `E:\RE\ghidra_sekiro_online\so` (DLL) and
`…\game` (copy of `ghidra_quick`, functions created on demand from `.pdata`).

---

## 2. Q1 — Architecture [DIS]

- **Transport: own, over Steam.** Session = Steam lobby (`SteamMatchMaking009`), peer traffic =
  `ISteamNetworkingSockets009` connections (SDR relay). A plain TCP socket to a central server is
  used only for summon signs/matchmaking, WinHTTP only for a version check.
- **Payload: the game's own DS3-lineage packets.** The DLL does **not** run the game's
  `SessionManager`/member vector. It redirects every native send/receive call site (≈70 call-site
  redirections: a detour plus a 1-byte `E8` patch so only that one `call` changes) to its own
  mailbox per (member, type) and re-labels the native type with its own id (table in §4.2).
- **Session state is faked per call site**, not by filling the native member vector: InSession
  (`0x1072FE0`), IsHost (`0x1072F80`), IsClient (`0x1072F00`/`0x8E4B10`), "offline"
  (`0x832050`), member count (`0x8E3E60` and three inline `(end-begin)/56` computations) are
  answered from the DLL's state (−1 none / `0xFFFEFFFF` host / `0xFFFDFFFF` client).
- **Receive loops over the (empty) native member vector are collapsed to one pass** by NOPping the
  6-byte loop/empty-check jumps, so the receive call inside runs and the DLL returns a packet from
  whichever peer has one: type 70 `0xA1B035`/`0xA1B250`, type 4 `0xA1BEB3`/`0xA1C09D`, type 71
  `0xA1D946`/`0xA1DA65` (in `0xA1D8D0`).
- **Remote players are native `PlayerIns`**, created through the game's own member-spawn
  `0xA7A840` (hook at `0xA7A275` in the net-step update) and removed with `0xA4BF90`; they are
  driven by native type-1 packets and the game's network manipulator. Nothing is puppeted.

## 3. Q2 — Hook/patch inventory (grouped)

Full machine-readable list: `E:\RE\work_sekiro_online\tools\hooks.tsv` (97 hook installs) plus
the byte patches below. This agrees with `RE-SEKIRO-ONLINE.md` §2-5; only the groups are listed.

| Subsystem | Sites (RVA) | What it does |
|---|---|---|
| Session fakes | `0xA1AF9C 0xA1B36D 0xA1BE41 0xA1C18B 0xA1C8A3 0xA1CA03 0xA1DB08 0xA1D918 0x91870B` (InSession), `0xA1CF85 0xA1D4B0 0xA4B8A4` (+1 unresolved) (IsHost), `0xA1D506 0xA4BA81 0xA6F351` (IsClient), `0x76F10B 0xF0E6B7 0x89A689 0xA4CF79` (offline/slot selector), `0xA4DC38 0xA734D8 0x6BC504` + mid-hooks `0xA1CF13 0xA1DD5E 0xA4E5BE` (member count) | answer from DLL state |
| Players | `0x8DA0DD`/`0x8D8050` (type 1 send/recv), `0x8DA142 0x8D8260 0x8D9464 0x8D7290 0x8D9484 0x8D7320 0x8D9E74 0x8D7D70 0x8DB7F4 0x8D9EB8 0x8D9583 0x8D9684 0xB1D6AE 0xA2E22D 0x8DB3B2 0x8DB4B8 0x8DB564 0x8D9802 0x8D9419 0x8D71C0`, spawn `0xA7A275`, `0xA5047F` (member lookup, skips 0xA7 bytes) | relay player packets; spawn native remote PlayerIns |
| Enemies | send/recv `0xA1C4F9`/`0xA1BF0B` (4), `0xA1B4BA`/`0xA1B07B` (70), `0xA1DC74`/`0xA1D98F` (71), `0xA1D1FC` (22), `0xA1E005`/`0xA1E075` (23), `0xA1C8D0` (5, recv only); loop NOPs (§2) | relay native enemy sync; see §4 |
| Damage / effects | `0x8DA215`, `0x8DBA35` (type 21 send), type 21 recv (pattern not resolved on our dump), `0x9E9F53` (SpEffect apply `0x9F8320`) | relay hits with an extra lives byte (§4.4); SpEffects to players (9) and enemies (29) |
| Events / flags | `0x8D9F46` (flag send), `0x6C44A0` (flag-block reset only offline), `0x65F657` (object events), `0x69D44C…0x69D491` ×6 and `0x6A2F8E` (`xor al,al`: EMEVD multiplayer conditions false), ESD env `0x84EE44 0x84EECD`, `0x84E814 0x84EA1F` (`xor al,al`), `0x909870` switch bound `0x11→0x0D` (patch `0x9098AD`) | single-player-style events; host flags pushed |
| World state | `0xA6F404 0xA6F45A` (client flag bank `EventFlagMan+0x220`), WorldState/EventState join packets (§4.6) | load the host's world |
| Items / UI | `0x10BF630 0x10B4BF0 0x10CEC30 0x10AA1CE 0x10AA24E 0x10A9E9E 0x10CEA50 0xD4E6BC 0x66CE66 0x7C0DE0 0xBD824B 0x1137AA0 0x117396B 0x844C80 0x69E5AA` | online items, texts, names, misc |
| Rest | `0x11C3A96` (idol interaction `0xEA7AE0`, action ids 6100/6101), `0x11C606A` | blocks resting while spirits are present |
| Scaling | `0xC04EA4`, `0xC04FFA`, `0x10B8D30` | §6 |
| Time / stability | `0x545020` (FD4Time ctor), `0xB573E0` (PlayerInAirAction buffer guard), `0x19DC9D0` (DL_PANIC → log) | |

Correction to `RE-SEKIRO-ONLINE.md` §3: the 4-byte header of the player packet (type 1) is not
written into "the session"; the receiver writes the two u16s into the **remote PlayerIns's posture**
(`*(*(chr+0x1FF8)+0x18)` `+0x148` current / `+0x14C` max) [DIS]. Player posture is synced by that
header on every player packet.

---

## 4. Q3 — Enemies

### 4.1 Who runs the AI [DIS for the plumbing, INF for the native semantics]

**Entirely the native NetAI ownership protocol.** The DLL contains no enemy logic of its own: no
position/HP writes, no ownership table, no ComManipulator/NetAI switching, no `IsOwnedHere`
(`0xA4A650`) detour, no slot-37 hook. What it does:

1. Makes the game believe it is in a session with N members (§2), so slot 37 (`0xA14B50`) gets
   `forceLocal = 0` from the chr loop `0xA4DC38` and consults the native ownership table.
2. Feeds the candidate builder `0xA4E520`: mid-hook at `0xA4E5BE` replaces the member count it
   reads from the session vector (`0x4924…` = /56) with the DLL's count [DIS].
3. Relays native type 22 (client → host candidates, `netId | prio<<16`) as its type 24, and only
   when it is a client; on the host it feeds each peer's list into the native per-member record
   with `0xA1D7E0` (get record) + `0xA1C6C0` (store list) [DIS].
4. Relays native type 23 (host → each member's owner list) as its type 23, only when host; the
   send stub at `0xA1E005` maps the native per-member record to the peer whose PlayerIns matches
   and sends it to that peer only [DIS].
5. Relays type 4 (movement), 70 (behaviour/hkb), 71 (actions) from every machine to every peer
   (types 22, 25, 30), only when ≥ 2 members [DIS].

So: **the host arbitrates, by the game's own priority (`0x9F0230`, distance-based) with the
game's own timers (22 every 5 s, 23 every 3 s) and tie-break (fewer candidates wins)**. Each
machine simulates only what the last 23 gave it; everything else is NetAI and follows records
[INF from our own native-protocol RE, `kb/enemy-authority-handover.md`].

### 4.2 Message map for enemies

| Native type | Content (from our RE) | DLL type | Direction | Notes [DIS] |
|---|---|---|---|---|
| 4 | movement, 28 B records, 8 enemies/packet | 22 | owner → all | recv loops all peers, first packet wins per call |
| 70 | behaviour (hkb events/vars, AI target), ≤ 0x5700 B | 25 | owner → all | **receive validates the packet** (below) |
| 71 | action inputs, 52 B × n | 30 | owner → all | |
| 22 | candidates `netId|prio<<16` | 24 | client → host | sent only by a client |
| 23 | owner list | 23 | host → that member | sent only by the host, routed per member |
| 5 | HP (host → client, per our RE) | 28 | host → client | receive hooked; **no sender found** [UNK: maybe the unresolved pattern] |
| 21 | hit report (damage router) | 27 (+1 byte) | attacker → all | §4.4 |
| — | SpEffect on an enemy | 29 | applier → all | `{u16 netId, u32 spEffectId, u32 check}`, applied with `0x9EE760` |
| 18 | throw (deathblow/grab) | 14 | | relayed as is |

Transport detail [DIS]: one receive slot per (peer, type); a newer packet overwrites an unread
one. Everything is sent with Steam flag 2 (not the Reliable flag 8). Latest-wins is fine for 4/70/71
but lossy for 22/23/21.

**Type-70 validation [DIS].** Before handing a behaviour packet to the game, the receive detour
checks: the map-block index in the first netId (`netId>>11`) is below the sync table's block
count (`*(WCM+0x630)+8`), the block exists, the record count is below that block's capacity
(`+0x10`), every record size (from the game's own size function) is ≤ 0x138 and the running total
≤ 0x5700, and every netId resolves to a chr. Otherwise the packet is dropped. This is a crash
guard for the native parser.

### 4.3 Hand-over, "client prediction", double simulation

- **Hand-over:** none of its own. Ownership changes only when a new 23 arrives; the receiver's
  native handler resets its whole table and marks the listed netIds (§4 of our KB note). No AI
  state is transferred [DIS absence].
- **"Client prediction"** (changelog): no prediction code exists in the DLL — no float math on
  enemy chrs, no extrapolation, no timers on the sync table [DIS absence]. The phrase most likely
  refers to the engine's own NetAI interpolation (`0x7EB2A0`: velocity toward the last record,
  snap thresholds) that becomes active once the native protocol runs [INF].
- **Avoiding two simulators:** structurally, by having exactly one arbiter (the native host
  distribution) and never overriding it locally. It inherits the native quirks: prio-0 ties
  (`0xA1C590`) and the "ownerless enemy" case (an enemy that is NetAI everywhere never becomes a
  candidate) [INF — not handled anywhere in the DLL].

### 4.4 HP, deaths and lives [DIS]

- HP: no HP channel of its own. The native damage router reports hits as type 21; the DLL relays
  it to everyone and every machine applies the same hit to its copy, so HP falls everywhere.
  On receive it **drops hits whose target is the local player** (the first u16 of the payload,
  compared with the local player's `chr+0x78`): a player's damage is decided on the victim's
  machine.
- **Lives piggyback:** the sender prepends one byte = the target's lives counter as seen on the
  attacker's machine (`data+0x25C`, data = `*(*(chr+0x1FF8)+0x18)`). The receiver, before the
  native hit is parsed: if its copy has lives > 0, the byte is non-zero and differs, it writes
  lives = byte, HP = max HP (`+0x130 = +0x134`) and posture = max posture (`+0x148 = +0x14C`).
  I.e. when the attacker has already consumed a life (deathblow), the other copies jump to the
  next life with a full bar, in the same message and the same order as the hit that follows.
  It does nothing for the last life (death comes from HP via the hit itself).
- SpEffects applied to enemies are broadcast (type 29) and re-applied by netId.
- Death of ordinary enemies: no explicit death message; deaths follow from the relayed hits plus
  the owner's behaviour/action stream, and from the world snapshot on join (§4.6) [INF].

### 4.5 State flags and objects [DIS]

Event flags go through the native filter (`0x8D9ED0` only sends net-synced flags; DLL type 17,
5 B). Object events (type 16, 32-B entries) with three entity ids excluded. The client's event
flag block is not reset while in a session (`0x6C44A0`).

### 4.6 World snapshot on join — enemies alive/dead [DIS plumbing, INF meaning]

The host sends a joining peer three blobs:
- **WorldState (type 31)** — four length-prefixed chunks made by native serializers:
  `0x76F230(*(GameMan'+0xBC0))` (with `GameMan' = 0x3D5A6A8`), `0xF0EB60(*(*(0x3D94970)+0x10)+0x5E0)`,
  `0x89A790(0x3D5D600)` and **`0xA4D0A0(WorldChrMan)`**. `0xA4D0A0` writes the `"CHR "` chunk
  (magic `0x43485220`, version `0x20012800`) by iterating the WCM's map blocks (`WCM+0xC0`
  count, `WCM+0xC8` array) and calling `0xA44490` per block — the same per-block chr save data
  the save file holds (dead/alive, positions).
- On the client these are written into GameMan's load buffers through `0x838810`, `0x838950`,
  `0x838AB0`, `0x8388B0` (DLL-side function table entries 0x3B-0x3E), then the native loaders
  `0x76F100`, `0xF0E6A0`, `0x89A670`, `0xA4CF60` read them on the next area load. Each loader
  first asks `0x832050` which buffer slot to use — that call is hooked (offline flag → "in
  session") so the loaders pick the slot that holds the host's data. `0xA4CF60` walks the
  `"CHR "` chunk and applies each block's data with `0xA439D0` (or `0xA43860` for the special
  version).
- **EventState (type 32)** — a raw copy of the whole EventFlagMan block (`*(EFM)`, length at
  `EFM+0x18`), applied with `memcpy` (or through `0x6C3610`/`0x6C4A10` in an alternative mode).
- Type 33 — up to 255 entries `{tag 0x4BB93, x, y, z}` from a list at `0x3F14088` [UNK purpose].

This is what the readme calls "World sync is complete … load the area and enemies correctly".

## 5. Q4 — Bosses and minibosses [DIS absence unless noted]

- **No special boss authority.** No boss pin, no EMEVD `SetNetworkUpdateAuthority` /
  `SetNetworkUpdateRate` call, no hook in any EMEVD instruction handler. Bosses go through the
  same native distance-priority distribution as trash mobs.
- The EMEVD multiplayer conditions (`0x69D310` → six condition calls) and one more test
  (`0x6A2F8E`) are forced false, and the ESD env query `0x8522B0` is answered for ids 60-67 and
  3980, so boss arenas, fog walls and events run as in single player on every machine.
- HP/posture: as §4.4 (relayed hits + lives byte); multi-life boss phases follow the lives
  piggyback.
- Player death: nothing boss-specific; the session (Steam lobby) survives, the game reloads,
  remote players are re-spawned by the spawn hook. Resting is blocked while spirits are present.

## 6. Q5/Q6 — Targeting, scaling, deathblows, stealth, throws

- **AI targeting (Q5):** nothing [DIS absence]: no hook in `SprjTargetingSystem`, `0x60D0A0`,
  team checks or perception. Enemies see and attack remote players because they are genuine
  native `PlayerIns` created by `0xA7A840` (native chr type/team), so the stock targeting treats
  them like any network player. The owner's target is replicated inside the native behaviour
  record (type 70).
- **Scaling [DIS]:** mid-hooks at `0xC04EA4` (inside `0xC04E30`, maxHpRate) and `0xC04FFA`
  (inside `0xC04FB0`, maxStaminaRate = posture) add `0.3 × n` (n = player count, only if n > 1) to
  the ClearCountCorrect row's `+0x00` / `+0x08` before it multiplies an NG-correctable SpEffect
  (`row+0x168 & 0x40`). So enemies with the NG+ scaling SpEffect get +60 % HP and posture with 2
  players. Clients also **take the host's NG cycle**: at `0x10B8D30` (inside `0x10B8D10`) the
  pointer to `GameDataMan+0x70` is replaced by the host's value when not host.
- **Deathblows:** native throw packet 18 (relayed as 14) plus the lives byte on hit packets
  (§4.4). No claim/ownership step.
- **Stealth:** nothing specific [DIS absence]; it falls out of the native AI state on the owning
  machine.
- **Grabs/throws:** native type 18 relayed; no ownership coupling.
- PvP "damage normalisation"/Mikiri on players and the 26/18 messages (custom SFX at a position,
  summon-warp sequences) are player features, not enemy sync.

## 7. Q7 — What it does that we lack or do differently (with the engine mechanism)

Things our docs already cover (`SPEC-NATIVE-SESSION.md`, `PLAN-OWNERSHIP.md`): call-site session
fakes, native 22/23 feeding, EMEVD/ESD single-player conditions, scaling hooks.
New or different:

1. **Native world-state snapshot on join** (`"CHR "` chunk by `0xA4D0A0` → GameMan load buffer
   → `0xA4CF60`, plus the idol/item/object serializers). We copy event-flag records only
   (`world.rs`); enemies' dead/alive state on the client comes from flags and our own lives
   sync. This would give the client exactly the host's per-block chr save state at its join load.
2. **The `0x832050` slot selector in the four world loaders.** The loaders choose between two
   GameMan state buffers (own world vs. host world) — the DS3-lineage phantom mechanism. Hooking
   that selector is how a client loads the host's world without touching its own save.
3. **Lives on the hit packet, in order with the hit.** We sync lives on a separate ~7 Hz Lives
   message with a min-merge (`livesync.rs`). Carrying the attacker's lives value in the type-21
   relay makes the "jump to next life, refill, then apply the hit" ordering exact on every copy.
   Could be added as a fast path beside the existing merge.
4. **Type-70 receive validation** (block index, record count vs. capacity, record ≤ 0x138,
   total ≤ 0x5700, netId resolves). A cheap crash guard before `0xA1AF40` parses peer bytes.
5. **Victim-authoritative player damage:** hit reports whose target is the local player are
   dropped on receive. Check that our 21 path does not double-apply hits on the local player.
6. **Player posture in every player packet header** (u16 current/max → remote PlayerIns data
   `+0x148/+0x14C`).
7. **Client uses the host's NG cycle** for ClearCountCorrect (`0x10B8D30`). If host and client
   differ in NG+, our scaling multiplies different rows; pinning the row to the host's cycle
   removes a source of HP/posture mismatch with absolute HP sync.
8. **Single arbiter, no local override.** It never overrides `IsOwnedHere`; we moved to a
   host-authored table (test11). His design shows the native path alone is coherent, but it also
   inherits the native gaps (ownerless enemies, slow 3-5 s cadence) that made us add our own.

Things **not** worth copying: latest-wins mailboxes for 21/22/23 and unreliable Steam flag 2 for
everything; the global `.rdata` vtable patch (per `RE-SEKIRO-ONLINE.md`); no boss pinning.

### Game functions involved that our docs do not cover yet

| RVA | What (from the dump) | Seen via |
|---|---|---|
| `0xA4D0A0` | WorldChrMan → `"CHR "` save chunk writer (per map block via `0xA44490`) | WorldState send |
| `0xA4CF60` | `"CHR "` chunk loader from the GameMan buffer (`0x831220`), applies blocks with `0xA439D0` | DLL table, hook `0xA4CF79` |
| `0xA44490` | per-block chr state serializer | called by `0xA4D0A0` |
| `0x831220`, `0x831F40` | GameMan state buffer get / slot valid (slot from `0x832050`) | in `0xA4CF60` |
| `0x838810`, `0x838950`, `0x838AB0`, `0x8388B0` | store a received chunk into GameMan's load buffers | WorldState apply |
| `0x76F230` / `0x76F100` | serializer / loader for `*(0x3D5A6A8)+0xBC0` (idol list? [INF]) | WorldState |
| `0xF0EB60` / `0xF0E6A0` | serializer / loader for `*(*(0x3D94970)+0x10)+0x5E0` [UNK] | WorldState |
| `0x89A790` / `0x89A670` | serializer / loader for `0x3D5D600` (object/item respawn) | WorldState |
| `0x9EE760` | apply SpEffect to a chr by netId (enemy path) | type 29 apply |
| `0xA1D8D0` | type-71 receive (loop at `0xA1D946`/`0xA1DA65`) | loop NOPs |
| `0xA50120` | finalises a joining member (remove + re-create chr, `0xA4BF90`/`0xA492E0`) | hook `0xA5047F` |
| `0xA4E520` | ownership candidate builder (member count read at `0xA4E5BE`) | mid-hook |
| `0x11C5FF0` / `0x11C1A20` | idol interaction state / rest action (ids 6100/6101) | rest block |
| `0x909870` | event-command dispatcher, 18 cases (the mod cuts cases 15-18) | patch `0x9098AD` |

## 8. Ideas worth checking in the engine (ranked by value)

1. **Join-time `"CHR "` transfer** (`0xA4D0A0` on the host, write into the client's GameMan
   buffer, let `0xA4CF60` load it with the slot selector forced). Read-only first: dump the chunk
   on both machines after a kill and diff the per-block bytes for that enemy.
2. **Pin the client's ClearCountCorrect row to the host's NG cycle** (`0x10B8D10`, read of
   `GameDataMan+0x70`). Small hook; removes a silent HP/posture mismatch when NG levels differ.
3. **Validate type 70 (and 4/71) before the native parser** with the same bounds (`*(WCM+0x630)`
   block table, capacity `+0x10`, record ≤ 0x138, total ≤ 0x5700). Cheap crash insurance.
4. **Attach the attacker's lives value to the relayed hit 21** and apply "lives → refill" before
   the hit on the receiver, alongside `livesync.rs` (fast path; the merge stays authoritative).
5. **Drop received 21s that target our own player**, if our relay does not already.
6. **Check whether native type 5 needs a sender at all** in our model (he hooks its receive but
   never sends it; HP stays consistent through 21 relays alone in his design).
7. Investigate `0xF0EB60`/`0xF0E6A0` and the type-33 list at `0x3F14088` (tag `0x4BB93`) — part
   of what he considers "world state", unknown to us.

## Confidence summary

| Claim | Tag |
|---|---|
| Not protected; targets 1.06 | DIS |
| Own transport (Steam lobby + NetworkingSockets), native packets relayed by call-site redirection | DIS |
| No own enemy logic; native 22/23 host arbitration with fake member counts | DIS (absence of other code + plumbing) |
| "Client prediction" = native NetAI interpolation | INF |
| Lives byte on hit relay and receiver refill | DIS (offset meaning from our docs) |
| Type-70 validation | DIS |
| `"CHR "` WorldState chunk and loader path | DIS plumbing, INF semantics of buffer slots |
| No boss pin / no EMEVD authority calls / no targeting hooks | DIS (absence) |
| +0.3·n HP/posture rate; client uses host NG cycle | DIS |
| Type 5 has no sender | DIS within the resolved tables; one hook pattern unresolved → UNK |
