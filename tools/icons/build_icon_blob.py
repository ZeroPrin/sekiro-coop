"""Build crates/coop-dll/assets/icons_bc7.bin: pre-encoded BC7 blocks for the custom icon cells,
which the DLL (icons.rs) writes into the menu atlas textures in memory while the game loads
menu:/{hi,low}/01_common.tpf. No game file is replaced (a me3 file override of
01_common.tpf.dcx crashed the game, 2026-10-23).

Cells (all in menu/{hi,low}/01_common.tpf, all BC7_UNORM, 1 mip):
  item icons   MENU_ItemIcon_<id>   in SB_Icon / SB_Icon_03  (hi 252x252, low 126x126)
               251 host bell, 552 join charm, 962 leave thread, 172 karma
  status icon  MENU_StatusIcon_<id> in SB_Status            (2150: hi 92x90, low 46x45)
               2150 karma (HUD buff icon under the HP bar, SpEffectParam.iconId)

usage (regenerate after changing a design):
  set SEKIRO_GAME_DIR=E:\\PROGRAMS\\Steam\\steamapps\\common\\Sekiro   (default F:\\SteamLibrary\\...)
  set SEKIRO_OODLE_DLL=%SEKIRO_GAME_DIR%\\oo2core_6_win64.dll
  either  set SEKIRO_KEYS_DIR=<dir with Data1..5.pem>   (or a local tools\\icons\\sekiro_keys.py)
  or      set SEKIRO_EXTRACTED=<dir holding menu\\hi\\01_common.*.dcx, menu\\low\\... extracted>
  py -3.13 tools\\icons\\build_icon_blob.py [host.png join.png leave.png] [--ids 251,552,962]
          [--karma-item design\\karma-item.png] [--karma-item-id 172]
          [--karma-status design\\karma-status.png] [--karma-status-id 2150]
          [--preview DIR]
  defaults: tools\\icons\\design\\gpt-bell.png gpt-charm.png gpt-thread.png -> ids 251 552 962,
            tools\\icons\\design\\karma-item.png -> 172, karma-status.png -> 2150.
  Item PNGs: any square size (resized to the cell). Status PNG: 92x90 (or the same aspect).
  Straight (non-premultiplied) alpha. The low cells are a premultiplied Lanczos downscale.
then rebuild the DLL (build-dist.cmd / build-public.cmd); include_bytes! embeds the file.

Record order: hi 251, 552, 962, 172, 2150, then low 251, 552, 962, 172, 2150.

Format (little endian), version 1, unchanged:
  header: b"SCIC", u32 version = 1, u32 record count
  record: char[32] texture name (UTF-8, zero padded, e.g. "SB_Icon", "SB_Status")
          u32 texture width, u32 texture height, u32 expected DDS byte size (148 + w*h for BC7)
          u32 block x, u32 block y, u32 blocks wide, u32 blocks high (4x4 BC7 blocks)
          u32 icon id (item icon id, or status icon id; the texture name tells which)
          u64 FNV-1a 64 of the vanilla block bytes (row by row), u64 FNV-1a 64 of the new bytes
          bw*bh*16 bytes of BC7 blocks, row-major
The DLL patches a cell only when the texture name, size, DXGI format (98 = BC7_UNORM), mip count
(1) and the vanilla hash all match; otherwise it logs and leaves the texture alone.
Cells that are not 4-px aligned (all low item cells) re-encode the few neighbour pixels that share
their edge blocks (decoded vanilla pixels, re-encoded); the gaps between cells absorb that.
"""
import argparse
import io
import os
import re
import struct
import sys
import tempfile

import numpy as np
from PIL import Image

HERE = os.path.dirname(os.path.abspath(__file__))
sys.path.insert(0, HERE)
import bc7  # noqa: E402
import bhd5  # noqa: E402
import dcx  # noqa: E402
from build_icons import DEFAULT_IDS, layout_rects, prepare_icon  # noqa: E402
from souls import BND4, TPF, dds_info  # noqa: E402

REPO = os.path.abspath(os.path.join(HERE, "..", ".."))
OUT = os.path.join(REPO, "crates", "coop-dll", "assets", "icons_bc7.bin")
DESIGN = os.path.join(HERE, "design")
DDS_HEADER = 148
KARMA_ITEM_ID = 172      # vanilla gourd variant, no param row uses it (find_unused_icons.py)
KARMA_STATUS_ID = 2150   # vanilla "poison resistance down" art, no SpEffectParam row uses it


def fnv1a64(data: bytes) -> int:
    h = 0xCBF29CE484222325
    for b in data:
        h ^= b
        h = (h * 0x100000001B3) & 0xFFFFFFFFFFFFFFFF
    return h


def block_rows(dds: bytes, w: int, bx: int, by: int, bw: int, bh: int) -> bytes:
    out = bytearray()
    for r in range(bh):
        o = DDS_HEADER + ((by + r) * (w // 4) + bx) * 16
        out += dds[o:o + bw * 16]
    return bytes(out)


def status_rects(res):
    """{status icon id: (texture, x, y, w, h)} from SB_Status*.layout."""
    raw, _ = dcx.decompress(bhd5.read_game_file(f"/menu/{res}/01_common.sblytbnd.dcx"))
    rects = {}
    for f in BND4(raw).files:
        s = f.data.decode("utf-8-sig")
        image = re.search(r'imagePath="([^"]+)\.png"', s).group(1)
        for n, x, y, w, h in re.findall(
                r'<SubTexture name="MENU_StatusIcon_(\d+)\.png" x="(\d+)" y="(\d+)" width="(\d+)" height="(\d+)"', s):
            rects[int(n)] = (image, int(x), int(y), int(w), int(h))
    return rects


def prepare_rect(path, w, h):
    """Load a PNG and resize it to exactly w x h with premultiplied-alpha Lanczos (straight RGBA out)."""
    im = Image.open(path).convert("RGBA")
    if im.size == (w, h):
        res = np.array(im)
        res[res[..., 3] == 0] = 0
        return res
    a = np.asarray(im).astype(np.float32) / 255.0
    pm = a.copy()
    pm[..., :3] *= pm[..., 3:4]
    out = np.stack([np.asarray(Image.fromarray(pm[..., c], mode="F").resize((w, h), Image.LANCZOS)) for c in range(4)], -1)
    out = np.clip(out, 0.0, 1.0)
    alpha = out[..., 3:4]
    rgb = np.where(alpha > 1e-4, out[..., :3] / np.maximum(alpha, 1e-4), 0.0)
    res = np.rint(np.concatenate([np.clip(rgb, 0, 1), alpha], -1) * 255).astype(np.uint8)
    res[res[..., 3] == 0] = 0
    return res


def main():
    ap = argparse.ArgumentParser()
    ap.add_argument("pngs", nargs="*", default=[os.path.join(DESIGN, f"gpt-{n}.png") for n in ("bell", "charm", "thread")])
    ap.add_argument("--ids", default=",".join(map(str, DEFAULT_IDS)))
    ap.add_argument("--karma-item", default=os.path.join(DESIGN, "karma-item.png"))
    ap.add_argument("--karma-item-id", type=int, default=KARMA_ITEM_ID)
    ap.add_argument("--karma-status", default=os.path.join(DESIGN, "karma-status.png"))
    ap.add_argument("--karma-status-id", type=int, default=KARMA_STATUS_ID)
    ap.add_argument("--out", default=OUT)
    ap.add_argument("--preview", default=os.path.join(tempfile.gettempdir(), "sekiro-icons-preview"))
    args = ap.parse_args()
    ids = [int(v) for v in args.ids.split(",")]
    assert len(args.pngs) == 3 and len(ids) == 3
    os.makedirs(args.preview, exist_ok=True)

    records = []
    for res in ("hi", "low"):
        items, status = layout_rects(res), status_rects(res)
        payload, _ = dcx.decompress(bhd5.read_game_file(f"/menu/{res}/01_common.tpf.dcx"))
        tex = {t.name: t for t in TPF(payload).textures}
        jobs = [("MENU_ItemIcon", png, icon_id, items[icon_id])
                for png, icon_id in zip(args.pngs + [args.karma_item], ids + [args.karma_item_id])]
        jobs.append(("MENU_StatusIcon", args.karma_status, args.karma_status_id, status[args.karma_status_id]))
        for kind, png, icon_id, (image, x, y, cw, ch) in jobs:
            t = tex[image]
            w, h, mips, fourcc, dxgi, _ = dds_info(t.dds)
            assert (fourcc, dxgi, mips, t.format) == (b"DX10", 98, 1, 102), (image, fourcc, dxgi, mips, t.format)
            assert len(t.dds) == DDS_HEADER + w * h, (image, len(t.dds))
            img = np.array(Image.open(io.BytesIO(t.dds)).convert("RGBA"))
            if kind == "MENU_ItemIcon":
                assert cw == ch, (icon_id, cw, ch)
                img[y:y + ch, x:x + cw] = prepare_icon(png, cw)
            else:
                img[y:y + ch, x:x + cw] = prepare_rect(png, cw, ch)
            bx, by = x // 4, y // 4
            bw, bh = (x + cw + 3) // 4 - bx, (y + ch + 3) // 4 - by
            px = img[by * 4:(by + bh) * 4, bx * 4:(bx + bw) * 4]
            new = bc7.encode(np.ascontiguousarray(px))
            old = block_rows(t.dds, w, bx, by, bw, bh)
            assert len(new) == len(old) == bw * bh * 16
            # preview: decode exactly what the DLL will write
            dec = np.array(Image.open(io.BytesIO(bc7.dds_header_bc7(bw * 4, bh * 4) + new)).convert("RGBA"))
            Image.fromarray(dec).save(os.path.join(args.preview, f"{res}_{icon_id}.png"))
            records.append((image, w, h, len(t.dds), bx, by, bw, bh, icon_id, fnv1a64(old), fnv1a64(new), new))
            print(f"[{res}] {kind}_{icon_id:05d}: {image} {w}x{h} cell ({x},{y}) {cw}x{ch} "
                  f"-> blocks ({bx},{by}) {bw}x{bh}, vanilla {fnv1a64(old):016x} new {fnv1a64(new):016x}")

    blob = bytearray(b"SCIC" + struct.pack("<II", 1, len(records)))
    for image, w, h, size, bx, by, bw, bh, icon_id, ho, hn, data in records:
        blob += image.encode().ljust(32, b"\0")
        blob += struct.pack("<IIIIIIIIQQ", w, h, size, bx, by, bw, bh, icon_id, ho, hn)
        blob += data
    os.makedirs(os.path.dirname(args.out), exist_ok=True)
    with open(args.out, "wb") as f:
        f.write(blob)
    print(f"wrote {args.out} ({len(blob)} bytes, {len(records)} cells); previews in {args.preview}")
    print("items.rs icon ids: HOST_ICON = %d, JOIN_ICON = %d, LEAVE_ICON = %d" % tuple(ids))
    print(f"karma: EquipParamGoods iconId = {args.karma_item_id}, SpEffectParam iconId = {args.karma_status_id}")


if __name__ == "__main__":
    main()
