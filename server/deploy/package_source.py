#!/usr/bin/env python3
"""Package server source and workspace dependencies without local data or keys."""

import argparse
from pathlib import Path
import tarfile


def main():
    parser = argparse.ArgumentParser()
    parser.add_argument("output", type=Path)
    args = parser.parse_args()
    root = Path(__file__).resolve().parents[2]
    args.output.parent.mkdir(parents=True, exist_ok=True)

    def include(info):
        path = Path(info.name)
        if any(p in {"target", "node_modules", "__pycache__", ".git"} for p in path.parts):
            return None
        if path.name == ".env" or path.suffix in {".db", ".sqlite", ".pyc"}:
            return None
        return info

    with tarfile.open(args.output, "w:gz") as archive:
        for name in ("Cargo.toml", "Cargo.lock", "LICENSE", "crates/core", "server", "src-tauri", "tests", "third_party"):
            archive.add(root / name, arcname=name, filter=include)
    print(f"Source archive: {args.output} ({args.output.stat().st_size} bytes)")


if __name__ == "__main__":
    main()
