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
