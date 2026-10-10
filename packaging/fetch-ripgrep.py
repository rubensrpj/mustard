#!/usr/bin/env python3
"""Build-time dependency only: install a verified upstream rg and its licenses.

The same pin supplies CI, installers and plugin bootstrap archives. No network
or Python is needed to locate rg at runtime. Extract named members only; never
let an archive choose an output path.
"""
import argparse
import hashlib
import io
import json
from pathlib import Path
import platform
import tarfile
import urllib.request
import zipfile


def host_target():
    system = platform.system()
    machine = platform.machine().lower()
    if system == "Darwin":
        return ("aarch64" if machine in ("arm64", "aarch64") else "x86_64") + "-apple-darwin"
    if machine not in ("amd64", "x86_64"):
        raise ValueError("unsupported ripgrep host architecture")
    return {"Windows": "x86_64-pc-windows-msvc", "Linux": "x86_64-unknown-linux-musl"}[system]


def install(target, dest, archive=None):
    pin = json.loads(Path(__file__).with_name("ripgrep.json").read_text())
    expected = pin["assets"][target]
    stem = "ripgrep-" + pin["version"] + "-" + target
    windows = "windows" in target
    extension = ".zip" if windows else ".tar.gz"
    if archive is None:
        url = "https://github.com/BurntSushi/ripgrep/releases/download/" + pin["version"] + "/" + stem + extension
        with urllib.request.urlopen(url, timeout=60) as response:
            data = response.read(16 * 1024 * 1024 + 1)
    else:
        data = Path(archive).read_bytes()
    if len(data) > 16 * 1024 * 1024 or hashlib.sha256(data).hexdigest() != expected:
        raise ValueError("ripgrep archive does not match the committed SHA-256")
    binary = "rg.exe" if windows else "rg"
    members = [binary, "COPYING", "LICENSE-MIT", "UNLICENSE"]
    # Read and validate all required members before creating any output.
    if windows:
        with zipfile.ZipFile(io.BytesIO(data)) as package:
            contents = {name: package.read(stem + "/" + name) for name in members}
    else:
        with tarfile.open(fileobj=io.BytesIO(data), mode="r:gz") as package:
            contents = {}
            for name in members:
                member = package.getmember(stem + "/" + name)
                if not member.isfile() or member.size > 32 * 1024 * 1024:
                    raise ValueError("invalid ripgrep archive member")
                contents[name] = package.extractfile(member).read()
    dest = Path(dest)
    dest.mkdir(parents=True, exist_ok=True)
    for name, content in contents.items():
        out = dest / (name if name == binary else "ripgrep-" + name)
        out.write_bytes(content)
        out.chmod(0o755 if name == binary else 0o644)
    print("Verified ripgrep " + pin["version"] + " (" + target + ") -> " + str(dest))


if __name__ == "__main__":
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--target", default="auto")
    parser.add_argument("--dest", required=True)
    parser.add_argument("--archive", help="Use a local archive; the same checksum verification applies")
    args = parser.parse_args()
    install(host_target() if args.target == "auto" else args.target, args.dest, args.archive)
