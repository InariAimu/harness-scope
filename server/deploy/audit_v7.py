#!/usr/bin/env python3
"""Audit an offline v6-to-v7 server migration; never prints payloads or secrets."""
import argparse
import json
from pathlib import Path
import sqlite3
from audit_v6 import digest


def audit(before, after):
    with sqlite3.connect(before.resolve().as_uri() + "?mode=ro", uri=True) as old, sqlite3.connect(after.resolve().as_uri() + "?mode=ro", uri=True) as new:
        assert old.execute("PRAGMA user_version").fetchone()[0] == 6, "Expected v6 baseline"
        assert new.execute("PRAGMA user_version").fetchone()[0] == 7, "Expected v7 result"
        for c in (old, new):
            assert c.execute("PRAGMA quick_check").fetchone()[0] == "ok"
        # These are local parse caches intentionally invalidated by v7. A
        # production sync server must not contain local source scan state.
        for table in ("file_state", "usage_read_cache"):
            assert old.execute(f'SELECT COUNT(*) FROM "{table}"').fetchone()[0] == 0, "Expected a sync-only server"
        preserved = {}
        for (table,) in old.execute("SELECT name FROM sqlite_master WHERE type='table' ORDER BY name"):
            baseline = digest(old, table)
            assert baseline == digest(new, table), f"Original table changed: {table}"
            preserved[table] = {"rows": baseline[0], "sha256": baseline[1]}
        for table in ("record_sources", "record_retire_candidates", "claude_event_bindings"):
            assert new.execute("SELECT 1 FROM sqlite_master WHERE type='table' AND name=?", (table,)).fetchone(), f"Missing v7 table: {table}"
        return {"schemaBefore": 6, "schemaAfter": 7, "quickCheck": "ok", "allOriginalTablesPreserved": True, "tables": preserved}


if __name__ == "__main__":
    p = argparse.ArgumentParser(description=__doc__)
    p.add_argument("before", type=Path); p.add_argument("after", type=Path); p.add_argument("--report", type=Path)
    args = p.parse_args()
    report = audit(args.before, args.after)
    if args.report:
        args.report.write_text(json.dumps(report, indent=2) + "\n", encoding="utf-8")
    print(json.dumps({k: v for k, v in report.items() if k != "tables"}))
