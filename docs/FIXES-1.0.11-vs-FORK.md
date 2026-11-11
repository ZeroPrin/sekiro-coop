# Sekiro Together: 1.0.11 vs our 1.0.9 fork, upstream fixes and open bugs

Date 2026-11-11. Read-only research; this file is the only write. Fork = `E:\REPOS\sekiro-coop` branch `share-pickups`
(4e05881) + worktrees in `E:\REPOS\sekiro-coop-wt\`. Already done or in progress, NOT re-researched here (see
`docs/FORK-PLAN-2026-11-11.md`): ObjAct sync (`objact-sync`), loadout sync (`loadout-sync`), action-unlock fix
(`items-unlock-fix`), player packet rate (`pos-rate`), PvP (`pvp-native`).

Labels: **[VAR]** verified in a source (URL / file:line / DLL string offset), **[ARV]** inferred, **[Г]** guess.
"@3bd8d0" = FILE offset of a string in `E:\GAMES\SekiroTogether\sekiro-together\sekiro_coop.dll` (1.0.11-beta,
5,417,984 B). RVAs inside those strings are sekiro.exe 1.06 RVAs printed by the author's own log messages.
Raw material (scratchpad, not in the repo): `dll\s111.txt` (27.5k strings), `dll\new111.txt` (974 mod strings with no
counterpart in the 1.0.9 tree), `web\releases.json`, `web\issues.json`, `web\issues\N.comments.json`,
`logs\ranked.tsv`, `logs\only1011_checked.tsv`.

## 0. Version facts [VAR]

- **There is no 1.0.10 release.** Tags: v1.0.0 … v1.0.9-beta, then v1.0.11-beta (2026-11-06, prerelease, "Quiet beta.
  Not tested in co-op yet.") https://github.com/Tihvert/sekiro-coop/releases/tag/v1.0.11-beta . "1.0.10" survives only
  as a stale header in the 1.0.11 zip (`HOW-TO-PLAY.txt:1` "SEKIRO TOGETHER 1.0.10 beta: quick guide").
- The 1.0.9 code commit is `e11f68b`; `ff0129c` (our base) is upstream main tip = 1.0.9 code + docs/FUNDING only. Tag
  v1.0.11-beta points at docs commit `8aae59a`; no 1.0.11 code is public.
- Next upstream build is **1.0.12** (unreleased): the author hand-ported our PR #6 (false wipe) and PR #7
  (share_pickups) into it and fixed issue #4 (easy talk prompt) there. https://github.com/Tihvert/sekiro-coop/pull/6 ,
  https://github.com/Tihvert/sekiro-coop/pull/7 , https://github.com/Tihvert/sekiro-coop/issues/4
- Author on source: "The source in this repo is still 1.0.9, I'll push the newer code soon" (1.0.11 notes); "the source
  has moved on a lot since 1.0.9" (PR #7). No date.
- 1.0.11 source files revealed by panic-path strings and absent from our `crates/coop-dll/src/` [VAR]: `objsync.rs`,
  `cutskip.rs`, `pvp.rs`, `drop.rs`, `getuptrace.rs`, `logzip.rs`. Further 1.0.11-only log prefixes (modules without a
  panic site): `paramtweaks:`, `idoltools:`, `chrguard:`, `phases:`, `flagcheck:`, `combat:`, `guard:` (packet guard),
  `bellresync:`, `wipejoin:`, `quit:`.
- 27 ini keys of 1.0.11 are absent from our `settings.rs` [VAR]: `sync_objects sync_breakables getup_deathblow_veto
  getup_watch stealth_hang_cover talk_lock story_units story_choices_host_only story_branch_own cutscene_skip_sync
  speffect_sync packet_guard friendly_fire pvp_hit_action pvp_team pvp_team_self enemy_damage_scaling hide_tutorials
  hide_item_popups easy_talk_prompt idol_extras item_drop item_drop_key item_drop_pad native_tabs send_logs_key
  cjk_names`. Everything else in the 1.0.11 ini (spectate_*, death_refill, boss_persist_on_death, restore_lost_items,
  native_lives_sync, puppet_type5_guard, clear_stale_no_revive, stealth_window_m, freeze_skip_driver_threads,
  native_mod_menu_inworld, ns_* …) already exists in 1.0.9. The ini's version tags (v8.43 optmenu pointer fix, v8.61
  driver threads, v7.7 ns_*) are all already in 1.0.9 (`optmenu.rs:353` "was truncated to 32 bit until v8.42").

## 1. What 1.0.11 fixed / added relative to 1.0.9

**Fork?** = present in share-pickups or a worktree branch. **Hit?** = seen in our players' logs (section 3).
Effort S ≤ 1 day, M 2–4 days, L ≥ 1 week. "1.0.11 DLL decompile" = load `sekiro_coop.dll` into a separate small Ghidra
project (Rust, no symbols), go string → xref → function, and lift the hook prologue, struct offsets or constant tables.
That is the efficient route whenever the author already printed the RVA (rows F1, F2, F7, F8, F18, F19).

| # | Fix / feature | Ver | Evidence | Fork? | Hit? | Reimplementation idea · effort · RE needed |
|---|---|---|---|---|---|---|
| F1 | **Levers, lifts, doors (incl. key doors and doors with no saved state) move for everyone** (`sync_objects`) | 1.0.11 | Release note [VAR]; ini comment; DLL `objsync ObjAct start (0x6BA7B0)` @3bd8d0, `objsync: we used ObjAct block slot … index … (class, flag, state, anim, caller` @3bd9fb; skip reasons @3bd81d: "object not here / already used up here / still running here / already in that state here / we used it ourselves just now / a different ObjAct here / not loaded here"; `native session stage 4 replays ObjActs itself, ours not sent` @3bd950 | Partly: `objact-sync` (polls WorldObjActMan, replays via `0x6BA910`, bulk 0xA7), not live-tested | Yes: 283 "door flag N is on but its door was closed here: opened" after load/rest (1.0.11 83 / p1 13 / p3 187), `L\log.old.txt:326`; 1.0.11 logged 1001 `objsync:` lines | **1.0.11 detours `0x6BA7B0` (local ObjAct start) directly** [VAR], so that detour is proven safe. Move the `objact.rs` sender to a `0x6BA7B0` detour and keep polling as a fallback; copy the skip-reason taxonomy into `objact:` lines. Upstream still has "doors closing again after an area reload" open (issue #3, see §2). **M**. RE: decompile the 1.0.11 objsync hook (string @3bd8d0 → xref) to lift the prologue bytes and the req fields it reads (+0x0C actor, +0x20 sub per FORK-PLAN). |
| F2 | **Breakable crates and fences break for everyone** (`sync_breakables`) | 1.0.11 | Release note; ini; DLL `objsync break queue (0x8975B0)` @3bd8f6, `objsync: we broke object … of block …` @3bda70, `: broken here (` / `not broken here (` @3bcf40 | **No** (no break handling in `objact-sync/…/objact.rs`) | 1.0.11 logs show "we broke object N of block X: sent to the others" [VAR]; fork: cosmetic desync [ARV] | Detour `0x8975B0` (break queue push) on the breaker's side → send `{map block id, object index}` on the objact bulk kind; the receiver pushes the same object into the queue if it is still intact. **M**. RE: `0x8975B0` signature in `E:\RE\ghidra_quick` + arg order from the 1.0.11 DLL; breakables list from `E:\RE\sekiro-data\map\msb_json`. |
| F3 | **A two-life miniboss can't be deathblown twice in a row** (`getup_deathblow_veto`, 10 s) | 1.0.11 | Release note; ini; DLL `deathblow: our attempt on … refused: it lost a life less than … s ago and gets up (fresh-life veto)` @427955, `deathblow: 10 s: locks ours` | **No** (`livesync.rs:197-226` has `own_getup_due` / `getup_guard` for life mirroring, no veto) | Yes: boss lives differ 7× and posture 11× in audit (p3); 1.0.11 logged the veto 6× | Stamp `life_lost_at[netId]` on every own or received life loss; refuse our deathblow on that netId for 10 s, before the throw starts (the fork already has a deathblow lock: "deathblow: enemy X locked by peer N", 36× on p3). **M**. RE: the point where a local deathblow start can be cancelled. Xref "fresh-life veto" in the 1.0.11 DLL to find their hook. `getuptrace.rs` (`getup_watch`) is a hardware-breakpoint diagnostic, not needed. |
| F4 | **Boss came back with old HP after everyone died** | 1.0.11 | Release note; DLL `… are down: boss restores and dead-set kills wait until they get up (lone death) or die too (wipe), at most` @42def5, `persist: the down peer(s) settled` @42df66 | Partly: our 4e05881 (false wipe; PR #6, confirmed by the author, ported into 1.0.12) | 6 wipes on p3; boss HP differs 4× in audit | While any peer is on a death screen, hold `persist` restores and dead-set kills until that peer revives (lone death) or dies too (wipe), capped at 30 s. Author: "1.0.11 fixes one case … not sure if it's yours" (issue #3). **M**, no RE: state machine in `persist.rs:91-170` + `bossrules.rs`. |
| F5 | **Stealth deathblows from a ledge hang / wall-corner cover work for both** (`stealth_hang_cover`, 12 m) | 1.0.11 | Release note; ini | **No** (`enemies.rs:1372-1456` covers rear/plunge approaches only) | [Г] not visible in logs | Add a third approach kind: while the local player hangs or is in cover, hand every enemy within 12 m to our AI / native claim (same path as `approach_kind`). Detection: HKS `act(160, THROWABLE_STATE_ATK_PC_HANG / _COVER_LOOK_L / _R)` (`E:\RE\sekiro-data\hks\c0000.lua:566-573`). **S–M**. RE: the runtime field that holds that throwable state (env/act 160 handler, or the hkb state ids from `c0000_cmsg.lua`). |
| F6 | **Shops: limited items keep each player's own stock** | 1.0.11 | Release note; DLL `world: … item/shop/skill/tutorial flags are per player (… item lots, … shop rows, … shop counter bits, … skill flags, … tutorial flags)` @4328ee | **Partly**: `world.rs:1499-1590` excludes `ShopLineupParam.eventFlag` (and release flags) only, not the **counter bits** | [Г] | Exclude each limited row's (`sellQuantity > 0`) whole stock-counter flag range from sync, not just `eventFlag`. **S**. RE: confirm how the count is stored (`E:\RE\sekiro-data\params\csv\ShopLineupParam.csv`: `eventFlag` base + N bits?), or lift the loop from the 1.0.11 DLL (xref "shop counter bits"). |
| F7 | **Crash guard after a cutscene when you went to another area alone** ("seen on Linux") | 1.0.11 | Release note; DLL `chrguard: character getter (0x9B5D00)` @3d52f6, `chrguard: 0x9B5D00 found no entry (table, index, entry …): answered false instead of the crash at +0x9b5d17` @3d5260 | **No** (0x9B5D00 is nowhere in the tree) | Related: issue #3 "peer crash on level transfer at *Three years ago*". Our only logged crash is a different RVA (§3, C1) | Detour `0x9B5D00`: repeat its table lookup with a null/range check and return false instead of dereferencing a missing entry. Author: the guard "only catches one case, yours is a different one", so widen it to the other callers. **S** for the guard, **M** to widen. RE: decompile `0x9B5D00` (dump project) + the 1.0.11 patch. |
| F8 | **Story phases follow the host** (Sunken Valley 8303/8307, burning castle, Kuro's Charm), Dragonrot cure per player, **only the host answers the Owl's Iron Code** | 1.0.11 | Release note; ini (`story_units`, `story_choices_host_only`, `story_branch_own`); DLL `phases:` @430520–4307d1 ("phase unit flag … from our script held with phase", "Owl's choice flags put back to the host's values", constants "common 910", "m11_00 11105631"), `talk SetEventFlag range check (0x11CA830)` @43823e | **No** | Indirect: audit "flag sync missed" 75× (39 in 11105xxx), `doctor: WARNING Dragonrot differs` 2× (`RE\log.txt:25842`) | Detour `0x11CA830` (talk ESD SetEventFlag) on the joiner to drop writes to the Owl-choice flags; group phase flags into units and apply/hold them atomically. **L**. RE: EMEVD in `E:\RE\sekiro-data\event\js`; the phase/unit tables are constants in the 1.0.11 DLL, so decompiling is the efficient route. |
| F9 | Talk lock gets a setting (`talk_lock`) and an **ESD condition hook** | 1.0.11 (lock existed in 1.0.9) | Release note; DLL `talk ESD condition (0x11C1A20)` @43483a, `talklock: ORDERING FAILED (no hook)`, talk ins `0x1411C9D80` | Partly: `talklock.rs` press hook `0xA775B0` (`talklock.rs:60,359`), no setting key, no ESD hook | – | Add the `0x11C1A20` condition hook so a locked NPC's prompt never opens (the press hook can fire too late). **S–M**. RE: prologue of `0x11C1A20`. |
| F10 | Co-op items are given when the temple idol is **lit**, no rest | 1.0.11 | Release note; DLL `items: the temple idol was just lit: giving the co-op items` @42ffaa | No (`items.rs:227` "rest at an idol to receive the co-op items") | – | Grant on the 0→1 edge of the temple-idol unlock flag (already read in `items.rs:708`). **S**, no RE. |
| F11 | **Warning when players run different me3 file mods** | 1.0.11 | Release note; DLL `doctor: me3 file mod scan`, "Different file mods (you: …", "Same file mods but different files (versions differ): the worlds can go out of sync." @42718c | No | **Yes**: the friend has the `ps5-glyphs` me3 mod (`L\logs\2026-11-08_02-11-08.log:6792`) | `doctor.rs`: read the me3 profile's package list + file sizes, send a hash in the doctor info, warn on mismatch. **S**. |
| F12 | **One log per game start; F7 zips all logs** (`send_logs_key`) | 1.0.11 | Release note; DLL `logzip.rs`, "Logs packed for a bug report", `logzip: wrote … files` @3d5100 | No (`log.rs:20-36`: log.txt → log.old.txt only) | **Yes**: the fork run of 11-09 ~02:00–03:19 ended without an exit line and its log was overwritten (`L\log.old.txt:5`) | Write `logs\<date_time>.log` per run (keep the last 20) and keep log.txt as a copy/link; F7 = store-only zip of logs + audit + ini. **S**. |
| F13 | **Mod Settings with categories** in Options (`native_tabs`) | 1.0.11 | Release note; ini; DLL category strings @428a61–42a5b0 | Partly: 1.0.9 `optmenu.rs` single page | – | Cosmetic, **L**. Skip. |
| F14 | QoL param tweaks: hide tutorials, hide item pop-ups, easier talk prompt, **enemy damage %** | 1.0.11 | Release note; ini; DLL `paramtweaks:` @3d4750–3d4929 (MenuTutorialParam, EquipParamGoods rows, ActionButtonParam talk rows, AtkParam_Npc rows scaled) | No | 1.0.11 failed on our game: "paramtweaks: holder 130 is not MenuTutorialParam (game version?), hide tutorials not applied" (`E:\RE\logs-2026-11-09\log.txt:157`) | Param writes at load time (same machinery as the `items.rs` row edits). **S** each. **Don't copy easy_talk_prompt as is**: it sets the talk angle to 360 and breaks all NPC talk (issue #4). Enemy damage = scale `AtkParam_Npc` attack fields by the host's %. |
| F15 | **Friendly fire + Rival's Charm duel** (`friendly_fire`, `pvp_hit_action=2`, `pvp_team=29`, `pvp_team_self=1`) | 1.0.11 | Release note; ini; DLL `pvp.rs`, `pvp: hit table …`, "Duel! To the death." @437600 | In progress: `pvp-native` (bc5ac93) | **Yes, the crash**: our only logged crash (1.0.11, 11-08 02:38) came 75 s after `pvp: on (friendly fire), team 29 (us and the others), hit action 2` (`2026-11-08_02-11-08.log:6724`), on our own death (§3 C1) | Covered by FORK-PLAN §B. The log supports hypothesis H1 there (team 29 on the LOCAL player + death → crash), and our plan already keeps the local team untouched. Crash RVA `sekiro.exe+0x66CE96` is the B0 target. |
| F16 | **Drop items to friends** from the inventory (`item_drop`) + fix "Drop didn't react in Inventory > Items" | 1.0.11 | Release note; ini; DLL `drop.rs`, hooks `GaitemSelectMenu refresh (0xDC7E70)`, `dtor (0xDBA310)`, `InventoryUnit update (0xCE79B0)` @425e2b | No (we share map pickups instead) | – | Low value next to share_pickups. **L**. Known 1.0.11 issues: one item per press, the list doesn't refresh, the dropped item shows only a glow. |
| F17 | Idol extras: upgrade/fit prosthetics, Gourd Seeds, send the Bell Demon away (`idol_extras`) | 1.0.11 | Release note; DLL `idoltools:` hooks `idol skill tree (0x11BB8B0)`, `menu alive check (0x11BAEC0)`, `idol menu (0x11BB0E0)`, `talk dispatcher (0x11C63A0)`; Bell release = flag 9830 off @439b63 | No | – | QoL, **M–L**. The Bell Demon release alone is **S** (flag 9830 off + banner 15). |
| F18 | Cutscene skip vote; a boss cutscene starts for everyone within 60 m (`cutscene_skip_sync`) | 1.0.11 (author: "NOT tested in the game yet") | ini; DLL `cutskip.rs`, hooks `skip bit (0xECD380)`, `PlayCutsceneAndWarpPlayer (0x67B760)`, `PlayCutscene 2002[13] (0x67BB90)` @3cd6c2 | No (`cutscene.rs` exists for other things) | off in all our runs | **M**. Only if players complain about desynced boss intros. The RVAs are known; only a prologue check is needed. |
| F19 | Status buildup (burn/poison/shock) on enemies owned by the other machine (`speffect_sync`, "osa9") | 1.0.11 (off by default) | ini; DLL `SpEffect gate (0x9E9D10)`, `speffect: gate refused id … on owned enemy … from remote source` @3cd76b | No | – | Detour the gate `0x9E9D10`: when the game refuses a remote-source SpEffect on an enemy we own, apply it. **M**, low priority (off upstream). |
| F20 | Packet guard: drop malformed native packets, kick after 20 in 10 s (`packet_guard`) | 1.0.11 (off by default) | ini; DLL `guard: soft rule: type …` @434081, `net: the host sends malformed game packets (dropped)` @3bfb38 | No | – | Robustness against untrusted peers only. **M**, skip. |
| F21 | `connection-test.exe` (firewall / UDP checker) | 1.0.11 | Release note; file in the 1.0.11 install | No | – | Standalone exe, independent of the DLL [ARV]: can ship next to the fork unchanged (LICENSE.txt in the 1.0.11 zip: MIT, Tihvert). **S**, no code. |
| F22 | Kindred Bell while connected = resync (both rest); greyed-out Quit Game; wipe → join via menu | 1.0.11 | DLL `bellresync: confirmed with the Kindred Bell` @3be9cb, `quit: Quit Game shown greyed out while in co-op` @3d5ce9, `wipejoin: going to the friend (menu)` @4351d0 | Partly (1.0.9 `resync.rs` F3 hotkey, `quitblock.rs` cancel+notice) | – | Small UX, **S** each. |
| F23 | Diagnostics: `enemies: HP mismatch: dead here / alive on host` @3bc38b, `flagcheck: we sync, the game would not / the game would sync, we do not` @426889, `combat:` per-enemy trace, `clock: peer's clock is N s behind` | 1.0.11 | DLL strings; 1.0.11 logs (376 `combat:` lines) | No | Boss desync is our most visible symptom (§3 P2) | The HP-mismatch line and `flagcheck` are cheap and directly useful for §3 P2/P3. **S**. |
| F24 | `cjk_names`, other cosmetics | 1.0.11 | ini | No | – | Not relevant for our players. |
| F25 | 1.0.12 (unreleased): easy-talk fix, PR #6 / #7 ported | 1.0.12 | issue #4, PR #6/#7 replies | #6/#7 are ours natively | – | When 1.0.12 source appears, diff `persist.rs` and their pickup check inside the `0x91FCF0` drop hook. |

## 2. Open upstream bugs (not fixed even in 1.0.11)

Sources: https://github.com/Tihvert/sekiro-coop/issues (7 items incl. 2 PRs) and the README section
https://github.com/Tihvert/sekiro-coop#whats-broken--not-done (not updated after 1.0.9). Nexus mod
https://www.nexusmods.com/sekiro/mods/2944 returned 403 on every tab (description, posts, logs, bugs), **not read**.
No Reddit threads about this mod found (search only hits LukeYui's older "Sekiro Online"). Discord not reachable.

| Bug | Source | Author's comment [VAR] | Our take |
|---|---|---|---|
| Doors one player opens stay shut for the other; **Ashina Estate doors close again after an area reload** | issue #3 | object sync in 1.0.11 should open them for the other player; "doors closing again after an area reload is not fixed yet" | `objact-sync` covers the first half. The second half: OneState doors have no saved flag (FORK-PLAN §A), so replayed ObjActs are lost on reload → keep a session ledger of executed ObjActs (map id + ObjActEntityID from `E:\RE\sekiro-data\map\objacts.md`) and re-apply on block load. **M**, high impact. Our logs show the flagged-door half: 283 reopen fixes (§3 P4). |
| Peer crash on level transfer at the "Three years ago" screen | issue #3 | crash in a character lookup after the cutscene; the 1.0.11 guard (0x9B5D00) "only catches one case, yours is a different one. i'll try to widen it" | F7. Our players did not hit it. |
| Boss not healed after a full wipe | issue #3 | "1.0.11 fixes one case … not sure if it's yours" | F4. |
| Palanquin serpent ignores player 2 (targets only the hidden host) | issue #3 | "scripted around the host … probably not possible" | Agree: EMEVD-driven. Skip. |
| Black screen / no control when joining via the idol into a running Demon of Hatred fight | issue #2 | not reproduced; use the 1.0.8 Esc watchdog (`stuck.rs`) | We have `stuck.rs` and `boss_start_gate`. Watch for `stuck:` lines. |
| After an Isshin wipe the idol door and Fountainhead gate are shut, Genichiro cutscene skipped, no boss spawn | issue #2 | the 1.0.8 own-world write-back "probably" fixes it, "not 100% sure" | 1.0.8 code is in our tree. Still open. Our logs: "own world: record N … NOT restored, kept as it is" 45× on p3, the same 5 records each time (71128000, 61120000, 21106000, 21105000, 11127000). |
| Cannot talk to NPCs at all (1.0.11 regression) | issue #4 | `easy_talk_prompt` sets the angle to 360 → fixed in the next build; workaround `easy_talk_prompt = 0` | Only matters if we port F14. |
| me3 crashes at launch | issue #5 | me3 bug in `me3_mod_host.dll` (also with DS3) | Not ours. |
| README "What's broken": Resurrect sometimes not offered; spectate sometimes black screen; enemies/bosses desync now and then; host lag/stutter spikes; puppets die instantly (fix untested); 3–6 players untested; lost flags → own-world restore misses story/NPC stuff; "the game can crash" | README | – | None of these is named in the 1.0.11 notes except levers/lifts/breakables and story phases, so they are probably still open in 1.0.11 [ARV]. Our logs confirm three of them: P2 (boss desync), P3 (flags), P6 (Resurrect not offered). |
| 1.0.11 Drop: one item per press, list not refreshed, model missing | 1.0.11 notes | – | Irrelevant unless F16. |
| Debug ActionUnlock rows 999996–999999 on the co-op items (dive / ninjutsu without unlocking) | our finding (FORK-PLAN "Ныряние"); the 1.0.11 DLL still picks the "unnamed unused goods rows (highest" @432600 | not reported upstream | 1.0.11 almost certainly has the same bug [ARV]. Fixed in `items-unlock-fix`. Worth reporting to Tihvert. |

## 3. Recurring problems in our players' logs

Logs (all from our machine; the friend is the other player): 7 files, 112,785 lines, 8 runs. 1.0.11-beta runs
11-08 02:06 – 11-09 00:39 (`%LOCALAPPDATA%\sekiro-coop\logs\2026-11-08_*.log`, `E:\RE\logs-2026-11-09\log.txt`
lines 1–25227); fork **1.0.9-pickups.1** 11-09 01:10–01:20 (`RE\log.txt` 25228–28404); **pickups.3** 11-09 03:19–03:45
and 11-11 02:35–05:28 (`L\log.old.txt`), plus `L\log.txt` (solo title screen). Counts are 1.0.11 / p1 / p3.

| # | Problem | Count | Example | Map to §1 | Severity |
|---|---|---|---|---|---|
| C1 | **Crash** `0xc0000005 at sekiro.exe+0x66ce96 (read of 0x10)`, first dead frame of OUR death while hosting, friendly fire ON with team 29 on us | 1 / 0 / 0 | `L\logs\2026-11-08_02-11-08.log:7223-7225`; stack `+0x68ae6a +0x67ff40 +0x67d1f8 +0x811d2d +0xa74c62 +0xa75885 +0xa6e0b9 +0x66eb01 +0xec6b7b` | F15 (PvP, H1 in FORK-PLAN §B) | high |
| C2 | Fork run with no exit line, log overwritten (crash or Task Manager), 11-09 ~02:00–03:19 | 0 / 0 / 1 | `L\log.old.txt:5`, `:261` ("unfinished session … restoring") | F12 (keep logs per run) | high (no evidence left) |
| P2 | **Boss / miniboss HP, lives or posture differ between machines** (audit) | 11 / 0 / 25 | `2026-11-08_02-40-29.log:9666` boss 1100300 dead here, 2 lives on the friend's side for 202.9 s; p3: 1000590 max HP 6380 vs 3190; 1110504 HP+lives differ 4×; 1700200 friend at 1 HP vs 100 % | F3, F4, F23 | **highest visible** |
| P3 | **Shared flag missed by sync** ("flag sync missed it") | 80 / 2 / 82 (audit file: 75 + 10 NPC-talk) | `2026-11-08_02-11-08.log:1651`; ranges 11105xxx 39, 21705xxx 10, 11705xxx 7 | F8 (phases), F23 (`flagcheck`) | medium–high |
| P3b | **Flag ping-pongs** between the players (a per-player flag treated as shared) | ~10 / 0 / 40 (audit 37, 26 in 11125xxx) | `RE\log.txt:1456` flag 21104018 | F6/F8-style per-player exclusion | medium |
| P4 | Door flag on but the door loaded closed → reopened by `doors.rs` (seen as a briefly closed door) | 83 / 13 / 187 | `L\log.old.txt:326` | F1, §2 doors-after-reload | medium |
| P5 | **POSSIBLE DOUBLE pickup** (fork only) | – / 1 / 17 | `L\log.old.txt:34341`: both players picked lot 1700070 in the same second (a real race); the other 16 point at friend pickups minutes old (lot 1700140 at 04:02, alerts until 04:24), i.e. a detector without a time window | fork share_pickups (`keyitems.rs:2108`) | medium (possible dupe) |
| P6 | **Resurrect NOT offered** (SpEffect 110093 without 110090) | 18 / 1 / 15 | `2026-11-08_02-11-08.log:2792` (`statelog.rs:303`) | README known bug; `clear_stale_no_revive` exists in both | medium |
| P7 | Own-world records NOT restored, the same 5 records every time | 33 / 18 / 45 | `2026-11-08_02-11-08.log:6057` record 11100000 | issue #2 (Isshin doors) | medium |
| P8 | Inventory divergence without a lot explanation (prosthetic tools, Healing Gourd 3001, goods 910000) | 8 / 2 / 31 | `L\log.old.txt:1274` friend has 660600 | share_key_items | low–medium |
| P9 | HP check on engage: lower HPs not taken, "scaling not settled" | 18 / 1 / 11 | `2026-11-08_02-11-08.log:664` (`nsession.rs:5118`) | P2 (max HP 6380 vs 3190) | medium |
| P10 | Native inbox drops stale packets (12–24 %) | 981 / 32 / 1039 lines | `2026-11-08_02-11-08.log:828` | `pos-rate` (already planned) | low (same in both versions) |
| P11 | Disengage / handshake timeout after a boss wipe ("no ownership list (23) within 5 s") | 0 / 0 / 1; Steam link drops 4× total | `L\log.old.txt:31808` | – | low–medium |
| P12 | `native: CharaInitParam 1080100 missing, using first existing row 9000` | 53 / 4 / 48 | `2026-11-08_02-11-08.log:483` | `loadout-sync` / appearance backlog | low |
| P13 | Stalls: one 1.5–1.6 s at every start, others 400–700 ms in driver/ntdll | 26 / 5 / 23 | `L\log.old.txt:119` | – | low |
| P14 | Config mismatches: me3 file mods (friend has `ps5-glyphs`), friendly_fire 0 vs 1, Dragonrot differs | 4+1 / 2 / – | `2026-11-08_02-11-08.log:6792`, `02-40-29.log:589`, `RE\log.txt:25842` | F11, F8 | low |

1.0.11-only problems seen live [VAR]: the C1 crash; `paramtweaks` hide-tutorials failed on our game build;
`world: flag ring overflow … ring lost 66662` after the role switch to host; `rest: … respawn NOT seen
(AllChrRespawn …)` with 4 enemies left dead (`02-40-29.log:9775`); `bossgate: … 246 held start flags dropped`.
So 1.0.11 is not strictly better than our fork: same boss desync and flag misses, plus its own regressions.

## 4. Top 10 next fixes for the fork (player impact × low effort)

1. **Per-run logs + F7 zip (F12, S).** C2 lost the only evidence of a fork crash. `log.rs:20-36`: instead of renaming
   to log.old.txt, open `logs\YYYY-MM-DD_HH-MM-SS.log` per run, keep the last 20, and write log.txt as before for
   tooling. Hotkey (next to the existing F11 handling) → store-only zip of the newest 3 logs +
   audit.txt + ini (redact `password`) into `%LOCALAPPDATA%\sekiro-coop\bugreport-*.zip`; the path goes on screen via `ui.rs`.
2. **Pickup double detector + race (P5, S).** `keyitems.rs:~2108`: bound the matcher to grants within ~3 s of the
   friend's pickup and the same goods id; compare against the node-lot flag (the idea already exists in the
   `pickups-fixes` worktree, cherry-pick only the detector). For the same-second race: when the friend's pickup message
   for a lot arrives after our own pickup of the same lot, the host decides by timestamp/slot and the loser's
   `GrantItems` is undone, or at least logged as a confirmed dupe.
3. **Fresh-life deathblow veto (F3, M; P2).** `livesync.rs`: record `Instant` per netId when lives drop (own or
   received). In the deathblow start path (the code that logs "deathblow: enemy X locked by peer", nsession/livesync),
   refuse our throw for 10 s after a life loss and log `deathblow: … refused (fresh-life veto)`. Add the 1.0.11-style
   `enemies: HP mismatch: dead here / alive on host` line to the 30 s stats so P2 has a cause next time.
4. **Ping-pong and missed flags triage (P3/P3b, S–M).** `world.rs:197 should_sync`: take the 37 ping-pong ids and the
   75 missed ids from `audit.txt`; add per-player ranges (11125xxx looks per-player [Г]) to the per-player set next to
   the item/shop flags (`world.rs:1499-1620`); add a `flagcheck:` log that compares `should_sync` with the game's own
   sync range (1.0.11 F23) to find the 11105xxx misses.
5. **Shop counter bits (F6, S).** `world.rs:1520-1560`: for each `SHOP_LINEUP_PARAM` row with `sellQuantity > 0`, add
   the whole counter-bit range starting at `eventFlag` (width from `sellQuantity`; confirm in ShopLineupParam.csv / the
   1.0.11 DLL) to the per-player set; extend the `world: … per player` log line with "shop counter bits".
6. **ObjAct start detour + reload ledger (F1 + §2 doors, M).** In `objact-sync`: replace polling in `objact.rs` with a
   detour on `0x6BA7B0` (lift its prologue from the 1.0.11 DLL or the dump), keep polling as a fallback. Keep
   `HashMap<(map_id, objact_entity_id), state>` for the session; on block load, re-apply entries whose object is still
   closed (`looks_open() == Some(false)`) via `0x6BA910`. Covers the "doors close again after reload" bug that 1.0.11
   still has.
7. **Persist waits for down peers (F4, M).** `persist.rs:91-170`: when our death reload happens while a peer is on a
   death screen, queue restores/dead-set kills; release on that peer's revive (lone death) or EV_DEATH (wipe), max
   30 s; log `persist: the down peer(s) settled`. Builds on 4e05881.
8. **Crash guard 0x9B5D00 (F7, S).** New `chrguard.rs`: detour `0x9B5D00` (verify prologue bytes first, `hook.rs`
   `install_detour`), do the table/index bounds check, return false for a missing entry and log once per entry. Then
   check the C1 RVA `0x66CE96` in `E:\RE\ghidra_quick` (FORK-PLAN B0): if it is a similar getter on the killer/attacker,
   guard it the same way.
9. **Stealth from hang / cover (F5, S–M).** `enemies.rs:1390-1456`: add `ApproachKind::Cover` when the local player is in
   hang/cover (THROWABLE_STATE_ATK_PC_HANG / COVER_LOOK_*, `c0000.lua:566-573`); radius 12 m; same claim/linger as
   rear approaches; setting `stealth_hang_cover` in `settings.rs`.
10. **me3 file-mod check + temple-idol items (F11 + F10, S).** `doctor.rs`: hash the me3 profile's `[[packages]]` /
    file list next to the DLL (`sekiro-coop.me3`), add a `filemods` field to the info and a WARNING on mismatch (the
    friend runs `ps5-glyphs`). `items.rs:708`: give the co-op items on the temple-idol flag 0→1 edge, not only on rest.

Not in the top 10, but next: Resurrect-not-offered (P6) investigation in `statelog.rs:303` / `resurrect.rs`; story
phases (F8, L, needs a DLL decompile of the phase tables); breakables (F2, M); talk-lock ESD hook (F9).
