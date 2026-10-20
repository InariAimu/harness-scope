#!/usr/bin/env python3
"""Read-only comparison of a pre-upgrade SQLite snapshot and its v4 migration."""
import argparse
import hashlib
import json
from pathlib import Path
import sqlite3


def open_readonly(path):
    conn = sqlite3.connect(path.resolve().as_uri() + "?mode=ro", uri=True)
    conn.execute("PRAGMA query_only=ON")
    conn.execute("BEGIN")
    return conn


def audit(before, after):
    assert after.execute("PRAGMA user_version").fetchone()[0] == 4
    report = {}
    for key in ("device_id", "server_instance_id", "config"):
        assert before.execute("SELECT value FROM meta WHERE key=?", (key,)).fetchone() == after.execute("SELECT value FROM meta WHERE key=?", (key,)).fetchone(), key
    device = json.loads(before.execute("SELECT value FROM meta WHERE key='device_id'").fetchone()[0])
    backfilled = 0
    for table in ("records", "remote_records", "server_records"):
        old = before.execute(f"SELECT device_id,id,revision,payload FROM {table} ORDER BY device_id,id")
        new = after.execute(f"SELECT device_id,id,revision,payload FROM {table} ORDER BY device_id,id")
        digest = hashlib.sha256()
        count = 0
        for a, b in zip(old, new, strict=True):
            assert a[:2] == b[:2], (table, "identity")
            ra, rb = json.loads(a[3]), json.loads(b[3])
            ta, tb = ra.pop("timing", None), rb.pop("timing", None)
            ra.pop("revision")
            rb.pop("revision")
            assert ra == rb, (table, "billing or metadata changed", a[:2])
            revision_delta = 0
            if ta != tb:
                assert table == "records" and a[0] == device and not ra["deleted"]
                stored = before.execute("SELECT payload FROM record_timings WHERE device_id=? AND id=?", a[:2]).fetchone()
                assert stored and json.loads(stored[0]) == tb
                revision_delta = 1
                backfilled += 1
                pending = after.execute("SELECT revision,payload FROM outbox WHERE id=?", (a[1],)).fetchone()
                assert pending and pending[0] == b[2] and json.loads(pending[1]) == json.loads(b[3])
            assert b[2] == a[2] + revision_delta, (table, "revision")
            digest.update(json.dumps(ra, sort_keys=True, separators=(",", ":")).encode())
            count += 1
        report[table] = {"rows": count, "unchanged_facts_sha256": digest.hexdigest()}
    assert list(before.execute("SELECT device_id,id,payload FROM record_timings ORDER BY device_id,id")) == list(after.execute("SELECT device_id,id,payload FROM record_timings ORDER BY device_id,id"))
    assert list(before.execute("SELECT device_id,id,source_path FROM records ORDER BY device_id,id")) == list(after.execute("SELECT device_id,id,source_path FROM records ORDER BY device_id,id"))
    assert list(before.execute("SELECT device_id,id,MAX(seq) FROM server_changes GROUP BY device_id,id ORDER BY device_id,id")) == list(after.execute("SELECT device_id,id,seq FROM server_changes ORDER BY device_id,id"))
    report["backfilled_timings"] = backfilled
    report["old_change_rows"] = before.execute("SELECT COUNT(*) FROM server_changes").fetchone()[0]
    report["new_change_rows"] = after.execute("SELECT COUNT(*) FROM server_changes").fetchone()[0]
    report["pending_records"] = after.execute("SELECT COUNT(*) FROM outbox").fetchone()[0]
    assert after.execute("PRAGMA quick_check").fetchone()[0] == "ok"
    return report


def main():
    parser = argparse.ArgumentParser()
    parser.add_argument("before", type=Path)
    parser.add_argument("after", type=Path)
    args = parser.parse_args()
    with open_readonly(args.before) as before, open_readonly(args.after) as after:
        report = audit(before, after)
    report["before_bytes"] = args.before.stat().st_size
    report["after_bytes"] = args.after.stat().st_size
    print(json.dumps(report, ensure_ascii=True))


if __name__ == "__main__":
    main()
