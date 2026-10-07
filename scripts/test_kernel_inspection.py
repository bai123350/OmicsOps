"""Pure metadata tests: load only inspection definitions, never a worker loop."""
import ast
import json
import pathlib
import re
import unittest


ROOT = pathlib.Path(__file__).resolve().parents[1]
WORKERS = ("crates/omicsops-runtime/src/process_backend.rs",
           "crates/omicsops-adapters/src/kernel.rs")


def load_worker(source_path, language="PYTHON"):
    """Read the literal/include_str concat used by the compiled worker."""
    source_file = ROOT / source_path
    source = source_file.read_text(encoding="utf-8")
    literal = re.search(r'const ' + language + r'_DRIVER: &str = r#"(.*?)"#;', source, re.S)
    if literal:
        return literal.group(1)
    assembly = re.search(r'const ' + language + r'_DRIVER: &str = concat!\((.*?)\n\);', source, re.S)
    assert assembly is not None, "worker compile-time assembly is missing"
    remaining = assembly.group(1).strip()
    fragments = []
    token = re.compile(r'\s*(?:r#"(.*?)"#|include_str!\("([^"]+)"\))\s*(?:,|$)', re.S)
    while remaining:
        match = token.match(remaining)
        assert match is not None, "unrecognized worker assembly expression"
        literal, included = match.groups()
        fragments.append(literal if literal is not None else
                         (source_file.parent / included).read_text(encoding="utf-8"))
        remaining = remaining[match.end():].strip()
    return "".join(fragments)


def load_helper(source_path=WORKERS[0]):
    worker = load_worker(source_path)
    tree = ast.parse(worker)
    allowed_functions = {"inspect_objects", "_inspect_name", "_inspect_metadata"}
    nodes = []
    for node in tree.body:
        if isinstance(node, ast.FunctionDef) and node.name in allowed_functions:
            nodes.append(node)
        elif isinstance(node, ast.Assign) and all(
                isinstance(target, ast.Name) and target.id.startswith("_INSPECT_")
                for target in node.targets):
            nodes.append(node)
    namespace = {"json": json}
    exec(compile(ast.Module(body=nodes, type_ignores=[]), "<metadata-only>", "exec"), namespace)
    assert "inspect_objects" in namespace, "worker inspection helper is missing"
    return namespace["inspect_objects"]


class KernelInspectionTests(unittest.TestCase):
    source_path = "crates/omicsops-runtime/src/process_backend.rs"

    def test_exact_builtins_and_opaque_methods(self):
        inspect = load_helper(self.source_path)
        calls = []

        class Hostile:
            def __repr__(self):
                calls.append("repr")
                raise AssertionError()

            def __str__(self):
                calls.append("str")
                raise AssertionError()

            def __len__(self):
                calls.append("len")
                raise AssertionError()

            @property
            def shape(self):
                calls.append("shape")
                raise AssertionError()

        class ListSubclass(list):
            def __len__(self):
                calls.append("subclass_len")
                raise AssertionError()

        class Meta(type):
            def __eq__(self, other):
                calls.append("type_eq")
                raise AssertionError()

        class Strange(metaclass=Meta):
            pass

        result = inspect({"items": [1, 2], "mapping": {1: 2}, "text": "SECRET_VALUE",
                          "scalar": 987654321, "hostile": Hostile(),
                          "subclass": ListSubclass(), "strange": Strange()})
        objects = {item["name"]: item for item in result["objects"]}
        self.assertEqual(objects["items"]["element_count"], 2)
        self.assertEqual(objects["mapping"]["element_count"], 1)
        self.assertEqual(objects["text"]["element_count"], 12)
        for name in ("hostile", "subclass", "strange"):
            self.assertEqual(objects[name]["type_label"], "opaque")
            self.assertIsNone(objects[name]["element_count"])
            self.assertIsNone(objects[name]["dimensions"])
        self.assertEqual(calls, [])
        self.assertNotIn("SECRET_VALUE", json.dumps(result))
        self.assertNotIn("987654321", json.dumps(result))

    def test_object_and_utf8_budgets(self):
        inspect = load_helper(self.source_path)
        result = inspect({"x" + str(i): [] for i in range(5000)})
        self.assertLessEqual(len(result["objects"]), 64)
        self.assertTrue(result["truncated"])
        result = inspect({"🧬" * 64: [], "🧬" * 65: [], "x" * 1000000: [],
                          "bad\nname": [], "surrogate\ud800": []})
        self.assertEqual([item["name"] for item in result["objects"]], ["🧬" * 64])
        self.assertTrue(result["truncated"])
        result = inspect({("界" * 80) + str(i): [] for i in range(64)})
        encoded = json.dumps(result, ensure_ascii=False).encode("utf-8")
        self.assertLessEqual(len(encoded), 12288)
        for item in result["objects"]:
            self.assertLessEqual(len(item["name"].encode("utf-8")), 256)
            self.assertLessEqual(len(item["summary"].encode("utf-8")), 128)
            self.assertLessEqual(len(item["type_label"].encode("utf-8")), 128)

    def test_secret_names_have_no_values(self):
        inspect = load_helper(self.source_path)
        sentinel = "sensitive_test_sentinel"
        result = inspect({"api_key": sentinel, "PASSWORD": sentinel,
                          "access_token": sentinel, "private_key": sentinel,
                          "client_secret": sentinel, "__internal": sentinel,
                          "normal": sentinel})
        self.assertEqual([item["name"] for item in result["objects"]], ["normal"])
        self.assertNotIn(sentinel, json.dumps(result))

    def test_binding_keys_and_binding_scan_do_not_call_user_code(self):
        inspect = load_helper(self.source_path)
        calls = []

        class Name(str):
            def __len__(self):
                calls.append("len")
                raise AssertionError()

            def encode(self, *args):
                calls.append("encode")
                raise AssertionError()

            def lower(self):
                calls.append("lower")
                raise AssertionError()

            def __lt__(self, other):
                calls.append("sort")
                raise AssertionError()

        scope = {Name("hostile"): [], 42: []}
        scope.update({"__hidden" + str(i): [] for i in range(4096)})
        scope["after_limit"] = []
        result = inspect(scope)
        self.assertEqual(result, {"objects": [], "truncated": True})
        self.assertEqual(calls, [])

        class Scope(dict):
            def items(self):
                calls.append("scope_items")
                raise AssertionError()

        self.assertEqual(inspect(Scope(normal=[])), {"objects": [], "truncated": True})
        self.assertEqual(calls, [])


class SshKernelInspectionTests(KernelInspectionTests):
    source_path = "crates/omicsops-adapters/src/kernel.rs"


class WorkerAssemblyTests(unittest.TestCase):
    def test_actual_python_workers_use_one_canonical_policy(self):
        canonical_path = ROOT / "scripts/kernel_inspection_helpers.py"
        self.assertTrue(canonical_path.is_file(), "canonical Python metadata policy is missing")
        canonical = canonical_path.read_text(encoding="utf-8")
        for node in ast.parse(canonical).body:
            self.assertTrue(isinstance(node, (ast.FunctionDef, ast.Assign)),
                            "canonical policy may contain only definitions and captures")
        for source_path in WORKERS:
            source = (ROOT / source_path).read_text(encoding="utf-8")
            self.assertIn('include_str!("../../../scripts/kernel_inspection_helpers.py")', source)
            self.assertNotIn("def inspect_objects(", source)
            self.assertNotIn("_INSPECT_TYPE =", source)
            worker = load_worker(source_path)
            self.assertEqual(worker.count(canonical), 1)
            self.assertEqual(worker.count("def inspect_objects("), 1)
            self.assertLess(worker.index("import contextlib"), worker.index("_INSPECT_TYPE ="))
            self.assertLess(worker.index("def inspect_objects("), worker.index("for raw in sys.stdin:"))
            ast.parse(worker)

    def test_actual_r_workers_use_one_canonical_policy(self):
        canonical_path = ROOT / "scripts/kernel_inspection_helpers.R"
        self.assertTrue(canonical_path.is_file(), "canonical R metadata policy is missing")
        canonical = canonical_path.read_text(encoding="utf-8")
        for source_path in WORKERS:
            source = (ROOT / source_path).read_text(encoding="utf-8")
            self.assertIn('include_str!("../../../scripts/kernel_inspection_helpers.R")', source)
            self.assertNotIn("inspect_bindings <- function", source)
            worker = load_worker(source_path, "R")
            self.assertEqual(worker.count(canonical), 1)
            self.assertEqual(worker.count("inspect_bindings <- function"), 1)
            self.assertLess(worker.index("library(jsonlite)"), worker.index("inspect_bindings <- function"))
            self.assertLess(worker.index("inspect_bindings <- function"), worker.index("repeat {"))


if __name__ == "__main__":
    unittest.main()
