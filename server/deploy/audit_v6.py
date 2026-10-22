#!/usr/bin/env python3
"""Read-only audit of an offline v4/v5 -> v6 database migration.

Use consistent SQLite backups, not a running database: concurrent sync changes
would correctly fail the exact comparison. No payloads or secrets are printed.
"""

import argparse
import hashlib
import json
from pathlib import Path
import sqlite3


def digest(conn, table):
    quoted = '"' + table.replace('"', '""') + '"'
    columns = len(conn.execute(f"PRAGMA table_info({quoted})").fetchall())
    order = ",".join(str(i + 1) for i in range(columns))
    count = 0
    checksum = hashlib.sha256()
    for row in conn.execute(f"SELECT * FROM {quoted} ORDER BY {order}"):
        payload = json.dumps(row, ensure_ascii=True, separators=(",", ":")).encode()
        checksum.update(len(payload).to_bytes(8, "big"))
        checksum.update(payload)
        count += 1
    return count, checksum.hexdigest()


def audit(before, after):
    with sqlite3.connect(before.resolve().as_uri() + "?mode=ro", uri=True) as old, \
         sqlite3.connect(after.resolve().as_uri() + "?mode=ro", uri=True) as new:
        if old.execute("PRAGMA user_version").fetchone()[0] not in (4, 5, 6):
            raise RuntimeError("Expected a v4/v5/v6 baseline")
        if new.execute("PRAGMA user_version").fetchone()[0] != 6:
            raise RuntimeError("Expected a v6 migration result")
        for conn in (old, new):
            if conn.execute("PRAGMA quick_check").fetchone()[0] != "ok":
                raise RuntimeError("Database integrity check failed")
        tables = [r[0] for r in old.execute("SELECT name FROM sqlite_master WHERE type='table' ORDER BY name")]
        for table in tables:
            baseline = digest(old, table)
            if baseline != digest(new, table):
                raise RuntimeError(f"Original table changed: {table}")
            print(f"Preserved {table}: {baseline[0]} rows")
        for table in ("shared_subscriptions", "subscription_observations", "server_subscription_observations", "provider_account_observations"):
            if not new.execute("SELECT 1 FROM sqlite_master WHERE type='table' AND name=?", (table,)).fetchone():
                raise RuntimeError(f"Missing v6 table: {table}")
        print("V6_AUDIT_OK: original tables and metadata preserved exactly")


if __name__ == "__main__":
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("before", type=Path)
    parser.add_argument("after", type=Path)
    args = parser.parse_args()
    audit(args.before, args.after)
