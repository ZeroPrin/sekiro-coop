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
