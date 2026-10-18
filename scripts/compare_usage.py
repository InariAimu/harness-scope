"""Compare local usage with read-only CC Switch/OpenCode data, without transcripts.

Run after collecting the same completed CLI session in Harness Scope. Exit 1 means
a difference, incomplete pricing, or no comparable reference; unavailable sources
are reported explicitly. The script never launches CLIs or changes a database.
"""
import argparse
from collections import defaultdict
from contextlib import closing
from decimal import Decimal
import json
from pathlib import Path
import sqlite3


def connect(path):
    conn = sqlite3.connect(Path(path).resolve().as_uri() + "?mode=ro", uri=True)
    conn.row_factory = sqlite3.Row
    conn.execute("PRAGMA query_only=ON")
    conn.execute("BEGIN")  # all tables belong to one consistent WAL snapshot
    return conn


def bucket():
    return dict(requests=0, input=0, output=0, cacheRead=0, cacheWrite=0,
                totalTokens=0, costUsd=Decimal(0), unknownCosts=0)


def add(groups, session, model, tokens, cost, count=1):
    group = groups[(session or "", model)]
    group["requests"] += count
    for key in ("input", "output", "cacheRead", "cacheWrite"):
        value = tokens[key]
        if not isinstance(value, int) or value < 0:
            raise ValueError(f"Invalid {key} for {session}/{model}")
        group[key] += value
        group["totalTokens"] += value
    if cost is None:
        group["unknownCosts"] += count
    else:
        group["costUsd"] += Decimal(str(cost))


def scope_groups(path, harness, sessions):
    groups = defaultdict(bucket)
    with closing(connect(path)) as conn:
        device = json.loads(conn.execute("SELECT value FROM meta WHERE key='device_id'").fetchone()[0])
        for row in conn.execute("SELECT payload FROM records WHERE device_id=?", (device,)):
            r = json.loads(row[0])
            if (r["deleted"] or r["harness"] != harness or r["recordSource"] != "session"
                    or (sessions and r.get("sessionId") not in sessions)):
                continue
            add(groups, r.get("sessionId"), r["model"], r["tokens"], r["cost"]["amountUsd"], r["count"])
    return groups


def ccswitch_groups(path, harness, sessions):
    groups = defaultdict(bucket)
    with closing(connect(path)) as conn:
        version = conn.execute("PRAGMA user_version").fetchone()[0]
        if version != 20:
            raise ValueError(f"CC Switch schema v20 required; found v{version}")
        # Compare native session ingestion, excluding proxy requests and day rollups.
        for r in conn.execute("SELECT * FROM proxy_request_logs WHERE app_type=? AND data_source=? AND status_code BETWEEN 200 AND 299", (harness, harness + "_session")):
            if sessions and r["session_id"] not in sessions:
                continue
            read, write, input_tokens = r["cache_read_tokens"], r["cache_creation_tokens"], r["input_tokens"]
            if harness == "codex" and r["input_token_semantics"] != 2:
                input_tokens -= read
                if r["input_token_semantics"] == 1:
                    input_tokens -= write
            add(groups, r["session_id"], r["model"], dict(input=input_tokens, output=r["output_tokens"], cacheRead=read, cacheWrite=write), r["total_cost_usd"])
    return groups


def opencode_groups(path, sessions):
    groups = defaultdict(bucket)
    with closing(connect(path)) as conn:
        tables = {r[0] for r in conn.execute("SELECT name FROM sqlite_master WHERE type='table'")}
        migrated = set()
        rows = []
        if "session_message" in tables:
            for r in conn.execute("SELECT session_id,type,data FROM session_message"):
                migrated.add(r["session_id"])
                if r["type"] == "assistant":
                    rows.append((r["session_id"], json.loads(r["data"])))
        if "message" in tables:
            for r in conn.execute("SELECT session_id,data FROM message"):
                if r["session_id"] not in migrated:
                    d = json.loads(r["data"])
                    if d.get("role") == "assistant":
                        rows.append((r["session_id"], d))
        for session, d in rows:
            if sessions and session not in sessions:
                continue
            if not d.get("time", {}).get("completed"):
                continue
            u = d["tokens"]
            model = d.get("modelID") or d["model"]["id"]
            add(groups, session, model, dict(input=u["input"], output=u["output"] + u.get("reasoning", 0), cacheRead=u.get("cache", {}).get("read", 0), cacheWrite=u.get("cache", {}).get("write", 0)), d.get("cost"))
    return groups


def plain(group):
    return {k: str(v) if isinstance(v, Decimal) else v for k, v in group.items()}


def compare(actual, reference):
    if not reference:
        return dict(status="unavailable", reason="No completed reference records for the selected sessions", groups=[])
    details = []
    for key in sorted(actual.keys() | reference.keys()):
        left, right = actual.get(key, bucket()), reference.get(key, bucket())
        differences = {k: dict(scope=str(left[k]), reference=str(right[k])) for k in left if left[k] != right[k]}
        if left["unknownCosts"] or right["unknownCosts"]:
            differences["pricing"] = dict(scope="unknown" if left["unknownCosts"] else "known", reference="unknown" if right["unknownCosts"] else "known")
        details.append(dict(sessionId=key[0], model=key[1], scope=plain(left), reference=plain(right), differences=differences))
    return dict(status="mismatch" if any(d["differences"] for d in details) else "passed", groups=details)


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--scope-db", type=Path, required=True)
    parser.add_argument("--ccswitch-db", type=Path)
    parser.add_argument("--opencode-db", type=Path)
    parser.add_argument("--harness", choices=["codex", "opencode"], required=True)
    parser.add_argument("--session-id", action="append", default=[])
    parser.add_argument("--report", type=Path)
    args = parser.parse_args()
    if not args.ccswitch_db and not args.opencode_db:
        parser.error("Provide --ccswitch-db or --opencode-db")
    if args.opencode_db and args.harness != "opencode":
        parser.error("--opencode-db requires --harness opencode")
    actual = scope_groups(args.scope_db, args.harness, set(args.session_id))
    checks = {}
    if args.ccswitch_db:
        checks["ccswitch"] = compare(actual, ccswitch_groups(args.ccswitch_db, args.harness, set(args.session_id)))
    if args.opencode_db:
        checks["opencodeNative"] = compare(actual, opencode_groups(args.opencode_db, set(args.session_id)))
    passed = any(c["status"] == "passed" for c in checks.values()) and not any(c["status"] == "mismatch" for c in checks.values())
    report = dict(harness=args.harness, status="passed" if passed else "unverified", checks=checks)
    output = json.dumps(report, ensure_ascii=False, indent=2)
    if args.report:
        args.report.parent.mkdir(parents=True, exist_ok=True)
        args.report.write_text(output + "\n", encoding="utf-8")
    print(output)
    return 0 if passed else 1


if __name__ == "__main__":
    raise SystemExit(main())
