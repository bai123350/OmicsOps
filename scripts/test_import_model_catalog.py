import json
import unittest
from import_model_catalog import PROVIDERS, normalize


class CatalogImportTests(unittest.TestCase):
    def source(self):
        return {key: {"api": "https://example.test/v1", "models": {}}
                for key in PROVIDERS}

    def test_exact_ids_limits_and_missing_capabilities_are_not_inferred(self):
        source = self.source()
        source["openai"]["models"] = {
            "exact": {"limit": {"context": 100, "output": 10},
                      "modalities": {"output": ["text"]}},
            "exact-sibling": {"limit": {"context": 200, "output": 20},
                              "modalities": {"input": ["image"], "output": ["text"]},
                              "tool_call": True, "reasoning": True,
                              "reasoning_options": [{"type": "effort", "values": ["max"]}]},
            "embedding": {"limit": {"context": 100, "output": 10}},
            "invalid": {"limit": {"context": 100, "output": 0},
                        "modalities": {"output": ["text"]}},
        }
        raw = json.dumps(source).encode()
        result = normalize(raw)
        self.assertEqual(result, normalize(raw))
        plain, sibling = result["models"]
        self.assertEqual(plain["model"], "exact")
        self.assertFalse(plain["supports_tools"])
        self.assertFalse(plain["supports_vision"])
        self.assertIsNone(plain["capabilities"]["reasoning_efforts"])
        self.assertEqual(sibling["model"], "exact-sibling")
        self.assertEqual(sibling["capabilities"]["reasoning_efforts"], ["max"])
        self.assertEqual(sibling["capabilities"]["source_sha256"], result["source_sha256"])

    def test_rejects_non_https_source_endpoints(self):
        source = self.source()
        source["deepseek"]["api"] = "http://example.test/v1"
        with self.assertRaises(ValueError):
            normalize(json.dumps(source).encode())
