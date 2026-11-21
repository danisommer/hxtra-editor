#!/usr/bin/env python3
"""build_site.py - assemble the catalog extractor page for GitHub Pages (or a local test).

  build_site.py HXTRA_CHECKOUT OUT_DIR [--hxtra-commit SHA]

OUT_DIR gets the page, its worker, our extractor and HXtra's Python tools (GPL-3.0, with its
license), plus files.json, the list the worker loads into Pyodide. No firmware and no catalog
ever goes into it: the page reads the user's own .hxf in the browser.
"""
import argparse
import json
import shutil
import subprocess
from pathlib import Path

HERE = Path(__file__).resolve().parent
ROOT = HERE.parent
PAGE_FILES = ["index.html", "worker.js"]
TOOL_DIRS = ["tools", "tools/features", "tools/sharc"]


def commit_of(checkout):
    try:
        return subprocess.run(["git", "-C", str(checkout), "rev-parse", "HEAD"], capture_output=True, text=True, check=True).stdout.strip()
    except (OSError, subprocess.CalledProcessError):
        return None


def main(argv=None):
    ap = argparse.ArgumentParser(description=__doc__.split("\n\n")[0])
    ap.add_argument("hxtra")
    ap.add_argument("out")
    ap.add_argument("--hxtra-commit")
    a = ap.parse_args(argv)
    hxtra, out = Path(a.hxtra), Path(a.out)
    if out.exists():
        shutil.rmtree(out)
    out.mkdir(parents=True)
    for name in PAGE_FILES:
        shutil.copy2(HERE / name, out / name)
    shutil.copy2(ROOT / "LICENSE", out / "LICENSE")
    files = []
    py = out / "py"
    py.mkdir()
    shutil.copy2(ROOT / "tools" / "catalog" / "hxs_catalog.py", py / "hxs_catalog.py")
    files.append("py/hxs_catalog.py")
    for d in TOOL_DIRS:
        dest = py / "hxtra" / d
        dest.mkdir(parents=True, exist_ok=True)
        for f in sorted((hxtra / d).glob("*.py")):
            shutil.copy2(f, dest / f.name)
            files.append(f"py/hxtra/{d}/{f.name}")
    shutil.copy2(hxtra / "LICENSE", py / "hxtra" / "LICENSE")
    manifest = {"hxtra_commit": a.hxtra_commit or commit_of(hxtra), "files": files}
    (out / "files.json").write_text(json.dumps(manifest, indent=1), encoding="utf-8")
    print(f"{len(files)} Python files, HXtra {manifest['hxtra_commit']} -> {out}")


if __name__ == "__main__":
    main()
