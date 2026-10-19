#!/usr/bin/env python3
"""Create a consistent SQLite snapshot; retain the latest fourteen snapshots."""

import os
from pathlib import Path
import sqlite3
from datetime import datetime, timezone


def main():
    os.umask(0o077)
    source = Path("/var/lib/harness-scope/server.db")
    directory = Path("/var/backups/harness-scope")
    directory.mkdir(mode=0o700, parents=True, exist_ok=True)
    stamp = datetime.now(timezone.utc).strftime("%Y%m%dT%H%M%S%fZ")
    target = directory / f"scope-{stamp}.sqlite"
    partial = target.with_suffix(".partial")
    try:
        with sqlite3.connect(f"file:{source}?mode=ro", uri=True) as src:
            with sqlite3.connect(partial) as dst:
                src.backup(dst, pages=256, sleep=0.1)
                if dst.execute("PRAGMA quick_check").fetchone()[0] != "ok":
                    raise RuntimeError("Backup integrity check failed")
        partial.replace(target)
    except BaseException:
        partial.unlink(missing_ok=True)
        raise
    for old in sorted(directory.glob("scope-*.sqlite"), reverse=True)[14:]:
        old.unlink()
    print(f"Verified SQLite backup: {target.name} ({target.stat().st_size} bytes)")


if __name__ == "__main__":
    main()
