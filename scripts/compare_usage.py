"""Compare local usage with read-only CC Switch/OpenCode/Claude data.

Run after collecting the same completed CLI session in Harness Scope. Exit 1 means
a difference, incomplete pricing, or no comparable reference; unavailable sources
are reported explicitly. Claude native comparison checks counts and tokens only,
with pricing explicitly marked not compared. No CLIs or databases are modified.
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


def claude_groups(path, sessions):
    """Assistant usage snapshots only; cost-state is a different aggregate scope."""
    groups = defaultdict(bucket)
    messages = {}
    paths = [path] if path.is_file() else sorted(path.rglob("*.jsonl"))
    if not path.exists():
        raise ValueError(f"Claude log source does not exist: {path}")
    for source in paths:
        with source.open(encoding="utf-8") as stream:
            for line in stream:
                if not line.strip():
                    continue
                row = json.loads(line)
                if row.get("type") != "assistant":
                    continue
                message = row.get("message") or {}
                usage = message.get("usage")
                if not isinstance(usage, dict):
                    continue
                session = row.get("sessionId") or source.stem
                if sessions and session not in sessions:
                    continue
                if not message.get("id"):
                    raise ValueError("Claude assistant usage has no message.id")
                for required in ("input_tokens", "output_tokens"):
                    if required not in usage:
                        raise ValueError(f"Missing Claude {required}")
                tokens = {key: usage.get(native, 0) for key, native in (
                    ("input", "input_tokens"), ("output", "output_tokens"),
                    ("cacheRead", "cache_read_input_tokens"),
                    ("cacheWrite", "cache_creation_input_tokens"))}
                for key, value in tokens.items():
                    if type(value) is not int or not 0 <= value <= 1_000_000_000_000:
                        raise ValueError(f"Invalid Claude {key}")
                reasoning = (usage.get("output_tokens_details") or {}).get("thinking_tokens", 0)
                if type(reasoning) is not int or not 0 <= reasoning <= tokens["output"]:
                    raise ValueError("Invalid Claude reasoning subset")
                cache_hour = (usage.get("cache_creation") or {}).get("ephemeral_1h_input_tokens", 0)
                if type(cache_hour) is not int or not 0 <= cache_hour <= tokens["cacheWrite"]:
                    raise ValueError("Invalid Claude cache write subset")
                if sum(tokens.values()) == 0:
                    continue
                key = (session, message["id"])
                previous = messages.get(key)
                if previous is None or tokens["output"] >= previous[1]["output"]:
                    messages[key] = (message.get("model") or "unknown", tokens)
    for (session, _), (model, tokens) in messages.items():
        add(groups, session, model, tokens, None)
    return groups


def plain(group):
    return {k: str(v) if isinstance(v, Decimal) else v for k, v in group.items()}


def compare(actual, reference, compare_cost=True):
    if not reference:
        return dict(status="unavailable", reason="No completed reference records for the selected sessions", groups=[])
    details = []
    for key in sorted(actual.keys() | reference.keys()):
        left, right = actual.get(key, bucket()), reference.get(key, bucket())
        keys = left.keys() if compare_cost else left.keys() - {"costUsd", "unknownCosts"}
        differences = {k: dict(scope=str(left[k]), reference=str(right[k])) for k in keys if left[k] != right[k]}
        if compare_cost and (left["unknownCosts"] or right["unknownCosts"]):
            differences["pricing"] = dict(scope="unknown" if left["unknownCosts"] else "known", reference="unknown" if right["unknownCosts"] else "known")
        details.append(dict(sessionId=key[0], model=key[1], scope=plain(left), reference=plain(right), differences=differences))
    result = dict(status="mismatch" if any(d["differences"] for d in details) else "passed", groups=details)
    if not compare_cost:
        result["pricing"] = "not_compared: Claude JSONL has no verified per-request billed cost; cost-state is excluded"
    return result


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--scope-db", type=Path, required=True)
    parser.add_argument("--ccswitch-db", type=Path)
    parser.add_argument("--opencode-db", type=Path)
    parser.add_argument("--claude-projects", type=Path, help="Claude projects directory or one JSONL file; compares request counts and tokens only")
    parser.add_argument("--harness", choices=["codex", "opencode", "claude"], required=True)
    parser.add_argument("--session-id", action="append", default=[])
    parser.add_argument("--report", type=Path)
    args = parser.parse_args()
    if not args.ccswitch_db and not args.opencode_db and not args.claude_projects:
        parser.error("Provide --ccswitch-db, --opencode-db or --claude-projects")
    if args.opencode_db and args.harness != "opencode":
        parser.error("--opencode-db requires --harness opencode")
    if args.claude_projects and args.harness != "claude":
        parser.error("--claude-projects requires --harness claude")
    actual = scope_groups(args.scope_db, args.harness, set(args.session_id))
    checks = {}
    if args.ccswitch_db:
        checks["ccswitch"] = compare(actual, ccswitch_groups(args.ccswitch_db, args.harness, set(args.session_id)))
    if args.opencode_db:
        checks["opencodeNative"] = compare(actual, opencode_groups(args.opencode_db, set(args.session_id)))
    if args.claude_projects:
        checks["claudeNativeTokens"] = compare(actual, claude_groups(args.claude_projects, set(args.session_id)), compare_cost=False)
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
