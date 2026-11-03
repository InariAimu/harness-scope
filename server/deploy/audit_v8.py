#!/usr/bin/env python3
"""Audit offline v7 -> v8 sync-server copies without printing user data.

Run against consistent backups with no concurrent writers. The only permitted
data changes are the new (initially NULL) model_usage projection column and one
query-cache version increment; all original facts and sync metadata must match.
"""

import argparse
import json
from pathlib import Path
import sqlite3

from audit_v6 import digest


def require(condition, message):
    if not condition:
        raise RuntimeError(message)


def audit(before, after):
    with sqlite3.connect(before.resolve().as_uri() + "?mode=ro", uri=True) as old, \
         sqlite3.connect(after.resolve().as_uri() + "?mode=ro", uri=True) as new:
        require(old.execute("PRAGMA user_version").fetchone()[0] == 7, "Expected v7 baseline")
        require(new.execute("PRAGMA user_version").fetchone()[0] == 8, "Expected v8 result")
        for conn in (old, new):
            require(conn.execute("PRAGMA quick_check").fetchall() == [("ok",)], "Integrity check failed")
        for table in ("file_state", "usage_read_cache"):
            require(old.execute(f'SELECT COUNT(*) FROM "{table}"').fetchone()[0] == 0,
                    "Expected a sync-only server")

        tables = "SELECT name FROM sqlite_master WHERE type='table' ORDER BY name"
        require(old.execute(tables).fetchall() == new.execute(tables).fetchall(), "Table set changed")
        preserved = {}
        for (table,) in old.execute(tables):
            if table in ("usage_index", "usage_state"):
                continue
            baseline = digest(old, table)
            require(baseline == digest(new, table), f"Original table changed: {table}")
            preserved[table] = {"rows": baseline[0], "sha256": baseline[1]}

        old_columns = old.execute("PRAGMA table_info(usage_index)").fetchall()
        new_columns = new.execute("PRAGMA table_info(usage_index)").fetchall()
        require(new_columns[:-1] == old_columns and
                new_columns[-1][1:] == ("model_usage", "TEXT", 0, None, 0),
                "Unexpected usage_index schema change")
        columns = ",".join('"' + c[1].replace('"', '""') + '"' for c in old_columns)
        for conn in (old, new):
            conn.execute(f"CREATE TEMP VIEW audit_v8_original_index AS SELECT {columns} FROM usage_index")
        require(digest(old, "audit_v8_original_index") == digest(new, "audit_v8_original_index"),
                "Original usage projection changed")
        require(new.execute("SELECT COUNT(*) FROM usage_index WHERE model_usage IS NOT NULL").fetchone()[0] == 0,
                "Unexpected model measurement during offline migration")

        state = old.execute("SELECT id, version FROM usage_state ORDER BY id").fetchall()
        require(len(state) == 1 and state[0][0] == 1, "Unexpected query-cache state")
        require(new.execute("SELECT id, version FROM usage_state ORDER BY id").fetchall() ==
                [(1, state[0][1] + 1)], "Unexpected query-cache version change")
        for table in ("records", "remote_records"):
            for operation in ("INSERT", "UPDATE"):
                trigger = new.execute("SELECT sql FROM sqlite_master WHERE type='trigger' AND name=?",
                                      (f"usage_index_{table}_{operation}",)).fetchone()
                require(trigger is not None and "$.modelUsage" in trigger[0],
                        f"Missing v8 projection trigger: {table}/{operation}")
        return {"schemaBefore": 7, "schemaAfter": 8, "quickCheck": "ok",
                "allOriginalFactsPreserved": True, "originalProjectionPreserved": True,
                "queryCacheVersionIncrement": 1, "tables": preserved}


if __name__ == "__main__":
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("before", type=Path)
    parser.add_argument("after", type=Path)
    parser.add_argument("--report", type=Path)
    args = parser.parse_args()
    result = audit(args.before, args.after)
    if args.report:
        args.report.write_text(json.dumps(result, indent=2) + "\n", encoding="utf-8")
    print(json.dumps({key: value for key, value in result.items() if key != "tables"}))
