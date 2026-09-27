#!/usr/bin/env python3
"""Sets Vyro's version everywhere it is written down, or checks that they agree.

    scripts/set-version.py 0.2.0     # before tagging a release
    scripts/set-version.py --check   # prints the version; fails if the files disagree

Files: Cargo.toml (workspace), Cargo.lock (the workspace crates), src-tauri/tauri.conf.json
(also the MSI version) and package.json. MSI versions are numeric, so only MAJOR.MINOR.PATCH.
"""

import json
import re
import sys
from pathlib import Path

ROOT = Path(__file__).resolve().parent.parent
CRATES = ("vyro", "vyro-core", "vyro-service")
SEMVER = re.compile(r"^\d+\.\d+\.\d+$")


def cargo_toml(text: str, version: str | None) -> tuple[str, str]:
    m = re.search(r'(\[workspace\.package\][^\[]*?\nversion = ")([^"]+)(")', text, re.S)
    if not m:
        sys.exit("Cargo.toml: no [workspace.package] version")
    if version:
        text = text[: m.start(2)] + version + text[m.end(2) :]
    return text, m.group(2)


def cargo_lock(text: str, version: str | None) -> tuple[str, list[str]]:
    found = []
    for name in CRATES:
        pat = re.compile(r'(\[\[package\]\]\nname = "' + re.escape(name) + r'"\nversion = ")([^"]+)(")')
        m = pat.search(text)
        if not m:
            sys.exit(f"Cargo.lock: no package {name}")
        found.append(m.group(2))
        if version:
            text = text[: m.start(2)] + version + text[m.end(2) :]
    return text, found


def json_version(text: str, version: str | None) -> tuple[str, str]:
    m = re.search(r'("version":\s*")([^"]+)(")', text)
    if not m:
        sys.exit("no version field")
    if version:
        text = text[: m.start(2)] + version + text[m.end(2) :]
    return text, m.group(2)


def main() -> None:
    args = sys.argv[1:]
    if len(args) != 1:
        sys.exit(__doc__)
    version = None if args[0] == "--check" else args[0]
    if version and not SEMVER.match(version):
        sys.exit(f"{version}: expected MAJOR.MINOR.PATCH")
    files = {
        "Cargo.toml": cargo_toml,
        "Cargo.lock": cargo_lock,
        "src-tauri/tauri.conf.json": json_version,
        "package.json": json_version,
    }
    seen = {}
    for rel, edit in files.items():
        path = ROOT / rel
        text, old = edit(path.read_text(encoding="utf-8"), version)
        seen[rel] = old
        if version:
            path.write_text(text, encoding="utf-8")
    if version:
        print(version)
        return
    flat = {v for old in seen.values() for v in ([old] if isinstance(old, str) else old)}
    if len(flat) != 1:
        sys.exit("versions disagree: " + json.dumps(seen))
    print(flat.pop())


if __name__ == "__main__":
    main()
