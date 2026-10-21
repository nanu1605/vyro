#!/usr/bin/env python3
"""Front-end size budget (P27): what each page loads before it shows anything, gzipped.

    python3 scripts/bundle_budget.py           # report
    python3 scripts/bundle_budget.py --check   # also fail when a page is over its budget

Run after `pnpm build` and `pnpm build:phone`. A page's start cost is every script, preloaded module and stylesheet its
HTML names. Lazy chunks (tabs and cards loaded on first view, the 3-D scene) are listed but not
budgeted: they cost nothing until used. Writes a Markdown table to $GITHUB_STEP_SUMMARY in CI.
"""

import gzip
import os
import re
import sys
from pathlib import Path

ROOT = Path(__file__).resolve().parent.parent
DIST = ROOT / "dist"

# Bytes, gzipped. Raise one only on purpose, in the same change as what needs it, and say why
# in the ROADMAP.
BUDGETS = {
    # The app's window. 125 kB until 1.1; raised for the new frame (sidebar, top bar, live
    # rail, icons, motion; ROADMAP P28), with the palette and the heavy views still lazy.
    "dist/index.html": 200_000,
    # The phone companion (P26). 32 kB until 1.3; raised for a custom accent's colour maths
    # (color.ts: the page shows the PC's accent, any colour; ROADMAP 1.3.0).
    "dist/remote.html": 34_000,
    # The phone app (1.4), served by the relay: the companion's readings plus pairing, the relay
    # link and its sealing (WebCrypto). The QR reader for scanning loads only when scanning.
    # 40 kB until 1.7; raised for the redesigned frame (the bar, the tabs at the bottom with their
    # icons, toasts, motion; ROADMAP 1.7.0), with every tab, the home screen, the offline states
    # and the sheets still lazy.
    "relay/public/index.html": 44_000,
}

REF = re.compile(r'<(?:script|link)\b[^>]*?\b(?:src|href)="/?(assets/[^"]+\.(?:js|css))"')


def gz(path: Path) -> int:
    return len(gzip.compress(path.read_bytes(), compresslevel=9, mtime=0))


def page(name: str) -> tuple[int, int, list[str]]:
    path = ROOT / name
    html = path.read_text(encoding="utf-8")
    files = sorted(set(REF.findall(html)))
    raw = sum((path.parent / f).stat().st_size for f in files)
    return raw, sum(gz(path.parent / f) for f in files), [str(Path(name).parent / f) for f in files]


def kb(n: int) -> str:
    return f"{n / 1000:.1f} kB"


def main() -> int:
    check = "--check" in sys.argv[1:]
    missing = [n for n in BUDGETS if not (ROOT / n).exists()]
    if missing:
        print(f"{', '.join(missing)} missing: run `pnpm build` and `pnpm build:phone` first", file=sys.stderr)
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
         and f"dist/assets/{p.name}" not in start),
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
