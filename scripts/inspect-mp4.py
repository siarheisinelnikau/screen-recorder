"""Read ISO BMFF codec, timing and sample tables from the synthetic MP4; no playback."""
from pathlib import Path
import struct

root = Path(__file__).resolve().parents[1] / ".test-output"
folders = sorted(root.glob("synthetic-*"), key=lambda p: p.stat().st_mtime_ns)
assert folders, "Run the synthetic example first"
data = (folders[-1] / "tutorial-edited.mp4").read_bytes()


def boxes(start, end):
    while start < end:
        assert start + 8 <= end
        size, kind = struct.unpack_from(">I4s", data, start)
        header = 8
        if size == 1:
            size = struct.unpack_from(">Q", data, start + 8)[0]
            header = 16
        if size == 0:
            size = end - start
        assert size >= header and start + size <= end
        yield kind, start + header, start + size
        start += size
    assert start == end


def child(parent, name):
    return next((begin, end) for kind, begin, end in boxes(*parent) if kind == name)


top = list(boxes(0, len(data)))
assert {b"ftyp", b"moov", b"mdat"} <= {kind for kind, _, _ in top}
moov = child((0, len(data)), b"moov")
tracks = []
for kind, begin, end in boxes(*moov):
    if kind != b"trak":
        continue
    mdia = child((begin, end), b"mdia")
    hdlr, _ = child(mdia, b"hdlr")
    handler = data[hdlr + 8:hdlr + 12]
    mdhd, _ = child(mdia, b"mdhd")
    if data[mdhd] == 1:
        timescale, duration = struct.unpack_from(">IQ", data, mdhd + 20)
    else:
        timescale, duration = struct.unpack_from(">II", data, mdhd + 12)
    minf = child(mdia, b"minf")
    stbl = child(minf, b"stbl")
    stsd, stsd_end = child(stbl, b"stsd")
    assert struct.unpack_from(">I", data, stsd + 4)[0] == 1
    codec, codec_begin, codec_end = next(boxes(stsd + 8, stsd_end))
    stsz, _ = child(stbl, b"stsz")
    count = struct.unpack_from(">I", data, stsz + 8)[0]
    assert count > 0
    if handler == b"vide":
        assert codec == b"avc1"
        width, height = struct.unpack_from(">HH", data, codec_begin + 24)
        assert (width, height, count) == (320, 180, 16)
        assert b"avcC" in data[codec_begin:codec_end]
        assert abs(duration / timescale - 1.6) < 0.02
    else:
        assert handler == b"soun" and codec == b"mp4a"
        assert b"esds" in data[codec_begin:codec_end]
        assert abs(duration / timescale - 1.6) < 0.1  # AAC priming/padding
    tracks.append((handler.decode(), codec.decode(), count, duration / timescale))
assert len(tracks) == 2
print("Validated synthetic MP4:", tracks)
print(folders[-1])
