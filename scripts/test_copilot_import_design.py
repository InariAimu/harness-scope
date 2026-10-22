"""Validate legacy compatibility and future modern Copilot design vectors."""
from decimal import Decimal
import json
from pathlib import Path
import unittest


FIXTURES = Path(__file__).resolve().parents[1] / "tests" / "fixtures"
CONTRACT = json.loads((FIXTURES / "copliot-import-contract.json").read_text(encoding="utf-8"))
CASES = {case["id"]: case for case in CONTRACT["cases"]}


def fixture(case_id):
    return FIXTURES / CASES[case_id]["fixture"]


def read_json(path):
    return json.loads(path.read_text(encoding="utf-8"), parse_float=Decimal)


def read_events(path):
    return [json.loads(line, parse_float=Decimal)
            for line in path.read_text(encoding="utf-8").splitlines() if line.strip()]


class SampleError(ValueError):
    pass


def replay_fixture(path):
    """Reference mutation replay for the design vectors, with strict path checks."""
    state = None
    for line in path.read_text(encoding="utf-8").splitlines():
        if not line.strip():
            continue
        try:
            operation = json.loads(line, parse_float=Decimal)
        except json.JSONDecodeError as error:
            raise SampleError("incomplete_json") from error
        kind = operation["kind"]
        if kind == 0:
            state = operation["v"]
            continue
        if state is None:
            raise SampleError("missing_initial_state")
        keys = operation["k"]
        if not keys:
            continue
        target = state
        try:
            for key in keys[:-1]:
                target = target[key]
            key = keys[-1]
            if kind == 1:
                target[key] = operation["v"]
            elif kind == 2:
                array = (target.get(key) if isinstance(target, dict) else target[key]) or []
                if "i" in operation:
                    del array[operation["i"]:]
                array.extend(operation.get("v") or [])
                target[key] = array
            elif kind == 3:
                if isinstance(target, dict):
                    target.pop(key, None)
                else:
                    target[key] = None
            else:
                raise SampleError("unknown_operation")
        except (IndexError, KeyError, TypeError) as error:
            raise SampleError("invalid_operation_path") from error
    if state is None:
        raise SampleError("missing_initial_state")
    return state


def user_requests(state):
    return [row for row in state["requests"] if not row.get("isSystemInitiated", False)]


class CopilotImportDesignTests(unittest.TestCase):
    def test_only_legacy_is_implemented_and_every_fixture_exists(self):
        self.assertEqual(CONTRACT["harnesses"], ["copliot-deprecated", "copliot"])
        self.assertTrue(CONTRACT["productionImporterImplemented"])
        self.assertEqual(CONTRACT["implementedHarnesses"], ["copliot-deprecated", "copliot"])
        self.assertEqual(len(CASES), len(CONTRACT["cases"]))
        for case in CASES.values():
            with self.subTest(case=case["id"]):
                path = fixture(case["id"]).resolve()
                self.assertTrue(path.is_relative_to(FIXTURES.resolve()))
                self.assertTrue(path.is_file())
                self.assertIn(case["harness"], CONTRACT["harnesses"])

    def test_legacy_missing_body_and_tokens_still_has_user_requests(self):
        case = CASES["legacy_missing_content_and_partial_usage"]
        state = read_json(fixture(case["id"]))
        expected = case["expected"]
        requests = user_requests(state)
        self.assertEqual(len(requests), expected["observedUserRequests"])
        self.assertEqual(len(state["requests"]) - len(requests), expected["systemRequests"])
        first, second = requests
        self.assertNotIn("message", first)
        self.assertIsNone(first.get("promptTokens"))
        self.assertIsNone(first.get("completionTokens"))
        self.assertTrue(first["requestId"])
        self.assertGreater(first["timestamp"], 0)
        self.assertTrue(all(value is None for value in expected["firstRequest"].values()))
        usage = second["result"]["usage"]
        self.assertEqual(usage["promptTokens"], expected["secondRequest"]["reportedPromptTokens"])
        self.assertEqual(usage["completionTokens"], expected["secondRequest"]["reportedOutputTokens"])
        self.assertGreater(len(second["result"]["metadata"]["toolCallRounds"]), 1)
        self.assertNotIn("modelTotals", second)
        self.assertIsNone(expected["secondRequest"]["wholeTurnTokens"])

    def test_modern_append_does_not_change_the_pinned_legacy_assignment(self):
        case = CASES["legacy_assignment_survives_modern_append"]
        state = replay_fixture(fixture(case["id"]))
        expected = case["expected"]
        self.assertEqual(case["harness"], "copliot-deprecated")
        self.assertEqual(len(user_requests(state)), expected["observedUserRequests"])
        charges = [r["copilotCredits"] for r in state["requests"] if "copilotCredits" in r]
        self.assertEqual(sum(charges, Decimal(0)), Decimal(expected["copilotCreditsKnownSum"]))
        self.assertEqual(sum("copilotCredits" not in r for r in state["requests"]), expected["creditsMissingRequests"])
        self.assertEqual(expected["reclassifiedSessions"], 0)
        self.assertEqual(state["requests"][0]["requestId"], "continued-old")

    def test_modern_v3_replay_keeps_latest_identity_and_decimal_credits(self):
        case = CASES["modern_v3_mutations_zero_and_multiple_calls"]
        state = replay_fixture(fixture(case["id"]))
        expected = case["expected"]
        self.assertEqual(state["version"], 3)
        self.assertEqual(read_json(fixture("legacy_missing_content_and_partial_usage"))["version"], 3)
        requests = user_requests(state)
        self.assertEqual(len(requests), expected["observedUserRequests"])
        self.assertEqual([r["requestId"] for r in requests], expected["nativeRequestIds"])
        self.assertEqual(len(set(r["requestId"] for r in requests)), len(requests))
        first = requests[0]
        self.assertNotIn("pendingMarker", first)
        self.assertEqual(first["response"], [{"value": "Synthetic final response"}])
        self.assertEqual(first["completionTokens"], expected["firstRequest"]["reportedOutputTokens"])
        self.assertEqual(first["promptTokens"], expected["firstRequest"]["reportedPromptTokens"])
        self.assertEqual(first["result"]["metadata"]["resolvedModel"], expected["firstRequest"]["resolvedModel"])
        self.assertEqual(sum((r["copilotCredits"] for r in requests if "copilotCredits" in r), Decimal(0)), Decimal(expected["copilotCreditsKnownSum"]))
        self.assertEqual(sum("copilotCredits" in r and r["copilotCredits"] == 0 for r in requests), expected["knownZeroCreditRequests"])
        self.assertEqual(sum("copilotCredits" not in r for r in requests), expected["creditsMissingRequests"])
        self.assertNotIn("cacheReadTokens", first)
        self.assertNotIn("reasoningTokens", first)
        self.assertIsNone(expected["firstRequest"]["cacheReadTokens"])
        self.assertIsNone(expected["cashUsd"])

    def test_explicit_zero_tokens_and_absent_credits_remain_distinct(self):
        case = CASES["modern_v3_mutations_zero_and_multiple_calls"]
        request = replay_fixture(fixture(case["id"]))["requests"][2]
        expected = case["expected"]["thirdRequest"]
        self.assertEqual(request["promptTokens"], expected["reportedPromptTokens"])
        self.assertEqual(request["completionTokens"], expected["reportedOutputTokens"])
        self.assertEqual(request["completionTokens"], 0)
        self.assertNotIn("copilotCredits", request)
        self.assertIsNone(expected["copilotCredits"])
        legacy = read_json(fixture("legacy_missing_content_and_partial_usage"))["requests"][0]
        self.assertIsNone(legacy.get("completionTokens"))
        self.assertNotEqual(request["completionTokens"], legacy.get("completionTokens"))

    def test_multicall_last_input_and_accumulated_output_are_different_scopes(self):
        case = CASES["modern_v3_mutations_zero_and_multiple_calls"]
        request = replay_fixture(fixture(case["id"]))["requests"][1]
        expected = case["expected"]["secondRequest"]
        self.assertEqual(request["promptTokens"], expected["reportedPromptTokens"])
        self.assertEqual(request["completionTokens"], expected["reportedAccumulatedOutputTokens"])
        self.assertEqual(request["result"]["metadata"]["outputTokens"], expected["reportedLastCallOutputTokens"])
        self.assertNotEqual(request["completionTokens"], request["result"]["metadata"]["outputTokens"])
        self.assertEqual(len(request["result"]["metadata"]["toolCallRounds"]), 2)
        self.assertNotIn("modelTotals", request)
        self.assertIsNone(expected["wholeTurnTokens"])

    def test_incomplete_json_and_invalid_path_are_not_complete_sources(self):
        for case_id in ("modern_tail_is_incomplete", "modern_invalid_mutation_path"):
            case = CASES[case_id]
            with self.subTest(case=case_id):
                with self.assertRaisesRegex(SampleError, case["expected"]["error"]):
                    replay_fixture(fixture(case_id))
                self.assertFalse(case["expected"]["complete"])
                self.assertFalse(case["expected"]["revokePreviouslyImportedRecords"])

    def test_agent_host_parent_total_already_includes_child(self):
        case = CASES["agent_host_whole_turn_excludes_double_counting"]
        data = read_json(fixture(case["id"]))
        expected = case["expected"]
        root = data["root"]
        usage = root["turn_usage"][0]["usage"]
        meta = usage["_meta"]
        totals = meta["turnTokenTotals"]
        child = data["child"]["turn_usage"][0]["usage"]
        self.assertEqual(root["turns"][0]["id"], root["turn_usage"][0]["turn_id"])
        self.assertNotEqual(root["turns"][0]["id"], root["turns"][0]["event_id"])
        self.assertEqual(len(totals), expected["models"])
        self.assertEqual(sum(t["inputTokens"] for t in totals), expected["reportedWholeTurnInputTokens"])
        self.assertEqual(sum(t["outputTokens"] for t in totals), expected["reportedWholeTurnOutputTokens"])
        self.assertEqual(sum(t["cachedTokens"] for t in totals), expected["reportedCachedTokens"])
        self.assertEqual(totals[1], child["_meta"]["directTurnTokenTotals"][0])
        self.assertNotEqual(sum(t["inputTokens"] for t in totals) + child["inputTokens"] + usage["inputTokens"], expected["reportedWholeTurnInputTokens"])
        self.assertEqual(Decimal(meta["copilotUsage"]["totalNanoAiu"]), Decimal(expected["turnNanoAiUnits"]))
        self.assertEqual(Decimal(meta["copilotUsage"]["sessionTotalNanoAiu"]), Decimal(expected["sessionNanoAiUnitsSnapshot"]))
        self.assertFalse(expected["addChildUsageAgain"])

    def test_cli_user_requests_calls_and_native_charge_units_are_distinct(self):
        case = CASES["cli_snapshot_units_and_granularity"]
        events = read_events(fixture(case["id"]))
        expected = case["expected"]
        self.assertEqual(len({e["id"] for e in events}), len(events))
        self.assertEqual(sum(e["type"] == "user.message" for e in events), expected["observedUserRequests"])
        self.assertFalse(any(e["type"] == "assistant.usage" for e in events))
        shutdown = events[-1]["data"]
        metrics = list(shutdown["modelMetrics"].values())
        self.assertEqual(sum(m["requests"]["count"] for m in metrics), expected["apiCalls"])
        self.assertNotEqual(expected["observedUserRequests"], expected["apiCalls"])
        self.assertEqual(Decimal(shutdown["totalPremiumRequests"]), Decimal(expected["premiumRequestUnits"]))
        self.assertEqual(Decimal(shutdown["totalNanoAiu"]), Decimal(expected["sessionNanoAiUnitsSnapshot"]))
        checkpoint = events[-2]["data"]["totalNanoAiu"]
        self.assertNotEqual(Decimal(checkpoint + shutdown["totalNanoAiu"]), Decimal(expected["sessionNanoAiUnitsSnapshot"]))
        self.assertEqual(sum(m["usage"]["inputTokens"] for m in metrics), expected["reportedSessionInputTokens"])
        self.assertEqual(sum(m["usage"]["outputTokens"] for m in metrics), expected["reportedSessionOutputTokens"])
        self.assertEqual(expected["usageScope"], "session_model")
        self.assertIsNone(expected["cashUsd"])

    def test_transcript_and_chat_snapshot_share_a_native_session(self):
        case = CASES["transcript_is_not_another_import"]
        events = read_events(fixture(case["id"]))
        session = read_json(fixture("legacy_missing_content_and_partial_usage"))
        self.assertEqual(events[0]["data"]["sessionId"], session["sessionId"])
        self.assertEqual(session["sessionId"], case["expected"]["matchesSession"])
        self.assertFalse(any(e["type"] in ("assistant.usage", "session.shutdown") for e in events))
        self.assertEqual(case["expected"]["additionalSessions"], 0)
        self.assertEqual(case["expected"]["additionalUserRequests"], 0)


if __name__ == "__main__":
    unittest.main()
