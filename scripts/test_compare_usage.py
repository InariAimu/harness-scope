"""Regression checks for the read-only reconciliation tool (stdlib only)."""
from collections import defaultdict
from contextlib import closing
from decimal import Decimal
import json
from pathlib import Path
import sqlite3
import tempfile
import unittest

from compare_usage import add, bucket, ccswitch_groups, claude_groups, compare, opencode_groups


class ComparisonTests(unittest.TestCase):
    def test_claude_blocks_copies_and_cost_state_do_not_duplicate_requests(self):
        with tempfile.TemporaryDirectory() as temp:
            root = Path(temp)
            rows = [dict(type="assistant", sessionId="fixture", message=dict(
                id="msg", model="synthetic-model", usage=dict(input_tokens=10, output_tokens=output,
                cache_read_input_tokens=20, cache_creation_input_tokens=5,
                output_tokens_details=dict(thinking_tokens=3)))) for output in [5, 8, 6]]
            rows.append(dict(type="cost-state", totalCostUSD=999, modelUsage={"synthetic-model": dict(outputTokens=1000)}))
            body = "\n".join(json.dumps(row) for row in rows) + "\n"
            (root / "one.jsonl").write_text(body, encoding="utf-8")
            (root / "copy.jsonl").write_text(body, encoding="utf-8")
            reference = claude_groups(root, {"fixture"})
            group = reference[("fixture", "synthetic-model")]
            self.assertEqual((group["requests"], group["output"], group["totalTokens"]), (1, 8, 43))
            actual = defaultdict(bucket)
            add(actual, "fixture", "synthetic-model", dict(input=10, output=8, cacheRead=20, cacheWrite=5), "0.25")
            check = compare(actual, reference, compare_cost=False)
            self.assertEqual(check["status"], "passed")
            self.assertIn("not_compared", check["pricing"])
            self.assertEqual(claude_groups(root, {"other"}), {})

    def test_claude_invalid_counts_or_partial_json_never_pass(self):
        with tempfile.TemporaryDirectory() as temp:
            path = Path(temp) / "bad.jsonl"
            for value in [-1, True, 2.5]:
                row = dict(type="assistant", message=dict(id="msg", usage=dict(input_tokens=value, output_tokens=1)))
                path.write_text(json.dumps(row), encoding="utf-8")
                with self.assertRaises(ValueError):
                    claude_groups(path, set())
            path.write_text('{"type":', encoding="utf-8")
            with self.assertRaises(ValueError):
                claude_groups(path, set())

    def test_decimal_costs_and_missing_usage_are_not_rounded_or_hidden(self):
        actual, reference = defaultdict(bucket), defaultdict(bucket)
        tokens = dict(input=80, output=20, cacheRead=30, cacheWrite=10)
        for cost in ("0.00000001", "0.00000002"):
            add(actual, "ses_one", "model", tokens, cost)
        add(reference, "ses_one", "model", {k: v * 2 for k, v in tokens.items()}, "0.00000003", 2)
        self.assertEqual(compare(actual, reference)["status"], "passed")
        add(reference, "ses_missing", "model", tokens, "0")
        self.assertEqual(compare(actual, reference)["status"], "mismatch")

    def test_unknown_cost_and_absent_reference_never_pass(self):
        actual, reference = defaultdict(bucket), defaultdict(bucket)
        tokens = dict(input=1, output=1, cacheRead=0, cacheWrite=0)
        add(actual, "ses_one", "model", tokens, None)
        add(reference, "ses_one", "model", tokens, None)
        self.assertEqual(compare(actual, reference)["status"], "mismatch")
        self.assertEqual(compare(actual, {})["status"], "unavailable")

    def test_ccswitch_inclusive_input_semantics(self):
        with tempfile.TemporaryDirectory() as temp:
            path = Path(temp) / "ccswitch.db"
            with closing(sqlite3.connect(path)) as conn:
                conn.executescript("""PRAGMA user_version=20;
                    CREATE TABLE proxy_request_logs(app_type TEXT,data_source TEXT,status_code INTEGER,session_id TEXT,model TEXT,input_tokens INTEGER,output_tokens INTEGER,cache_read_tokens INTEGER,cache_creation_tokens INTEGER,input_token_semantics INTEGER,total_cost_usd TEXT);
                    INSERT INTO proxy_request_logs VALUES('codex','codex_session',200,'ses_one','model',120,20,30,10,1,'0.1');
                    INSERT INTO proxy_request_logs VALUES('codex','proxy',200,'ses_one','model',120,20,30,10,1,'0.1');
                    INSERT INTO proxy_request_logs VALUES('codex','codex_session',500,'ses_one','model',120,20,30,10,1,'0.1');""")
            group = ccswitch_groups(path, "codex", {"ses_one"})[("ses_one", "model")]
            self.assertEqual((group["requests"], group["input"], group["totalTokens"]), (1, 80, 140))
            self.assertEqual(group["costUsd"], Decimal("0.1"))

    def test_opencode_migration_uses_v2_once_and_includes_reasoning(self):
        with tempfile.TemporaryDirectory() as temp:
            path = Path(temp) / "opencode.db"
            with closing(sqlite3.connect(path)) as conn:
                conn.executescript("CREATE TABLE message(session_id TEXT,data TEXT); CREATE TABLE session_message(session_id TEXT,type TEXT,data TEXT);")
                d = dict(role="assistant", modelID="model", time=dict(completed=1), tokens=dict(input=100, output=20, reasoning=5, cache=dict(read=60, write=10)), cost=0)
                conn.execute("INSERT INTO message VALUES(?,?)", ("ses_one", json.dumps(d)))
                conn.execute("INSERT INTO session_message VALUES(?,?,?)", ("ses_one", "assistant", json.dumps(d)))
                conn.commit()
            group = opencode_groups(path, {"ses_one"})[("ses_one", "model")]
            self.assertEqual((group["requests"], group["output"], group["totalTokens"]), (1, 25, 195))


if __name__ == "__main__":
    unittest.main()
