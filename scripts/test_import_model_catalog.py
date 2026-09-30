import json
import hashlib
import unittest
from import_model_catalog import PROVIDERS, normalize


class CatalogImportTests(unittest.TestCase):
    def test_go_responses_uses_exact_reviewed_ids_and_source_limits(self):
        source = self.source()
        ids = ['grok-4.7', 'grok-4.6', 'gpt-6-luna', 'gpt-5.6-luna',
               'muse-spark-1.3-contributor', 'muse-spark-1.2-contributor']
        source['opencode-go']['api'] = 'https://opencode.ai/zen/go/v1'
        source['opencode-go']['models'] = {
            name: {'limit': {'context': 123456, 'input': 100000, 'output': 23456},
                   'modalities': {'input': ['text'], 'output': ['text']}, 'tool_call': True}
            for name in ids + ['grok-4.7-sibling']
        }
        raw = json.dumps(source).encode()
        result = normalize(raw)
        rows = [row for row in result['models'] if row['provider'] == 'open_ai_responses']
        self.assertEqual({row['model'] for row in rows}, set(ids))
        self.assertEqual(result, normalize(raw))
        for row in rows:
            self.assertEqual(row['capabilities']['source_sha256'], hashlib.sha256(raw).hexdigest())
            self.assertEqual(row['capabilities']['context_limit'], 123456)
            self.assertEqual(row['capabilities']['input_limit'], 100000)
            self.assertEqual(row['capabilities']['output_limit'], 23456)
            self.assertEqual(row['path'], '/zen/go/v1')

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

    def test_opencode_go_keeps_the_exact_gateway_identity_and_merged_limits(self):
        source = self.source()
        source["opencode-go"] = {
            "api": "https://opencode.ai/zen/go/v1",
            "models": {
                "glm-5.3": {
                    "limit": {"context": 1_000_000, "output": 131_072},
                    "modalities": {"input": ["text"], "output": ["text"]},
                    "tool_call": True,
                    "reasoning": True,
                    "reasoning_options": [
                        {"type": "effort", "values": ["low", "high", "max"]}
                    ],
                },
                "minimax-m3": {
                    "limit": {"context": 204_800, "output": 131_072},
                    "modalities": {"input": ["text"], "output": ["text"]},
                    "tool_call": True,
                },
                "grok-4.6": {
                    "limit": {"context": 2_000_000, "output": 131_072},
                    "modalities": {"input": ["text"], "output": ["text"]},
                    "tool_call": True,
                },
            },
        }

        rows = normalize(json.dumps(source).encode())["models"]
        go_rows = [item for item in rows if item["capabilities"]["source_provider"] == "opencode-go"]
        self.assertEqual([item["model"] for item in go_rows], ["glm-5.3", "grok-4.6", "minimax-m3"])
        row = go_rows[0]
        self.assertEqual(
            (row["provider"], row["host"], row["port"], row["path"], row["model"]),
            ("open_ai_compatible", "opencode.ai", 443, "/zen/go/v1", "glm-5.3"),
        )
        self.assertEqual(row["capabilities"]["context_limit"], 1_000_000)
        self.assertEqual(row["capabilities"]["output_limit"], 131_072)
        self.assertEqual(row["capabilities"]["reasoning_efforts"], ["low", "high", "max"])
        self.assertEqual(go_rows[2]["provider"], "anthropic")
