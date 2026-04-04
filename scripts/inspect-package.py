"""Read PE headers/imports and checksums without executing the application."""
from pathlib import Path
import hashlib
import struct
import tomllib

root = Path(__file__).resolve().parents[1]
version = tomllib.loads((root / "Cargo.toml").read_text())["package"]["version"]
asset = root / "dist" / f"ScreenRecorder-v{version}.exe"
data = asset.read_bytes()
assert data[:2] == b"MZ"
pe = struct.unpack_from("<I", data, 60)[0]
assert data[pe:pe+4] == b"PE\0\0"
machine, count = struct.unpack_from("<HH", data, pe+4)
assert machine == 0x8664, "Package is not x64"
optional_size = struct.unpack_from("<H", data, pe+20)[0]
optional = pe+24
assert struct.unpack_from("<H", data, optional)[0] == 0x20B
assert struct.unpack_from("<H", data, optional+68)[0] == 2, "Expected Windows GUI subsystem"
sections = []
for i in range(count):
    header = optional+optional_size+i*40
    virtual_size, rva, raw_size, raw = struct.unpack_from("<IIII", data, header+8)
    sections.append((rva, max(virtual_size, raw_size), raw))


def file_offset(rva):
    for start, size, raw in sections:
        if start <= rva < start+size:
            return raw+rva-start
    raise AssertionError(f"Unmapped RVA: {rva:x}")


import_rva = struct.unpack_from("<I", data, optional+112+8)[0]
descriptor = file_offset(import_rva)
imports = []
while any(data[descriptor:descriptor+20]):
    name_rva = struct.unpack_from("<I", data, descriptor+12)[0]
    name = file_offset(name_rva)
    end = data.index(0, name)
    imports.append(data[name:end].decode("ascii"))
    descriptor += 20
for name in imports:
    lower = name.lower()
    assert not lower.startswith(("vcruntime", "msvcp", "api-ms-win-crt")) and lower != "ucrtbase.dll", f"Dynamic CRT import: {name}"
assert not any(n.lower().startswith(("opengl", "d3d", "dxgi", "vulkan")) for n in imports), "Unexpected GPU renderer import"
digest = hashlib.sha256(data).hexdigest()
checksum = (root / "dist" / "SHA256SUMS.txt").read_text().strip().split()
assert checksum == [digest, asset.name]
assert (root / "dist" / "LICENSE.txt").is_file()
notices = (root / "dist" / "THIRD_PARTY_NOTICES.txt").read_text(encoding="utf-8")
assert "Ubuntu" in notices and "OFL" in notices and "softbuffer" in notices
print(f"Validated package: x64 Windows GUI, {len(data):,} bytes, no dynamic MSVC/UCRT imports")
print(f"SHA-256 {digest}")
print("Imports: " + ", ".join(imports))
