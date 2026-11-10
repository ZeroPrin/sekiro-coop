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

Problem: in 1.0.9 only event flags travel. doors.rs replays a friend's door only for ObjAct
states that keep a flag (door / one-dir door / one-loop); most doors seen live are
`SprjObjActOneState` (no flag, EMEVD sets one), so the object the friend opened stays closed
here until a reload (IMPLEMENTATION-NOTES §37 "Risks"). The game's own type 45 replay only
runs with the native session engaged at stage 4.

Design (`objact.rs`):

- **Sender: a poll, not a detour.** Every 50 ms the WorldObjActMan walk (doors.rs layout)
  reads each loaded block's infos in one read (at most 512 per block, 2048 per poll). An info
  whose run state (`+0x54`) turns 3 is a new execution. It is judged at the next frame at
  least 150 ms later: ours when `info+114` went 0 -> 1 between the poll before the edge and
  that moment (the player's own path `0x6BA7B0` -> `0x6BC1B0` sets it; the replay `0x6BA910`
  sets `+113 = 1, +114 = 0`), or `+114` is still 1 from an earlier execution and our player
  started an `ObjActParam.playerAnimId` animation within 2 s (used for one send only). Our
  animation alone never sends: every execution in every loaded block would match (an EMEVD
  gate, the friend's type 45), and no object position is known for a proximity check. Never
  sent: our own `0x6BA910` replays (doors.rs and objact.rs mark them per ObjAct, a door's sub
  infos sharing their primary `+0xB0`: no echo), one-loop states unless `sync_objact = 2`, an
  unknown state class. The infos of one ObjAct go out once per 2 s. For a flag state the
  target is the opposite of the flag before the toggle. A detour on `0x6BC1B0` was not done:
  its signature and prologue are in no note here.
- **Message:** bulk kind `0xA7` (`BULK_OBJACT`), 25 bytes: magic `0x4F`, version 1, reason 1,
  state class, target state, then the block's **map id**, info index, sub index, ObjActParam
  id (-1: not located yet), sequence number. The block descriptor is a `WorldBlockInfo` [ARV]:
  `+0x20` (the slot doors.rs and `0x6BA910` use) is its `world_block_index` in the game's
  WorldInfo (vendor/sekiro `world_info.rs`), so it is probably the same on both machines; the
  map id (`+0x08`, checked against its area's number) is sent anyway because it describes the
  block by itself and is checked here. Sent to `Dest::All`: the host to every client, a client
  to the host, whose coop-net relays it to the other clients with the original sender.
  An older build logs `net: bulk transfer kind 167 received` for every message and keeps it
  in its 24-entry bulk inbox until newer transfers push it out.
- **Receiver** (game thread, 300 ms after arrival, retried for 5 s; at most 32 waiting, a
  drop is logged): the loaded block with that map id, the info by index / sub, then
  `replay_gate`: target state 0 / 1 only (a one-state only 1); not replayed by doors.rs or us
  in the last 3 s (doors.rs checks our marks too: one `0x6BA910` per ObjAct); at most one
  replay per ObjAct per load; not executed here since the load; its run state is the idle one
  it had when first seen after the load; class known; one-loop only with `sync_objact = 2`;
  never a door toggling closed (doors.rs keeps door flags on). Only then the object is looked
  at (`doors::object_view`, never for an unknown class): already at that execution's
  animation (playing or finished) = nothing. Else `0x6BA910(block, index, -1, state)`.
  Nothing runs in a Remnant memory.
- **Settings:** `sync_objact` 0 off, 1 on (default; one-loop states neither sent nor
  replayed), 2 also one-loop states (both players). `[DEBUG] objact_debug` (default 0): every
  execution, every map flag write, the raw info bytes (first 8 per load) and the first bytes
  of `0x6BC1B0` / `0x6BA7B0` / `0x6BA910`.

Log lines (prefix `objact:`):
- `loaded ObjAct blocks: slot S mAA_BB_CC_DD (0x...) (N infos), ...` once per load; a `?` map
  means the WorldBlockInfo reading is wrong.
- `ObjAct ran here: map ..., block slot, info index, sub, <class> state, flag F before B now
  A, info+113/+114 (+114 before the edge), object handle, object at start, our ObjAct
  animation ...; <why>: sending as #N | not sent: <why>` (every execution with `+114 = 1` or
  our animation near; all of them with `objact_debug`), then `sent our ObjAct #N ... ok`.
- `map flags written from 5 s before it until now: [...]` and `flag F = V (within 5 s after
  our ObjAct)`; with `objact_debug = 1` every `flag F = V` (8-digit ids: all map groups, the
  doors.rs groups 1/2 and the 611xxxxx door flags seen live included).
- `1 s after our ObjAct ...: run state, info+113/+114, flag, object anim (reached)`.
- Receiver: `peer P ran ObjAct #N: map ..., info index, sub, class, target state`, then
  `peer P's ObjAct #N here: ... run state R (idle ...); replayed with 0x6BA910(...) ok = ...`
  or `not replayed: <why>` or `no such loaded info here within 5 s`, then the same info
  `1 s after` / `3 s after` (only if the same block is still loaded).

Open questions:
- Actor -1 vs the friend's native replica handle: if the object animates but still blocks the
  way, try the replica (`REPLAY_ACTOR` in objact.rs, marked TODO).
- Whether the run state really returns to a different value after an execution (the idle
  check) and whether a finished door keeps its last animation (`reached`): first live log.
- Keyed doors (a key item check runs before the execution on the opener's side only: the
  receiver opens it without the key; probably fine for co-op, not checked).
- Lifts: no object position offset is known, so a player standing on a lift is not checked;
  one-loop states are off unless both set `sync_objact = 2`. Whether lifts are ObjActs at all
  is unknown.
- Chests: whether a chest is an ObjAct whose item node spawns on opening (see the pickups
  section) is unknown; the log will show their class.
- Whether `info+114` really marks the local player (vs NPC / script executions) and where the
  ObjActParam id and the actor sit in the info: read the `objact_debug` info dumps in IDA.

Recommended first live test: both players set `native_session_stage = 4` and
`objact_debug = 1` (both need this build), stand at a known one-state door in the same area,
one opens it; the other watches whether it swings open and walks through. Exchange both
`log.txt` files and compare the `objact:` lines (with stage 4 the receiver should mostly log
`not replayed: ... executing` / `executed here` / `already at that execution's animation`,
i.e. type 45 was first; then repeat with `native_session_stage = 3` to see our replay alone).

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
