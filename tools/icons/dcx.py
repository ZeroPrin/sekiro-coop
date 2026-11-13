"""DCX container (Sekiro: DCX_KRAK and DCX_DFLT), own implementation.

Header (big endian):
  0x00 "DCX\0"  u32 0x11000  u32 0x18  u32 0x24  u32 0x44  u32 0x4C
  0x18 "DCS\0"  u32 uncompressed size  u32 compressed size
  0x24 "DCP\0"  "KRAK"/"DFLT"  u32 0x20  u8 level  ...  (0x20 bytes block)
  0x44 "DCA\0"  u32 8
  0x4C compressed data
Kraken uses the game's own oo2core_6_win64.dll (OodleLZ_Compress/Decompress) via ctypes.
"""
import ctypes
import os
import struct
import zlib

OODLE_DLL = r"F:\SteamLibrary\steamapps\common\Sekiro\oo2core_6_win64.dll"
_oodle = None


def oodle():
    global _oodle
    if _oodle is None:
        _oodle = ctypes.WinDLL(OODLE_DLL)
        _oodle.OodleLZ_Decompress.restype = ctypes.c_ssize_t
        _oodle.OodleLZ_Compress.restype = ctypes.c_ssize_t
    return _oodle


def kraken_decompress(comp: bytes, raw_size: int) -> bytes:
    out = ctypes.create_string_buffer(raw_size)
    o = oodle()
    n = o.OodleLZ_Decompress(
        comp, ctypes.c_ssize_t(len(comp)), out, ctypes.c_ssize_t(raw_size),
        1, 0, 0, None, ctypes.c_ssize_t(0), None, None, None, ctypes.c_ssize_t(0), 3)
    if n != raw_size:
        raise ValueError(f"Oodle decompress returned {n}, expected {raw_size}")
    return out.raw


BLOCK = 0x40000  # Kraken block = seek chunk: 256 KiB of output


def kraken_blocks(stream: bytes, raw_size: int):
    """(blocks, blocks with the decoder-reset flag) of a Kraken stream. Each 256 KiB of output
    starts with a 2-byte block header (b0 low nibble 0xC, bit 7 reset, bit 6 uncompressed), then
    for a compressed block a 3-byte quantum header (6 with a CRC; 0x3FFFF = memset, 4 bytes)."""
    p = out = n = reset = 0
    while out < raw_size:
        b0, b1 = stream[p], stream[p + 1]
        if b0 & 0xF != 0xC:
            raise ValueError(f"bad Kraken block header 0x{b0:02X} at {p}")
        n += 1
        reset += b0 >> 7 & 1
        p += 2
        left = min(BLOCK, raw_size - out)
        if b0 >> 6 & 1:
            p += left
        else:
            size = (stream[p] << 16 | stream[p + 1] << 8 | stream[p + 2]) & 0x3FFFF
            if size != 0x3FFFF:
                p += (6 if b1 >> 7 else 3) + size + 1
            else:
                p += 4
        if p > len(stream):
            raise ValueError("Kraken stream ends inside a block")
        out += left
    return n, reset


def kraken_compress(raw: bytes, level: int = 6) -> bytes:
    """Kraken with a decoder reset on every 256 KiB block (seekChunkReset), as vanilla DCX_KRAK
    files have. The game's loader decodes the blocks independently: a stream without the resets
    stalled it (the first wardrobe part files) and is the likely cause of the 2026-10-23 icon
    override crash (IMPLEMENTATION-NOTES section 21). Oodle's default options have no resets."""
    o = oodle()
    # OodleLZ_CompressOptions (Oodle 2.6): 12 x int32; [2] seekChunkReset, [3] seekChunkLen.
    o.OodleLZ_CompressOptions_GetDefault.restype = ctypes.c_void_p
    opts = (ctypes.c_int32 * 12)()
    ctypes.memmove(opts, o.OodleLZ_CompressOptions_GetDefault(8, level), ctypes.sizeof(opts))
    opts[2] = 1
    opts[3] = BLOCK
    cap = len(raw) + 274 * ((len(raw) + BLOCK - 1) // BLOCK) + 0x10000
    out = ctypes.create_string_buffer(cap)
    # OodleLZ_Compressor_Kraken = 8
    n = o.OodleLZ_Compress(8, raw, ctypes.c_ssize_t(len(raw)), out, level,
                           ctypes.byref(opts), None, None, None, ctypes.c_ssize_t(0))
    if n <= 0:
        raise ValueError(f"Oodle compress failed: {n}")
    comp = out.raw[:n]
    blocks, reset = kraken_blocks(comp, len(raw))
    if blocks != reset:
        raise ValueError(f"Kraken stream has a decoder reset on only {reset} of {blocks} blocks; refusing to write it")
    return comp


def decompress(data: bytes):
    """Returns (payload, info). info = dict(fmt, level, header) for round trips."""
    if data[:4] != b"DCX\0":
        return data, None
    raw_size, comp_size = struct.unpack_from(">II", data, 0x1C)
    fmt = data[0x28:0x2C]
    (data_off,) = struct.unpack_from(">I", data, 0x14)
    comp = data[data_off:data_off + comp_size]
    info = {"fmt": fmt, "level": data[0x30], "header": data[:data_off]}
    if fmt == b"KRAK":
        return kraken_decompress(comp, raw_size), info
    if fmt == b"DFLT":
        return zlib.decompress(comp), info
    raise ValueError(f"unsupported DCX format {fmt!r}")


def compress(payload: bytes, info, level: int = None) -> bytes:
    """Re-wrap payload with the same DCX header. Kraken level: default = header level
    (vanilla 6 = Optimal2, slow: ~80 s for 70 MB); 4 (Normal) is ~25x faster and ~3% bigger."""
    fmt = info["fmt"]
    if fmt == b"KRAK":
        comp = kraken_compress(payload, level if level is not None else (info.get("level", 6) or 6))
    elif fmt == b"DFLT":
        comp = zlib.compress(payload, 9)
    else:
        raise ValueError(fmt)
    hdr = bytearray(info["header"])
    struct.pack_into(">II", hdr, 0x1C, len(payload), len(comp))
    return bytes(hdr) + comp


if __name__ == "__main__":
    import sys
    src, dst = sys.argv[1], sys.argv[2]
    payload, info = decompress(open(src, "rb").read())
    print(src, info and info["fmt"], info and info["level"], len(payload))
    os.makedirs(os.path.dirname(os.path.abspath(dst)), exist_ok=True)
    open(dst, "wb").write(payload)
