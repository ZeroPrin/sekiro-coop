# Karma: art assets

Karma uses two custom images. Both are placeholders for now: a kanji 業 drawn by code (`tools/icons/make_karma_placeholders.py`). Replace them with final art as described below.

No game file is replaced. The DLL repaints two **existing** vanilla icon cells in the menu atlases while the game loads them (see IMPLEMENTATION-NOTES §21). So each image must fit the size of the cell it replaces.

## 1. Item icon (inventory, quick-item slot, pickup popup)

| | |
|---|---|
| Icon id | `172` (EquipParamGoods `iconId`), cell `MENU_ItemIcon_00172` |
| Vanilla art in that slot | a red gourd variant that no item uses. It shows if the patch is off. |
| Hi cell | `SB_Icon` (4096x2048), rect x=2816 y=512, **252 x 252 px** |
| Low cell | `SB_Icon` (2048x2048), rect x=780 y=130, **126 x 126 px**. It is made from the hi image by the script. |
| Source file | `tools/icons/design/karma-item.png` |
| Size to deliver | square. 252x252 or larger (for example 1024x1024). It is resized to 252 and 126. A non-square image is centred on a transparent square. |
| Safe margin | Vanilla item art leaves about 5 to 14 px clear at 252 px (median), and glows often reach the edge. Keep the main shape at least 8 px (3%) inside the edge. |
| On screen | The menus define the image as 252x252 and scale it down per menu (inventory grid, quick slot, pickup popup). The exact on-screen sizes were not measured. |

## 2. HUD status icon (under the HP bar while the Karma effect is active)

| | |
|---|---|
| Icon id | `2150` (SpEffectParam `iconId`), cell `MENU_StatusIcon_02150` |
| Vanilla art in that slot | "poison resistance down" (毒 with a green arrow). No SpEffect uses it in vanilla. |
| Hi cell | `SB_Status` (512x512), rect x=0 y=0, **92 x 90 px** (width x height) |
| Low cell | `SB_Status` (256x512), rect x=0 y=0, **46 x 45 px**. It is made from the hi image by the script. |
| Source file | `tools/icons/design/karma-status.png` |
| Size to deliver | exactly 92x90, or the same aspect ratio larger (for example 368x360). |
| Safe margin | Vanilla status icons keep their art 1 to 4 px from the cell edge (the replaced cell: 3 px on every side). Keep at least 3 px clear. |
| Style | Vanilla: a kanji with a dark outline over a soft coloured cloud or aura, sometimes with a small up or down arrow. It must read at small size. |
| On screen | The HUD (`menu/01_000_fe.gfx`) defines the image as 92x90 units on its 1920x1080 stage. Every parent clip has an identity transform, so it draws at about 92x90 at 1080p unless the HUD code scales it. This was read from the file and not checked in game. The same icon also shows in the pause-menu status list (`02_200_mainmenu.gfx`). |

## Format for both images

- PNG, RGBA, **straight (not premultiplied) alpha**. Vanilla atlases are straight alpha, and fully transparent pixels are stored as (0,0,0,0). The script sets them that way.
- The game texture is **BC7** (DXGI 98, BC7_UNORM, no mipmaps). Our encoder uses one colour line per 4x4 pixel block (BC7 mode 6). Soft glows, gradients and semi-transparent edges are fine. Very fine detail with many different colours inside one 4x4 block will look smeared. A hard colour change exactly at an alpha edge can get a faint fringe.
- Colours are stored as authored (UNORM, no sRGB conversion).
- Low-resolution versions are a Lanczos downscale with premultiplied alpha. Do not supply them separately.

## How to bake a replacement

1. Save the new PNG over `tools/icons/design/karma-item.png` and/or `tools/icons/design/karma-status.png`.
2. Rebuild the icon data. It reads the vanilla atlases from the game archives (read-only) or from an extracted copy:

   ```
   set SEKIRO_GAME_DIR=E:\PROGRAMS\Steam\steamapps\common\Sekiro
   set SEKIRO_OODLE_DLL=%SEKIRO_GAME_DIR%\oo2core_6_win64.dll
   set SEKIRO_KEYS_DIR=E:\PROGRAMS\ClaudeCode\tools\souls-tools\keys\sekiro
   py -3.13 tools\icons\build_icon_blob.py --preview %TEMP%\karma-preview
   ```

   Reading the archives directly also needs the `cryptography` and `pycryptodome` Python packages. Without them, extract `menu/` once with `souls-tools\bin\SekiroTool\SekiroTool.exe extract <game> <dir> <keys> "menu/"`, then `set SEKIRO_EXTRACTED=<dir>` instead of `SEKIRO_KEYS_DIR`.
3. Check the previews in the `--preview` folder (`hi_172.png`, `low_172.png`, `hi_2150.png`, `low_2150.png`). They are decoded from exactly the BC7 blocks the DLL will write.
4. Rebuild the DLL (`build-public.cmd`, or `build-dist.cmd` for test builds). The data is embedded from `crates/coop-dll/assets/icons_bc7.bin`.

Other options: `--karma-item <png>` and `--karma-status <png>` read a file from another place. `--karma-item-id` and `--karma-status-id` pick other cells, but then the DLL code and the param rows must change too (`ITEM_ICON` / `STATUS_ICON` in `crates/coop-dll/src/karma.rs`).

## Texts

The item's name, short info and long description are served by the GoodsName / GoodsInfo / GoodsCaption detours of `items.rs` for goods row 930150. The strings themselves are in `text_strings()` in `crates/coop-dll/src/karma.rs`. The game language is detected from the vanilla name of the Prayer Bead (Cyrillic = Russian, anything else = English). The numbers (10 %, 2 %, 6) are filled in from the constants `HP_PCT_PER_KARMA`, `DAMAGE_PCT_PER_KARMA` and `MAX_KARMA`. `\n` is a line break; the long text wraps at about 40 characters per line in the item menu.

| | English | Russian |
|---|---|---|
| Name | Karma | Карма |
| Short info (one line under the name) | The weight of deaths in co-op. Lowers vitality, posture and damage. Rest at an idol to cleanse it. | Бремя смертей в совместной игре. Снижает здоровье, концентрацию и урон. Отдых у идола очищает её. |
| Description | Karma gathers each time you truly die / while fighting beside a companion. // Each Karma lowers your max vitality and / posture by 10% and the damage you deal / by 2% (up to 6). // Rest at a Sculptor's Idol yourself / to cleanse it. | Карма копится, когда вы гибнете, / сражаясь бок о бок с соратником. // Каждая единица снижает максимальное / здоровье и концентрацию на 10%, / а наносимый урон на 2% (не больше 6). // Отдохните у идола Скульптора сами, / чтобы очистить карму. |

(`/` = line break, `//` = empty line.) Other texts the player can see: the options page row "Karma" with its help line (`optmenu_model.rs`, row 5), the ImGui panel checkbox (`ui.rs`), and the `death_debuffs` comment in the settings file template (`settings.rs`).
