# share-pickups fork

Unofficial branch of Sekiro Together, based on the newest public source (1.0.9-beta,
commit `e11f68b`; the 1.0.10 / 1.0.11 sources were not published when this was made).
Version string: `1.0.9-pickups.3`. Everybody in a session needs the same build.

## What it adds: shared map pickups (`[GAMEPLAY] share_pickups`, default 1)

An item one player picks up from the map while connected (treasure, chests, items on
corpses: a one-time map lot with one sure goods slot) is given to the other player too, and
that lot's flag is set on their side, so they cannot pick it up again.

- **Detection** (`keyitems.rs`, `KeyItems::pickups`): the map treasure rows of
  ItemLotParam (`classify_lot` = `MapTreasure`, one sure goods slot, not a co-op item) are
  indexed by lot flag (`LotIndex::map`, `map_by_flag`). Once per load, when the world has
  settled (10 s) and a session is up, every one of those flags is read; after that our own
  flag writes (the SetEventFlag ring in `hook.rs`, bank 0) that switch one of them on are
  our new pickups (`scan_pickups`). Pickups made alone or before the session are never sent:
  the snapshot is taken again after every load and while not connected nothing is watched.
- **Message**: bulk kind 46 (`BULK_PICKUP`), the ledger format with reason 2
  (`LEDGER_PICKUP`), sent to every link (host <-> each client). An older build leaves it
  unread.
- **Receiver** (`apply_pickups`), 2 s later and only when `game::safe_for_reward` allows:
  per row, its lot flag off here -> its sure goods through GrantItems (small pickup log) and
  the flag set; a unique item already owned only gets the flag; flag already on = already
  picked up here, nothing. The flag write we make is not sent back (`pickup_echo`).
- **Not covered**: shops (their flags are ShopLineupParam flags), lottery lots (no sure
  slot), pickups of 3+ player sessions between two clients (links are host <-> client),
  and a pickup in the first ~10 s after a load / warp / death reload (the map flags are
  read when the world has settled; a pickup before that counts as an old one).

## Taking the item off the map (1.0.9-pickups.2)

Live test of pickups.1 (2026-11-09): setting the lot flag does not remove an item that is
already lying on the loaded map; the friend picked the host's Pellet up a second time.
From a memory dump of the running 1.06 exe (layout checked live on 133 loaded treasures):

- MapItemMan (`*(base+0x3D6CDC0)`) keeps map items in linked lists per kind
  (`handle & 0xF`): 1 `+0x20`, 2 `+0x40`, 3 `+0x30` (map treasure), 4 `+0x50`, 5 `+0x68`
  (enemy drops, no flag). Node: `+0x20` handle, `+0x28` lot flag, `+0x30` position,
  `+0x54` item count, `+0x58` items.
- `0x91FCF0 PickUpMapItem(mim, handle, bool)` = the player's pickup: `0x91D450(handle, 1)`
  then GrantItems with the node's items. `0x91D450 MapItemPicked(mim, handle, picked)` sets
  the node's flag (`0x921E40`) and takes the node off the map (`0x91FAD0`): glow and prompt go.

So the receiver now, after giving the friend's pickup, finds the loaded nodes with that
lot flag (the sure slot's flag or the row's) and calls `0x91D450(handle, 1)`: the item
disappears as if picked up, nothing is given twice. And `0x91FCF0` is detoured: a pickup of
a node whose flag the friend's pickup set here (and is on) is refused the same way (node
off the map, no GrantItems). Log lines: `pickup: lot L taken off the map here (handle ...)`,
`pickup: a second pickup refused ...`.

Still open: chests. If a chest is an ObjAct whose item node only spawns when it is opened,
the chest itself stays closed for the receiver (opening it gives nothing). Enemy drops
(kind 5, no lot flag) are not shared.

## ObjAct sync (experimental, `[GAMEPLAY] sync_objact`, default 1; NOT verified live)

Problem: in 1.0.9 only event flags travel. A door, gate or chest the friend opened stays
closed here, and even where the object was replayed (doors.rs, the game's own type 45) the map
script did not go on: the door templates (common_func 20005616 / 20005615, m11_01 11115620)
and the chest templates (20005510 / 20005511) wait for `ObjActEventFlag(ObjActEntityID)` before
they set the door's flag, disable the ObjAct, deactivate the blocker and rebuild the navmesh,
or enable the chest's treasure. Doors also "close again after an area reload" (upstream issue
#3, open in 1.0.11 too).

### What RE says (Sekiro 1.06, dump 2026-11-11, Ghidra copy `E:\RE\ghidra_objact`) [VAR]

- `0x6BA7B0(block, index, actor)` = the local start: sets `info+0x70/+0x71/+0x72 = 1`, takes the
  value from `state->vt[6]`, builds the request (`0x6BCBC0`) and runs `0x6BC1B0(block, req, 0)`.
  Prologue `48 89 74 24 10 44 89 44 24 18 57 48 83 EC 70` (15 bytes, position independent).
  The closed 1.0.11 detours the same function (`objsync ObjAct start (0x6BA7B0)`).
- `0x6BA910(block, index, actor, value)` = the replay (type 45 receive `0x6BE740`, doors.rs):
  `state->vt[5](value)` (door / one-loop states: `SetEventFlag(flag, value != 0)`; one-state:
  nothing), then `+0x70 = ObjActParam.isEventKickSync`, `+0x71 = 1`, `+0x72 = 0`, no packet 45.
- `ObjActEventFlag(X)` = `EcTestObjActExecuted` (test `0x8FCB90`): true while the list
  `[[0x143D56058]+0x70]` holds `{0, X}`. The list is cleared once per event step (`0x6C76D0`,
  right after the EMEVD update `0x67D180`). Its only writer is `0x6B2AF0(info)`: if `+0x70` and
  `+0x0C != 0`, push `{0, info+0x0C}`, then `+0x70 = 0`. `0x6BC1B0` calls it at its end when
  `eventKickTiming == 1`; the info update `0x6B35D0` when the animation ends and timing is 0.
- Every door and chest row (9999xx) has isEventKickSync 0, eventKickTiming 1: **a replay never
  fires ObjActEventFlag**, only the local start does. The actor only moves / animates that
  character; the kick does not depend on it.
- info fields: `+0x0C` = MSB ObjActEntityID (dump vs `objacts_msb.tsv`: 31 infos of m11_01,
  m11_02, m13_00 all match), `+0x50` = object handle (not an entity id), `+0x78` = ObjActParam id,
  `+0x80` = its row (`+0x1B` isEventKickSync, `+0x25` eventKickTiming, dump vs csv).
  `state->vt[4]` (run on `+0x72`) toggles the flag: `SetEventFlag(flag, !flag)`.

So the receiver replays with `0x6BA910(block, index, -1, value)` and then kicks the EMEVD:
`info+0x70 = 1; 0x6B2AF0(info)`, the same push the opener's own start makes. Calling
`0x6BA7B0` on the receiver would kick too, but it also toggles the door flag (the friend's
flag, already applied here, would go off) and sends packet 45 at stage 4.

### Design (`objact.rs`)

- **Sender** (`[DEBUG] objact_sender = hook | poll`, default hook). `hook`: a detour on
  `0x6BA7B0` (bytes checked at start; on a mismatch a log line and the poll sender). The detour
  only notes the call (block, index, actor, run state and flag before, caller); the frame task
  sends it when the actor is our player's handle (`chr+0x08`) and the info started (run state
  3). `poll`: the 50 ms run-state walk of the first version (fallback; `+0x72` evidence). The
  walk runs in both modes, the receiver needs its facts. One-loop states only with
  `sync_objact = 2`; a door's sub infos go out once per 2 s.
- **Message v2**: bulk kind `0xA7`, 29 bytes: magic `0x4F`, version 2, reason 1, state class,
  target value, map id, ObjActEntityID, info index, sub index, ObjActParam id, sequence number.
  A version 1 message (first objact build) is refused with a log line: both players need this
  build.
- **Receiver** (game thread, 300 ms after arrival, retried 5 s, at most 32 waiting): the info
  by ObjActEntityID in the loaded block with that map id; by index / sub only when that id is
  not in the block (an index whose own id differs is refused). Then `receive_plan`:
  replay + kick when idle since the load; **kick only** when it already ran here as a replay
  (`+0x72 = 0`, e.g. the game's type 45 at stage 4) and was not kicked; nothing when doors.rs /
  we replayed it in the last 3 s, it was replayed or kicked since the load, it ran here as a
  local start, our own player used it, a one-loop below mode 2, a door toggling closed, an
  unknown class. The object check (`doors::object_view`) stays: already at that animation =
  nothing. The kick is skipped when isEventKickSync = 1 (the replay kicked), a kick is pending,
  the info has no ObjActEntityID, `0x6B2AF0` has other bytes, or the event manager is missing.
  doors.rs kicks after its own `0x6BA910` too (only with `sync_objact` on).
- **Journal** (reload persistence): every executed ObjAct of the session, ours and the peers',
  by (map id, ObjActEntityID), at most 256 (oldest dropped), only one-state / door states that
  opened (a door closing leaves it; one-loop never: a lift must not move on a reload). Cleared
  on a new session, never written to disk. Once per second, an info of a journaled ObjAct that
  has been loaded and watched for 3 s, has not run since its load and whose object is not at
  that animation is replayed and kicked once per load (`journal:` line). This is the fix for
  doors that close again after a reload.
- **Chests** (ObjActParam 999980 / 999985 / 999990, one-state, kick sync 0, timing 1) take the
  same path: the kick lets 20005510 / 20005511 enable the treasure, so the item node spawns
  here and share_pickups sees it. A chest the friend already looted: its lot flag came with the
  pickup, the `0x91FCF0` refusal of the pickups section still applies.
- Nothing runs in a Remnant memory.

Log lines (prefix `objact:`):
- `0x6BA7B0 hooked ...` / `NOT hooked ...` at start; `sender: hook | poll ...; journal: N` once.
- `we used ObjAct: map ..., ObjActEntityID E, ObjActParam P, <class> state, flag F before B now
  A, run state R0 -> R1, actor A (our player H), caller sekiro+0x...; <why>: sending as #N
  (target state T) | not sent: <why>; journaled (n in the journal)`, then `sent our ObjAct #N`.
  The first 5 calls per load by another actor are logged too (to catch a wrong handle).
- `peer P ran ObjAct #N: map, ObjActEntityID, index, sub, ObjActParam, class, target; journaled`.
- `peer P's ObjAct #N here: block slot, info index (found by ObjActEntityID | by index), ...;
  replayed with 0x6BA910(...) ..., run state now 3; EMEVD kicked: ObjActEventFlag(E) holds ...`
  or `it ran here already as a replay without its EMEVD kick ...: EMEVD kicked ...` or
  `not replayed: <why>`; then the info `1 s after` / `3 s after` with `info+0x70/+0x71/+0x72`.
- `doors.rs's replay (block ..., info index I): EMEVD kicked ... | EMEVD not kicked: <why>`.
- `journal: our | a peer's ObjAct of this session (...), after its load: replayed ...; EMEVD
  kicked ...` (only when something was done, everything with `objact_debug`).
- With `objact_debug = 1`: every execution seen by the walk, every map flag write, raw info
  dumps, the first bytes of `0x6BC1B0` / `0x6BA7B0` / `0x6BA910` / `0x6B2AF0`.

Verified by RE (dump, not live): entry points and arguments, prologue bytes, the
ObjActEventFlag mechanism and its writer, the info and param offsets, the flag toggle. Not
verified live: that the kick from the frame task is seen by the next EMEVD step (ordering
[ARV]: our push lands outside the `0x811C90` event step, so the next step evaluates it before
its clear), that `chr+0x08` of our player equals the actor `0x6BA7B0` gets (dump: the player's
`chr+0x08` is `0x10050000`, a chr-type handle), the journal timing after a reload, chests
spawning their item node here.

Live test protocol (both players on this build, `objact_debug = 1`, `native_session_stage = 3`):
1. Start: each log has `objact: 0x6BA7B0 hooked` and `sender: hook`.
2. Same area, a one-state door (m11_01 sliding door 1113713 / ObjActParam 999970, or any
   9999xx door). A opens it. A's log: `we used ObjAct ... sending as #N`. B's log: `peer ...
   here: ... found by ObjActEntityID ... replayed ... EMEVD kicked`. B walks through the door
   (the blocker must be gone), and the door's script flag is set on B (`flag ... = true` lines).
3. A chest (999980): A opens it, B sees it open; the item node: B's pickup is refused or
   shared as share_pickups says.
4. B reloads the area (rest at an idol or quit to menu and back): `journal: ... replayed ...`
   or nothing because the door loaded open; B can still pass.
5. Repeat 2 with `native_session_stage = 4`: B should log `it ran here already as a replay
   without its EMEVD kick ... EMEVD kicked` (type 45 first) and pass.
6. If nothing is sent: look for `not sent: the actor is not our player` (handle) and retry with
   `objact_sender = poll`.

Open: keyed doors open on the receiver without the key (flags are shared anyway); lifts and
levers (one-loop) stay off unless both set `sync_objact = 2` and are not journaled.
## Loadout sync (experimental, `[DEBUG] loadout_sync`, default 1; not tested live yet)

**Bug.** The friend's native replica (`native.rs`, a PlayerIns with its own PlayerGameData)
never got the friend's equipment. `create` fills the replica's PGD from the first existing
CharaInitParam row (the dev test's row 1080100 does not exist in retail), and the deferred
dress called `ghost::dress_like(remote, player)` with the LOCAL player: the observer's 25
ChrAsm slots (`ChrIns+0x2140`, ids at `+0x24 + 4*slot`, set through `0x9CCBA0`), once.
Nothing about equipment went over the network (the snapshot has pose, animation and HP
only). Seen live: the replica wore the observer's prosthetics, and every combat art of the
friend played as Whirlwind Slash (the template's default art).

- **Ids, not handles** (RE of a live dump, 2026-11-11, `E:/RE/notes-2026-11-11.md`). Layout 1
  sent the ChrAsm slots (`ChrIns+0x2140`, `+0x24 + 4*slot`): those are gaitem HANDLES
  (`0x8080xxxx`, `0x907Fxxxx`, identical to `PGD+0x310` ChrAsm), and the getter `0x9CBE40`
  resolves them through the LOCAL player's inventory, so on another machine they mean nothing.
  The real ids are in EquipGameData (`PGD+0x278`): `+0x374` i32[25] by ChrAsm slot
  (`PGD+0x5EC`), category bits included. Dump values: slot 0 = 70100 (Shuriken), **slot 1 =
  5200 (Nightjar Slash): the right "weapon" is the equipped combat art** (5100 = Whirlwind
  Slash, the template's), 2 = 73000 (Axe), 4 = 72000 (Flame Vent), 3/5 = 110000 (bare hands),
  12..15 = protectors (`0x1000_0000 | id`), the rest -1. That is why the friend's arts played
  as Whirlwind Slash: the replica's PGD kept the template's slot 1.
- **Message**: bulk kind 0xA8 (`loadout::BULK_LOADOUT`; reliable, to every peer; the host
  relays). Body (`coop_net::encode_loadout`): magic `LD`, **layout version 2**, the sender's
  25 EquipGameData ids (GameDataMan+8 -> PGD -> `+0x5EC`), `art` and `extra[3]` (reserved,
  -1; the art is slot 1). Sent when our ids change and every 3 s as a refresh (late join, lost
  packets; only while no bulk chunk is unacked). A layout-1 body (the previous build) is
  ignored and logged once per peer.
- **Receiver**: the last loadout per peer slot (`loadout::of`). When the deferred dress is due
  (`native_dress_delay_ms`), and again whenever a newer loadout differs, `loadout::apply`
  replays on the replica's OWN PlayerGameData (`Remote.pgd`) what the game does when it fills
  a PGD from a CharaInitParam row (`0x7B7470`, the same call `create` uses on that PGD) and
  the equip menu (`0xD592F0`), for each slot whose id differs from the PGD's `+0x5EC`:
  - item already in that PGD's inventory (`0x79D7E0(egd, &id)` >= 0): its entry's handle
    (`0xC3C9B0` / `0xC3CB00`, the menu's way), then equip;
  - else a new gaitem handle `0xC37740(GaitemMan 0x143D888A8, &h, &id)`, added to the PGD's
    inventory `0x79A710(egd, id & 0xF0000000, &h, 1, 1, 0, 1)` (count 1, no "new" mark, no
    auto-equip; fill_a passes 0 for the last one and lets `0x7A12F0` auto-equip), then equip,
    then `0xC396C0(&h)` drops our reference as fill_a does;
  - equip = `0x79F8A0(egd, slot, &h, inv_idx, 0, 1, 0)` (fill_a's arguments): it builds the
    functor `0x7A46B0` -> `0x9CCBA0` on the PGD's ChrAsm (`egd+0x98`) and `0x7A52B0` writes
    `egd+0x24[slot]` = inventory index and `egd+0x374[slot]` = id.
  Then `loadout::sync_chr` copies the PGD's ChrAsm into the PlayerIns holders `+0x2140` and
  `+0x2198` with `0x9CCEE0(holder, pgd)` (what the PlayerIns ctor `0xA217A0` does, and the
  equip menu for a weapon swap), and the parts reload through PlayerIns vtable slot 21
  (`0xA30A50`, only when the slot holds exactly that function and the replica is in the player
  chr set). The menu's own follow-up `0xA2E1B0(chr)` is NOT called: it sends OUR equipment
  over the game's session (`PlayerIns+0x2038`), not a refresh.
- **Validation**: network ids reach a game call only through `loadout::plan`: slots 0..11 take
  a weapon id (category 0) that is a row of EquipParamWeapon, 12..16 a protector id (category
  1) that is a row of EquipParamProtector (`SoloParamRepository`, `find_index`). Anything else
  is skipped with a log line: -1 (unequip is not replayed), accessories / slots 17..24 (fill_a
  adds them through `0x79BF90`, not replayed), a wrong category, a missing row. Unit tests:
  `loadout::tests`, `protocol::tests::loadout_round_trip_and_rejects`.
- **Fallback**: no loadout yet (the friend on an older build, or `loadout_sync = 0`), and N0
  clones: `ghost::dress_like` as before, OUR handles into the replica's holder. Kept on
  purpose: without it the template replica has no visible body (ghost.rs). The first loadout
  then re-syncs the holders from the PGD (even when no slot differed), replacing our handles.
- **Log lines**: `loadout: sent ids [...]` (ours, on change), `loadout: peer N ids [...]`
  (theirs, on change), `loadout: peer slot N slot S a -> b applied (ret inventory I, added /
  in inventory; slot now Some(b))`, `loadout: peer slot N slot S id X skipped (NoRow /
  Empty / ...)`, `loadout: peer slot N K slot(s) applied, H ChrAsm holder(s) of 0x... synced
  from its PGD`, `loadout: refresh of 0x...: parts reloaded (vtable slot 21)` or `... skipped:
  ...`, `native: new loadout from peer slot N, re-dressing ...`, and the fallback `native: no
  loadout from peer slot N yet, ... wears ours for now`.
- **Verified vs not.** From the dump (static, Ghidra): the id array and its values, the
  handle nature of the ChrAsm slots, fill_a's call sequence and arguments, the menu path, the
  PlayerIns ctor's holder copy, that `0x79A710` / `0x79F8A0` work on the EGD they are given
  (inventory `egd+0x168`, ChrAsm `egd+0x98`) and that their side effects on the LOCAL player
  (`0xA2AA00` item-count bookkeeping, `0x7A1B60`) only fire for goods ids
  (`0x400001F4`, `0x4000044C/D`, `0x40000456`), never for weapons / protectors. NOT tested
  live: that the replica's model and moveset follow (the combat art in particular may also
  need the hand/style refresh `0xA26150` the menu calls for weapon swaps, not done), that the
  gaitem manager's new handles behave on a remote PGD over a long session, and the crash risk
  of the vtable-21 reload on a live remote (`loadout_sync = 0` turns all of it off).
- **Live test**: both on this build. The friend equips a combat art and prosthetics the
  observer does not have before the session and changes the art during it. The observer's log
  must show `loadout: peer N ids` matching the friend's `loadout: sent ids`, `slot 1 ... ->
  <art> applied`, the holder sync and the reload; the friend's art must play as itself on the
  replica, the friend's prosthetics must be visible.
## Player packet rate (experimental)

`[ADVANCED] player_packet_hz` (12..60, default 30; 12 = the game's own rate, nothing patched).
The friend's native remote is driven by the game's own player frame, packet type 1 (position,
rotation, HP, sync states, ~76 B). The game sends it every 5 frames (12/s at 60 fps, live log
`type 1: 120 (12.0/s, 9120 B)`), so the remote showed the friend 130-200 ms late at ~100 ms ping.

Found in Ghidra on the 1.06 dump of 2026-11-11 [VAR]:

- Sender `0x7EEB50` (PadManipulator slot 11): countdown `pad+0x360`, minus 1 per frame; at < 1
  (or with the force byte `chr+0x2201`) it builds the frame and sends it (`0x8DA080` →
  `0x8D7000`, session slot 1, type 1), then re-arms with `0x96F8A0(5)` (`mov ecx, 5` at
  `0x7EEF47`, `call` at `0x7EEF4C`).
- `0x96F8A0` reads the debug config key `ChrSync.FrameSkipCount` (UTF-16 at `0x2A110C0`) with
  that default. Its only caller is that call, so nothing else (no other packet type) uses it.
  The count is in frames, not time: 24/s at 120 fps.
- Receiver `0x7EC250` (NetworkManipulator slot 9) measures the time since the previous packet
  itself (`+0x3CC`, reset per packet) and `0x7EC9E0` divides by 1.2 × that. No fixed 1/12 s
  anywhere on the receive side; two packets in one frame snap (interval 0), as before.

What the mod does (`packetrate.rs`, `native.rs`):

- Start-up (setting above 12): checks the call site bytes and the prologue of `0x96F8A0`, then
  redirects `0x96F8A0` to a function that returns the frame count for the setting at the
  measured frame time (2 at 60 fps for 30 Hz, 4 at 120 fps), never more than the game's 5. Log:
  `native: player packet rate 12 -> 30 Hz (interval 33 ms; every 2 frames ...)`, and a line when
  the frame count changes. A change of the setting applies live; from 12 it needs a restart.
- Each side changes only its own sending. Use the same value on all machines, not above the
  other players' frame rate (the receiver plays out at most one frame per frame).
- Receiver, only while the friend's measured interval is under 60 ms (so nothing changes
  against a 12/s sender): the play-out may release half a frame early (the cadence is a whole
  number of the sender's frames), the type-1 queue keeps ~200 ms (6 frames at 30/s), and the
  position pin's delay (`native_pin_delay_ms`, [DEBUG], 0 = auto) follows the measured rate:
  play-out wait + 1.2 × interval − one frame, 60..130 ms (about 70 ms at 30/s). A fixed 130 ms
  at a faster rate made the remote read walking as dashing (2026-10-23, IMPLEMENTATION-NOTES § 7).
  The formula is the game's own 1.2 factor, not tuned live yet.
- `native stats` shows `our type 1 every N frames at F fps` and, per inbox, `play-out` and
  `pin delay`.

Bandwidth per direction and per receiving player, 76 B + 19 B PlayerRaw header + 28 B UDP/IP:
12/s ≈ 1.5 KB/s, 30/s ≈ 3.7 KB/s, 60/s ≈ 7.4 KB/s. The 60/s snapshots (~same size) were already
there.

Live checks: the sender's tee stats `type 1: ~300 (30.0/s ...)`; the receiver's `frame interval
33 ms`, `dropped old` near 0, `pin delay` ~70 ms; by eye: walk / run / dash look right, the friend
is closer to their real position. If walking reads as running, set `native_pin_delay_ms` by hand
(try 90-130) and report the value.

## Fixes to the 1.0.9 base

- **False wipe** (1.0.9-pickups.3, `persist.rs`). A wipe was "every player's committed death
  within 90 s", even when the first player had already reloaded and was alive again. Live
  2026-11-09: the host died, reloaded and played on for 79 s, the friend then died, and both
  sides cleared the dead set, the paid kills and the boss records (killed enemies and damaged
  minibosses would come back on the next load). Now a death stops counting once that player
  loaded after it and is alive (`death_over`: ours after our death reload, a peer's after its
  last hidden snapshot); checked after the wipe test, so a death that comes in during the other
  player's death reload is still a wipe. Log line: `persist: ... is back after ... death: it no
  longer counts for a wipe`.

## Base fixes ported from 1.0.11

The 1.0.11-beta source is not public. These were rebuilt from its release notes, its ini
comments, the log lines in its DLL and our players' 1.0.11 logs (catalogue:
`docs/FIXES-1.0.11-vs-FORK.md`, Top-10). Branch `base-fixes`. Every item is a unit-tested
rule plus game-side wiring; **none of them is verified live yet**: the oracle is a playtest
with the log lines below.

| Item | Setting (default) | Log lines to look for | Status |
|---|---|---|---|
| Per-run log files, F7 bug-report zip, crash zip (`log.rs`, `logzip.rs`) | `log_per_run` (1, next start), `send_logs_key` (118 = F7, 0 = off) | `log: this run also goes to ...logs\<date_time>.log`, `logzip: wrote N files ...`, `logzip: the previous run crashed / had no exit line: wrote ...` | unit tests (zip layout, CRC, names, pruning, password hidden); not run in game |
| Fresh-life deathblow veto, 10 s (`nsession.rs dblock`, `livesync.rs`) | `getup_deathblow_veto` (1) | `deathblow: enemy E netId N lost a life (seen here / the friend's deathblow): no new deathblow on it here for 10 s ...`, `deathblow: our attempt on N refused: ... (fresh-life veto)`, `fresh-life vetoes` in the 10 s stats | unit tests; uses the existing gate detour `0xBE3810` (live since 1.0.9) |
| Boss lives desync line (`livesync.rs`) | none (log only) | `livesync: DESYNC enemy ... for 15 s ...`, `... lives agree with the other players again after N s` | unit tests |
| Limited shop stock per player (`world.rs shop_counter_bits`) | `shop_stock_per_player` (1, next start) | `world: ... per player (... shop rows, 75 shop counter bits, ...)` | unit test with the real 1.06 rows; width fitted to 1.0.11's own count (75) [ARV] |
| Restores wait for a down peer (`persist.rs`) | `persist_wait_for_down` (1) | `persist: our death reload while peer(s) [..] are down: ...`, `persist: the down peer(s) settled N ms after our death reload: ...` | unit test for the end rule; builds on the false-wipe fix |
| Ledge-hang / wall-hug stealth (`enemies.rs`) | `stealth_hang_cover` (1) | `stealth: our throw state ... (hang/cover): enemies within 12 m run on our AI ...`, `throw: handoff ... reason hang/cover`, `stealth: claiming enemy ... (..., hang/cover, ...)` | unit tests; reads ThrowModule `+0x79` (HKS act 160, RE-BACKSTAB.md) [VAR in IDA, not read live by the fork yet] |
| Co-op items when the temple idol is lit (`items.rs`) | under `connection_items` | `items: the temple idol was just lit: giving the co-op items` | unit test for the edge |
| me3 file mod warning (`me3scan.rs`, `doctor.rs`) | none (log + notice only) | `doctor: me3 profile ...: N package(s) ...`, `doctor: WARNING me3 file mods differ: ...` | unit tests (parser, fingerprint, warnings) |

Notes:

- The per-run file is a second copy of every line; log.txt is unchanged (append-only, moved
  to log.old.txt at 8 MB), so `upload-log.ps1`, testsync and the existing instructions still
  work. Zips and run logs live in `%LOCALAPPDATA%\sekiro-coop\logs\` (20 runs, 10 zips kept).
- The veto refuses every throw of our player on that enemy at the gate, as the peer lock
  does; 1.0.11 used the same gate (its stats line counts both).
- The shop counter width (`eventFlag + 1 ..= eventFlag + floor(log2 sellQuantity)`) is
  inferred, not read in the game's shop code: it is the only simple width that reproduces
  1.0.11's "75 shop counter bits" with the same 1084 other per-player flags.
## Co-op items unlocked diving and every skill (`[DEBUG] fix_debug_item_unlocks`, default 1)

The co-op items sit on EquipParamGoods rows 999997 (Kindred Bell), 999998 (Kindred Charm) and
999999 (Severed Thread); 1.0.11 also uses 999996 (Rival's Charm). In vanilla these rows are
FromSoftware debug items: their `actionUnlockParamId` (+0x28) is their own id, and
ActionUnlockParam 999999 unlocks every action, 999998 all but action03 (Mortal Blade), 999997 all
but 03 and 06, 999996 all but 20-23. The owner of the items could dive without Mibu Breathing
(HKS `env(3033, 0)`), use the Mortal Blade, ninjutsu and the skill actions without having them.

How the game handles it (RE 2026-11-11, dump + Ghidra, details in `docs/RE-NOTES.md`):
EquipInventoryData (EquipGameData+0x168 = PlayerGameData+0x3E0) keeps 30 counters at +0x90, one
per action. `0x7A9A80` adds the item's quantity to every action of its unlock set when it is
added or removed (goods: +0x28, weapons/skills: EquipParamWeapon+0x24C), with the params of that
moment. `0x7A8200` zeroes the counters and adds the whole inventory again; the save loader
(`0x7A8140`, [ARV]) and the debug param reload (`0x10B2AB0`) call it. `0x7A7FE0` = "counter > 0 or bit in
+0x118" answers HKS `env(3033, n)` (case 0xBD9 of `0xB36410`). The mod rewrote our rows only
seconds after the save had loaded, so the counters already held the debug sets and kept them.
In the 2026-11-11 dump the counters matched "inventory x vanilla rows" exactly (action00 = 4: one
from each of 999996-999999, no Mibu Breathing owned).

Fix (`items.rs`): the ids stay (saves, the 1.0.11 Rival's Charm and upstream builds use them).
- +0x28 of rows 999996-999999 (and of the rows this run chose) is set to 0 as soon as
  EquipParamGoods is loaded, at the title before any save load; checked again every second.
  Log: `items: debug unlock rows, vanilla actionUnlockParamId: 999996=999996, ... -> 0 (zeroed,
  before the save load)`.
- After every save load (and whenever a row had to be zeroed again in the world) the game's own
  `0x7A8200` rebuilds the counters once (prologue bytes checked first). Owned items such as Mibu
  Breathing (2420), Mortal Blade (2400) or ninjutsu (2100/2110/2120 -> 2301) count as before;
  only the debug sets drop out. Log: `items: action unlocks rebuilt (debug unlocks removed):
  before [...] after [...]; locked again: [a00 dive (Mibu Breathing), a03 Mortal Blade, ...]`,
  or `items: action unlocks rebuilt, unchanged [...]` once the fix is in place.
- The row writer sets `actionUnlockParamId = 0` explicitly; `items: chosen rows, vanilla
  actionUnlockParamId: ...` lists the vanilla values.
- `fix_debug_item_unlocks = 0` turns the zeroing and the rebuild off (the old behaviour).

Existing saves need nothing: the save holds items, not counters. A player who learned to dive
only through this bug cannot dive any more after the fix until they get Mibu Breathing.

Note for upstream (Tihvert): 1.0.11 has the same bug on 999996-999999. The fix is the two parts
above: zero `actionUnlockParamId` of those rows before the save loads, then call
`0x7A8200(PlayerGameData + 0x278 + 0x168)` once in the world.

## Working on this fork

- Repository: https://github.com/ZeroPrin/sekiro-coop, branch `share-pickups` (the fork's
  default branch). `main` stays a copy of the original author's `main`.
- New work: a branch from `share-pickups`, then a pull request into `share-pickups`.
- Bump `MOD_VERSION` in `crates/coop-dll/src/ui.rs` (`1.0.9-pickups.4`, ...) with every build
  you give to other players: everybody in a session needs the same build.
- When the original author publishes newer source (1.0.10 / 1.0.11), the pickup commits can be
  rebased onto it (`git remote add upstream https://github.com/Tihvert/sekiro-coop`).

## Build

`build-public.cmd` (VS 2022 Build Tools with MSVC x64 and Windows SDK 10.0.26100, Rust
stable). Unit tests: `cargo test --release -p sekiro_coop --no-default-features --features dev-native`.
