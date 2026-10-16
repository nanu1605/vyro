#!/usr/bin/env python3
"""Front-end size budget (P27): what each page loads before it shows anything, gzipped.

    python3 scripts/bundle_budget.py           # report
    python3 scripts/bundle_budget.py --check   # also fail when a page is over its budget

Run after `pnpm build`. A page's start cost is every script, preloaded module and stylesheet its
HTML names. Lazy chunks (tabs and cards loaded on first view, the 3-D scene) are listed but not
budgeted: they cost nothing until used. Writes a Markdown table to $GITHUB_STEP_SUMMARY in CI.
"""

import gzip
import os
import re
import sys
from pathlib import Path

DIST = Path(__file__).resolve().parent.parent / "dist"

# Bytes, gzipped. Raise one only on purpose, in the same change as what needs it, and say why
# in the ROADMAP.
BUDGETS = {
    "index.html": 125_000,  # the app's window
    "remote.html": 32_000,  # the phone companion (P26)
}

REF = re.compile(r'<(?:script|link)\b[^>]*?\b(?:src|href)="/?(assets/[^"]+\.(?:js|css))"')


def gz(path: Path) -> int:
    return len(gzip.compress(path.read_bytes(), compresslevel=9, mtime=0))


def page(name: str) -> tuple[int, int, list[str]]:
    html = (DIST / name).read_text(encoding="utf-8")
    files = sorted(set(REF.findall(html)))
    raw = sum((DIST / f).stat().st_size for f in files)
    return raw, sum(gz(DIST / f) for f in files), files


def kb(n: int) -> str:
    return f"{n / 1000:.1f} kB"


def main() -> int:
    check = "--check" in sys.argv[1:]
    if not (DIST / "index.html").exists():
        print("dist/ is missing: run `pnpm build` first", file=sys.stderr)
        return 2
    lines = ["| Page | Start files | Raw | Gzipped | Budget |", "|---|---:|---:|---:|---:|"]
    over = []
    start = set()
    for name, budget in BUDGETS.items():
        raw, packed, files = page(name)
        start.update(files)
        mark = "" if packed <= budget else " **over**"
        lines.append(
            f"| {name} | {len(files)} | {kb(raw)} | {kb(packed)}{mark} | {kb(budget)} |"
        )
        if packed > budget:
            over.append(f"{name}: {kb(packed)} gzipped, over its {kb(budget)} budget")
    lazy = sorted(
        (p for p in (DIST / "assets").iterdir() if p.suffix == ".js"
         and f"assets/{p.name}" not in start),
        key=lambda p: p.stat().st_size,
        reverse=True,
    )
    lines += ["", "Largest chunks loaded on demand:", "", "| Chunk | Gzipped |", "|---|---:|"]
    lines += [f"| {p.name} | {kb(gz(p))} |" for p in lazy[:6]]
    report = "\n".join(lines)
    print(report)
    summary = os.environ.get("GITHUB_STEP_SUMMARY")
    if summary:
        with open(summary, "a", encoding="utf-8") as f:
            f.write("### Front-end size\n\n" + report + "\n")
    if check and over:
        for o in over:
            print(f"::error::{o}", file=sys.stderr)
        return 1
    return 0


if __name__ == "__main__":
    sys.exit(main())
