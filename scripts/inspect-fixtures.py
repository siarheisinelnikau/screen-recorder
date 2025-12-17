"""Inspect only the artificial media produced by examples/synthetic.rs; never play it."""
from pathlib import Path
import struct
import wave


def chunks(data, start, end):
    while start < end:
        assert start + 8 <= end, "Truncated RIFF header"
        kind, size = struct.unpack_from("<4sI", data, start)
        body = start + 8
        assert body + size <= end, "RIFF chunk exceeds parent"
        yield kind, body, size
        start = body + size + size % 2
    assert start == end, "Unaligned RIFF list"


root = Path(__file__).resolve().parents[1] / ".test-output"
fixtures = sorted(root.glob("synthetic-*"), key=lambda p: p.stat().st_mtime_ns)
assert fixtures, "Run cargo run --offline --locked --example synthetic first"
folder = fixtures[-1]
data = (folder / "tutorial-edited.avi").read_bytes()
assert data[:4] == b"RIFF" and data[8:12] == b"AVI "
assert struct.unpack_from("<I", data, 4)[0] == len(data) - 8
top = list(chunks(data, 12, len(data)))
hdrl = next((body, size) for kind, body, size in top if kind == b"LIST" and data[body:body+4] == b"hdrl")
movi = next((body, size) for kind, body, size in top if kind == b"LIST" and data[body:body+4] == b"movi")
avih = next((body, size) for kind, body, size in chunks(data, hdrl[0]+4, hdrl[0]+hdrl[1]) if kind == b"avih")
microseconds, _, _, _, count, _, streams, _, width, height = struct.unpack_from("<10I", data, avih[0])
assert (microseconds, count, streams, width, height) == (100000, 16, 1, 320, 180)
frames = list(chunks(data, movi[0]+4, movi[0]+movi[1]))
assert len(frames) == count
for kind, body, size in frames:
    assert kind == b"00dc" and data[body:body+2] == b"\xff\xd8" and data[body+size-2:body+size] == b"\xff\xd9"
idx = next((body, size) for kind, body, size in top if kind == b"idx1")
assert idx[1] == count * 16
for i, (_, body, size) in enumerate(frames):
    kind, flags, offset, length = struct.unpack_from("<4sIII", data, idx[0]+i*16)
    assert kind == b"00dc" and flags == 16
    assert movi[0]+offset == body-8 and length == size
with wave.open(str(folder / "silence.wav"), "rb") as wav:
    assert (wav.getnchannels(), wav.getsampwidth(), wav.getframerate(), wav.getnframes()) == (1, 2, 16000, 32000)
    assert not any(wav.readframes(wav.getnframes())), "WAV is not silent"
print(f"Validated synthetic AVI: {count} frames, {width}x{height}, 1.6 seconds, one video stream; WAV: 2 seconds silence")
print(folder)
