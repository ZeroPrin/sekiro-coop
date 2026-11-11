# RE notes: Sekiro 1.06 (1.6.0.0), Steam-verified exe, 68,005,144 bytes

Every entry: what, where, source, and how it was verified on this machine.

## Facts verified in-game (2026-10-22)

| What | Value | Source | Verified by |
|---|---|---|---|
| No Arxan | `dearxan::disabler: arxan detected: false` | me3 log | me3 launch |
| `.text` is encrypted on disk | Bytes at file offset for RVA 0x6C1A10 are random | SteamStub `.bind` section | Reading the exe; code is only readable at runtime |
| WorldChrMan global | RVA `0x3D7A1E0` | SekiroTool `Offsets.cs` | Matches cheat-menu log `WorldChrMan[0x143D7A1E0]`; player position read correctly |
| Player ChrIns | `[WorldChrMan] + 0x88` | SekiroTool | Position matches movement |
| ChrIns modules | `ChrIns + 0x1FF8` → table; `+0x18` data, `+0x68` physics | SekiroTool | HP and position read correctly |
| HP | data module `+0x130` | SekiroTool | Plausible values (wolf 28, soldier 233) |
| Position / angle | physics module `+0x80` (xyz, w at `+0x8C`) / `+0x74` | SekiroTool | Correct |
| Second position copy | `ChrIns + 0x1060` | Our F6 scan | Found 2 copies on a live wolf |
| `GetChrInsByEntityId` | RVA `0x6C1A10`, `ChrIns* f(const int* id)` | SekiroTool asm (`movabs rcx, &id`) | Passing the value crashed at `+0x6C1A29`; the pointer form works and finds 89 characters |
| Flag block | `ChrIns + 0x1F40`: byte0 bit5 NoHit, bit6 NoAttack, bit7 NoMove; byte1 bit4 FreezeAi | SekiroTool | FreezeAi stops a wolf visibly |
| **UpdateCoords store** | RVA `0xBC4796`, `movaps [rsi+0x80], xmm6` (`0F 29 B6 80 00 00 00`), `rsi` = physics module | SekiroTool noclip hook | Bytes match at runtime; our hook moves a live puppet visibly |
| **CSDebugChrCreator** | `WorldChrMan + 0x3148` (`.?AVCSDebugChrCreator@NS_SPRJ@@`); `+0x3150` CSDebugChrPerfChecker | Our F11 RTTI dump | RTTI name read live (full dump: `docs/rtti-dump-2026-10-22.log`) |
| Leftover session objects | `WorldChrManDbg + 0x158 -> PlayerSessionHolder -> +0x18 PlayerNullSession` | RTTI dump | Live |
| Entity id range | 7 digits, map-prefixed (e.g. 1100350–1100951 near the first idol) | Scan | 89 hits in 1,000,000–2,600,000 |

## Behaviour findings

- **Writing physics `+0x80` directly does nothing visible.** The game overwrites it later in the frame at UpdateCoords from Havok.
- **The hook alone is enough** to move a live character. An A/B test with and without a direct write made no difference.
- **FreezeAi + hook does not move the character, and costs a lot of FPS** (60 → 15). Keep AI on and use NoAttack + NoHit only.
- **Respawn reads the hooked position**: after the player's death the re-created wolf spawned at the hook target.
- **Cost of following**: about 2–3 FPS (57–58 → 55–58).
- A full position-copy scan (F6, ~68k `VirtualQuery` calls) stalls the game for about 10 s; it is fine for one-off RE only.

## Action unlocks (2026-11-11, dump `sekiro-1.06-ingame-2026-11-11.dmp` + Ghidra)

| What | Value | Verified by |
|---|---|---|
| ActionUnlockParam getter | `0x10B4F10(out*, id)`: `0x10CE0E0(repo, 0x74)` + binary search (only caller `0x10B4ED0`) | Decompile; repo index 0x74 = `ACTION_UNLOCK_PARAM_ST` in the dump |
| Unlock counters | EquipInventoryData `+0x90`, `i32[30]` (one per action00..29); EquipInventoryData = EquipGameData `+0x168` (`0x79EAC0`/`0x79EAD0`) = PlayerGameData `+0x3E0` | Constructor `0x7A6FF0` zeroes 0x78 bytes at +0x90; dump: counters == sum over the inventory of qty x unlock set with the VANILLA goods rows (exact match, 30 values) |
| Add/remove | `0x7A9A80(inv, u32* itemId, int delta)`: goods `row+0x28`, weapons (incl. skills 2000xx) `EquipParamWeapon+0x24C` -> ActionUnlockParam, `counter[i] += delta` per set action | Decompile; callers `0x7A7330`, `0x7A82D0`, `0x7A8F20`, `0x7A93F0`, `0x7A864C` |
| Rebuild | `0x7A8200(inv)`: zero +0x90..+0x108, then `0x7A9A80` for entries `0..=[inv+0x88]`. Prologue `40 55 56 41 56 48 83 EC 20 48 8D 81 08 01 00 00` | Disasm; callers: save deserializer `0x7A8140` [ARV: reads +0x88 and the entries from a stream, called through a vtable], debug param reload `0x10B2AB0` (`testdata:/interrootparam/`), `0x7A9BF0`, `0x79EAE0` |
| Check | `0x7A7FE0(inv, u8 action)`: `counter[a] > 0` or bit `a` of `inv+0x118` | Disasm; dump `+0x118` = 0 |
| HKS `env(3033, n)` | case 0xBD9 in `0xB36410`: `0x7A7FE0(PGD+0x278+0x168, arg2)` | Decompile |

The counters use the params of the moment an item is added or removed. Changing `+0x28` later
does not change them until `0x7A8200` runs.

## Not yet known / next

- `ForceAnimation` RVA `0x6D5A90`, `f(int entityId, int animId)` (SekiroTool snake loop: `ecx=1100850, edx=20000`). Not yet called by us.
- Making the puppet look like Wolf (`c0000`), via the debug chr creator or a player-model NPC.
- The puppet has no animations while moved (slides).
- Facing: the puppet's angle is not set yet (only xyz goes through the hook).
