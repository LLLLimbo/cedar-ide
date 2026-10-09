#!/usr/bin/env python3
"""Strict fixed receipt checks; no process launch, SSH or deployment."""
import copy
import json
import unittest
from unittest import mock

import linux_agent_bundle_acceptance as acceptance


def valid():
    return {
        **{key: True for key in acceptance.BOOLS}, **acceptance.FIXED,
        "kind": "cedar_linux_agent_bundle_probe", "status": "success",
        "capability_count": 26, "explicit_client_calls": 76, "elapsed_ms": 123,
    }


class ReceiptTests(unittest.TestCase):
    def test_complete_receipt_and_exact_elapsed_boundary(self):
        for elapsed in (0, 30000):
            receipt = valid()
            receipt["elapsed_ms"] = elapsed
            self.assertEqual(acceptance.validate_probe(receipt), receipt)
            output = b"running 1 test\n" + json.dumps(receipt).encode() + b"\ntest result: ok\n"
            self.assertEqual(acceptance.parse_probe(output), receipt)

    def test_every_required_witness_is_boolean_true(self):
        for key in acceptance.BOOLS:
            for replacement in (False, None, 1, "true", [], {}):
                with self.subTest(key=key, replacement=replacement):
                    receipt = valid()
                    receipt[key] = replacement
                    with self.assertRaises(ValueError):
                        acceptance.validate_probe(receipt)

    def test_missing_extra_and_wrong_root_shapes_rejected(self):
        for key in valid():
            receipt = valid()
            del receipt[key]
            with self.subTest(key=key), self.assertRaises(ValueError):
                acceptance.validate_probe(receipt)
        for receipt in (None, [], [valid()], [[valid()]], True, "success",
                        {**valid(), "private_extra": "discarded"}):
            with self.subTest(receipt=type(receipt).__name__), self.assertRaises(ValueError):
                acceptance.validate_probe(receipt)

    def test_fixed_integer_identity_and_budgets_do_not_coerce(self):
        for key, expected in acceptance.FIXED.items():
            for value in (True, False, str(expected), float(expected), expected + 1, None):
                receipt = valid()
                receipt[key] = value
                with self.subTest(key=key, value=value), self.assertRaises(ValueError):
                    acceptance.validate_probe(receipt)

    def test_bounded_counts_late_results_and_nonfinite_values_rejected(self):
        wrong = {
            "capability_count": (0, 33, -1, True, 1.0, "26"),
            "explicit_client_calls": (0, 97, -1, True, 76.0, "76"),
            "elapsed_ms": (-1, 30001, True, 0.0, float("nan"), float("inf")),
        }
        for key, values in wrong.items():
            for value in values:
                receipt = valid()
                receipt[key] = value
                with self.subTest(key=key, value=value), self.assertRaises(ValueError):
                    acceptance.validate_probe(receipt)
        receipt = valid()
        receipt["elapsed_ms"] = float("nan")
        with self.assertRaises(ValueError):
            acceptance.parse_probe(json.dumps(receipt).encode())

    def test_failure_and_wrong_kind_rejected(self):
        for key, value in (("kind", "other"), ("status", "failed"), ("status", True)):
            receipt = valid()
            receipt[key] = value
            with self.assertRaises(ValueError):
                acceptance.validate_probe(receipt)

    def test_duplicate_receipts_keys_malformed_and_overflow_rejected(self):
        encoded = json.dumps(valid()).encode()
        for output in (b"", b"running no receipt", encoded + b"\n" + encoded,
                       encoded[:-1] + b', "status": "success"}', b"{broken}",
                       b"\xff", b"x" * (acceptance.MAX_TEST_OUTPUT + 1)):
            with self.subTest(size=len(output)), self.assertRaises((ValueError, UnicodeError)):
                acceptance.parse_probe(output)

    def test_gate_validates_build_host_before_touching_output(self):
        with mock.patch.object(acceptance.bundle, "require_build_host",
                               side_effect=ValueError("unsupported host")):
            with mock.patch.object(acceptance.Path, "resolve") as resolve:
                with self.assertRaisesRegex(ValueError, "unsupported host"):
                    acceptance.run("unused", "unused", "a" * 40,
                                   "https://github.com/LLLLimbo/cedar-ide/actions/runs/1")
                resolve.assert_not_called()

    def test_validation_does_not_change_receipt(self):
        receipt = valid()
        before = copy.deepcopy(receipt)
        acceptance.validate_probe(receipt)
        self.assertEqual(receipt, before)


if __name__ == "__main__":
    unittest.main()
