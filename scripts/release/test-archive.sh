#!/bin/sh
set -eu
SCRIPT_DIR=$(CDPATH= cd -- "$(dirname -- "$0")" && pwd)
[ "$#" -eq 0 ] || [ "$#" -eq 2 ] || { printf 'Usage: test-archive.sh [BINARY VERSION]\n' >&2; exit 2; }
TMP=$(mktemp -d)
trap 'rm -rf "$TMP"' EXIT HUP INT TERM
python3 - "$SCRIPT_DIR" "$TMP" "$@" <<'PY'
import lzma
import os
from pathlib import Path
import subprocess
import sys
import tarfile

scripts, tmp = map(Path, sys.argv[1:3])
# Frozen from v0.1.0: compile independently of src/update.rs so future fixes
# cannot accidentally relax the compatibility regression.
source = tmp / "validator.rs"
source.write_text(r'''
use std::path::{Path, Component};
fn archive_entry_path_is_normal(name: &str) -> bool {
    !name.is_empty()
        && !name.contains('\0')
        && !name.contains('\\')
        && !name.contains(':')
        && Path::new(name)
            .components()
            .all(|component| matches!(component, Component::Normal(part) if !part.is_empty()))
        && !name
            .split('/')
            .any(|part| part.is_empty() || part == "." || part == "..")
}
fn main() {
    for name in std::env::args().skip(1) {
        if !archive_entry_path_is_normal(&name) { std::process::exit(1); }
    }
}
''')
validator = tmp / "validator"
subprocess.run(["rustc", str(source), "-o", str(validator)], check=True)

cases = [("1.2.3", None), ("1.2.3-beta.1", None)]
if len(sys.argv) == 5:
    cases.append((sys.argv[4], Path(sys.argv[3]).resolve()))
for index, (version, real_binary) in enumerate(cases):
    case_dir = tmp / str(index)
    case_dir.mkdir()
    root = case_dir / f"lazydb_{version}_aarch64-apple-darwin"
    root.mkdir()
    binary = root / "lazydb"
    binary.write_text(
        '#!/bin/sh\n[ "$#" -eq 2 ] && [ "$1" = version ] && '
        '[ "$2" = --json ] || exit 9\n'
        f"printf '%s\\n' '{{\"version\":\"{version}\"}}'\n"
    )
    binary.chmod(0o755)
    if real_binary is not None:
        binary.write_bytes(real_binary.read_bytes())
    for name in ("README.md", "LICENSE-MIT", "LICENSE-APACHE"):
        (root / name).write_bytes(b"fixture\0contents\n")
    (root / "docs").mkdir()
    (root / "docs" / "note.txt").write_text("nested content\n")
    archive = tmp / "package.tar.xz"
    command = [sys.executable, str(scripts / "package-archive.py"), str(root), str(archive)]
    subprocess.run(command, check=True)
    original = archive.read_bytes()
    for path in [root, *root.rglob("*")]:
        os.utime(path, (123456789, 123456789))
        path.chmod(0o700 if path.is_dir() or path == binary else 0o600)
    subprocess.run(command, check=True)
    assert archive.read_bytes() == original, "archive is not deterministic"

    # Inspect serialized names: tarfile normalizes trailing slashes on read.
    raw = lzma.decompress(original)
    offset, names, types = 0, [], []
    while raw[offset:offset + 512] != bytes(512):
        header = raw[offset:offset + 512]
        assert len(header) == 512
        checksum = int(header[148:156].rstrip(b"\0 "), 8)
        assert checksum == sum(header[:148] + b" " * 8 + header[156:])
        name = header[:100].split(b"\0")[0].decode()
        prefix = header[345:500].split(b"\0")[0].decode()
        name = f"{prefix}/{name}" if prefix else name
        assert not name.endswith("/"), name
        assert name == root.name or name.startswith(root.name + "/")
        names.append(name)
        types.append(header[156:157])
        size = int(header[124:136].rstrip(b"\0 "), 8)
        offset += 512 + ((size + 511) // 512) * 512
    assert names[0] == root.name and types[0] == b"5"
    assert len(names) == len(set(names))
    assert all(kind in (b"0", b"5") for kind in types)
    subprocess.run([str(validator), *names], check=True)
    assert subprocess.run([str(validator), root.name + "/"]).returncode != 0
    assert raw[offset:] == bytes(len(raw) - offset)
    with tarfile.open(archive) as package:
        assert package.getmember(root.name + "/lazydb").mode == 0o755
        for path in root.rglob("*"):
            if path.is_file():
                assert package.extractfile(path.relative_to(case_dir).as_posix()).read() == path.read_bytes()
    unpack = case_dir / "unpack"
    unpack.mkdir()
    subprocess.run(["tar", "-xJf", str(archive), "-C", str(unpack)], check=True)
    extracted = unpack / root.name / "lazydb"
    assert extracted.read_bytes() == binary.read_bytes()
    smoke = ["sh", str(scripts / "smoke-artifact.sh"), str(extracted)]
    subprocess.run([*smoke, version], check=True)
    assert subprocess.run([*smoke, "wrong"], capture_output=True).returncode != 0
    for output in ('printf "not-json"', 'printf "{}"', 'printf "[]"',
                   f"printf '%s' '{{\"version\":\"{version}\"}}'; exit 7"):
        extracted.write_text("#!/bin/sh\n" + output + "\n")
        assert subprocess.run([*smoke, version], capture_output=True).returncode != 0
    extracted.unlink()
    assert subprocess.run([*smoke, version], capture_output=True).returncode != 0
print("Archive compatibility and artifact smoke tests passed")
PY
