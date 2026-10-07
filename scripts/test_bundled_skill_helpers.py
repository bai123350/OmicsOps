"""Offline contracts for the three fixed, bundled Wisp helper sources."""

import builtins
import copy
import io
import json
import os
from pathlib import Path
import socket
import sys
import time
import traceback
import types
import unittest
import urllib.error
import urllib.parse
import urllib.request
from contextlib import ExitStack, contextmanager, redirect_stdout
from unittest.mock import Mock, patch


_BUNDLE = Path(__file__).resolve().parents[1] / "skills" / "wisp-science"
_HELPERS = {
    "paper-narrative": _BUNDLE / "paper-narrative" / "runtime.py",
    "literature-review": _BUNDLE / "literature-review" / "runtime.py",
    "figure-style": _BUNDLE / "figure-style" / "runtime.py",
}
FAKE_ENV = {
    "OPENALEX_API_KEY": "FIXTURE_SECRET_OPENALEX",
    "WISP_LITERATURE_CONTACT_EMAIL": "fixture-contact@example.invalid",
}


@contextmanager
def offline_helper_call(modules=None):
    """Restore globals and fail closed on unmocked I/O or heavy imports."""
    modules = modules or {}
    real_import, real_open, real_io_open, real_os_open = (
        builtins.__import__, builtins.open, io.open, os.open
    )
    blocked_write = Mock(side_effect=AssertionError("helper attempted file write"))
    blocked_import = Mock(side_effect=AssertionError("helper imported optional library"))

    def guarded_import(name, *args, **kwargs):
        if name.split(".")[0] in {"matplotlib", "PIL", "numpy", "pandas", "scipy", "seaborn"}:
            if name not in modules:
                return blocked_import()
        return real_import(name, *args, **kwargs)

    def guarded_open(original, file, mode="r", *args, **kwargs):
        if any(flag in mode for flag in "wax+"):
            return blocked_write()
        return original(file, mode, *args, **kwargs)

    def guarded_os_open(file, flags, *args, **kwargs):
        if flags & (os.O_WRONLY | os.O_RDWR | os.O_CREAT | os.O_TRUNC | os.O_APPEND):
            return blocked_write()
        return real_os_open(file, flags, *args, **kwargs)

    output = io.StringIO()
    with ExitStack() as stack:
        stack.enter_context(patch.dict(os.environ, FAKE_ENV, clear=True))
        stack.enter_context(patch.dict(sys.modules, modules))
        stack.enter_context(redirect_stdout(output))
        stack.enter_context(patch("builtins.__import__", side_effect=guarded_import))
        stack.enter_context(patch("builtins.open", side_effect=lambda *a, **k: guarded_open(real_open, *a, **k)))
        stack.enter_context(patch("io.open", side_effect=lambda *a, **k: guarded_open(real_io_open, *a, **k)))
        stack.enter_context(patch("os.open", side_effect=guarded_os_open))
        blocked_io = [stack.enter_context(patch.object(owner, name, side_effect=AssertionError("helper attempted external I/O")))
                      for owner, name in (
                          (urllib.request, "urlopen"), (urllib.request.OpenerDirector, "open"),
                          (urllib.request.URLopener, "open"), (socket, "create_connection"),
                          (socket.socket, "connect"), (socket.socket, "connect_ex"), (time, "sleep"),
                      )]
        yield
        blocked_write.assert_not_called()
        blocked_import.assert_not_called()
        for blocked in blocked_io:
            blocked.assert_not_called()
        assert output.getvalue() == "", "helper emitted stdout"


def load_bundled_helper(skill_name: str) -> dict[str, object]:
    path = _HELPERS[skill_name]
    # Read the fixed bundled source before installing write/I/O guards.
    source = path.read_text(encoding="utf-8")
    namespace = {"__name__": "__main__", "__file__": str(path)}
    with offline_helper_call():
        exec(compile(source, str(path), "exec"), namespace)
    return namespace


class FakeHttpResponse:
    def __init__(self, payload: bytes, status: int = 200):
        self.payload, self.status = payload, status

    def read(self):
        return self.payload

    def __enter__(self):
        return self

    def __exit__(self, exc_type, exc_value, exc_traceback):
        return False


class FakeBbox:
    def __init__(self, x0, y0, x1, y1):
        self.x0, self.y0, self.x1, self.y1 = x0, y0, x1, y1
        self.width, self.height = x1 - x0, y1 - y0

    @staticmethod
    def union(boxes):
        return FakeBbox(min(b.x0 for b in boxes), min(b.y0 for b in boxes),
                        max(b.x1 for b in boxes), max(b.y1 for b in boxes))

    def padded(self, pad):
        return FakeBbox(self.x0 - pad, self.y0 - pad, self.x1 + pad, self.y1 + pad)


class FakeAxis:
    def __init__(self, box, slot, label, shared_axis):
        self.box, self.slot, self.label, self.shared_axis = box, slot, label, shared_axis

    def findobj(self, text_type):
        return [types.SimpleNamespace(get_text=lambda: self.label, get_fontweight=lambda: "bold")] if self.label else []

    def get_tightbbox(self, renderer):
        return self.box

    def get_subplotspec(self):
        return self.slot

    def get_shared_y_axes(self):
        return types.SimpleNamespace(joined=lambda ax, sibling: self.shared_axis == "y")

    def get_shared_x_axes(self):
        return types.SimpleNamespace(joined=lambda ax, sibling: self.shared_axis == "x")


def make_panel_fixture(shared_axis: str, same_slot: bool) -> tuple[types.SimpleNamespace, dict[str, object]]:
    """Two 100-dpi axes in a 200x100 frame; only the first is lettered."""
    first_slot = types.SimpleNamespace(rowspan=range(0, 1), colspan=range(0, 1))
    sibling_slot = types.SimpleNamespace(
        rowspan=range(0, 1) if shared_axis == "y" and same_slot else range(1, 2),
        colspan=range(0, 1) if shared_axis == "x" and same_slot else range(1, 2),
    )
    axes = [FakeAxis(FakeBbox(10, 20, 60, 80), first_slot, "a", shared_axis),
            FakeAxis(FakeBbox(80, 10, 220, 95), sibling_slot, "", shared_axis)]
    mpl = types.ModuleType("matplotlib")
    mpl.rcParams = {"savefig.dpi": "figure", "savefig.bbox": None}
    mpl.use = Mock(side_effect=AssertionError("backend must not change"))
    mpl.get_backend = lambda: "fixture-backend"
    transforms = types.ModuleType("matplotlib.transforms")
    transforms.Bbox = transforms.BboxBase = FakeBbox
    text = types.ModuleType("matplotlib.text")
    text.Text = object
    mpl.transforms, mpl.text = transforms, text
    renderer = object()
    fig = types.SimpleNamespace(
        axes=axes, dpi=100, get_size_inches=lambda: (2, 1),
        get_tightbbox=lambda r: FakeBbox(0.1, 0.1, 1.9, 0.9),
        canvas=types.SimpleNamespace(draw=lambda: None, get_renderer=lambda: renderer),
    )
    return fig, {"matplotlib": mpl, "matplotlib.transforms": transforms, "matplotlib.text": text}


class BundledHelperLoadTests(unittest.TestCase):
    def test_loads_fresh_main_namespaces(self):
        first = load_bundled_helper("paper-narrative")
        second = load_bundled_helper("paper-narrative")
        self.assertEqual(first["__name__"], "__main__")
        self.assertIsNot(first, second)
        self.assertIsNot(first["paper_brief_task"], second["paper_brief_task"])

    def test_all_fixed_helpers_load_without_optional_libraries_or_side_effects(self):
        expected = {"paper-narrative": "paper_brief_task", "literature-review": "search_openalex",
                    "figure-style": "panel_crops"}
        with patch.dict(sys.modules, {name: None for name in ("matplotlib", "PIL", "numpy")}):
            for name, function in expected.items():
                with self.subTest(skill=name):
                    self.assertTrue(callable(load_bundled_helper(name)[function]))

    def test_rejects_names_outside_fixed_bundle(self):
        for name in ("../paper-narrative", "user-installed", "figure-style/runtime.py"):
            with self.subTest(skill=name), self.assertRaises(KeyError):
                load_bundled_helper(name)

    def test_restores_environment_modules_and_mocks(self):
        environment, modules = dict(os.environ), dict(sys.modules)
        original_open, original_urlopen = builtins.open, urllib.request.urlopen
        original_sleep, original_import = time.sleep, builtins.__import__
        load_bundled_helper("literature-review")
        self.assertTrue(dict(os.environ) == environment, "environment was not restored")
        self.assertTrue(dict(sys.modules) == modules, "modules were not restored")
        self.assertIs(builtins.open, original_open)
        self.assertIs(urllib.request.urlopen, original_urlopen)
        self.assertIs(time.sleep, original_sleep)
        self.assertIs(builtins.__import__, original_import)

    def test_missing_or_invalid_fixed_source_fails_without_skip(self):
        with patch.object(Path, "read_text", side_effect=FileNotFoundError("missing fixture source")):
            with self.assertRaises(FileNotFoundError):
                load_bundled_helper("paper-narrative")
        with patch.object(Path, "read_text", return_value="def broken(:"):
            with self.assertRaises(SyntaxError):
                load_bundled_helper("paper-narrative")

    def test_explicit_calls_see_only_fake_credentials_and_contact(self):
        namespace = load_bundled_helper("literature-review")
        with offline_helper_call():
            self.assertEqual(dict(os.environ), FAKE_ENV)
            self.assertEqual(namespace["_contact_email"](), "fixture-contact@example.invalid")
            self.assertEqual(namespace["_openalex_key_param"](), "&api_key=FIXTURE_SECRET_OPENALEX")

    def test_loading_leaves_existing_matplotlib_state_unchanged(self):
        _, modules = make_panel_fixture("y", True)
        mpl = modules["matplotlib"]
        original = copy.deepcopy(mpl.rcParams)
        with patch.dict(sys.modules, modules):
            load_bundled_helper("figure-style")
            self.assertEqual(mpl.rcParams, original)
            self.assertEqual(mpl.get_backend(), "fixture-backend")
            mpl.use.assert_not_called()


class PaperNarrativeTests(unittest.TestCase):
    def setUp(self):
        self.namespace = load_bundled_helper("paper-narrative")
        self.figures = [
            {"key": "a", "claim": '中文 "引号"', "composite_path": r'C:\研究\my "figure".png',
             "metadata": {"labels": ["原始"]}},
            {"key": "b", "claim": "second claim", "composite_path": None},
            {"key": "c", "caption": "caption fallback"},
        ]
        self.original = copy.deepcopy(self.figures)
        self.expected = [
            {"key": "a", "claim": '中文 "引号"', "composite_path": r'C:\研究\my "figure".png'},
            {"key": "b", "claim": "second claim", "composite_path": None},
            {"key": "c", "claim": "caption fallback"},
        ]

    def assert_claim_contract(self, namespace):
        rows = json.loads(namespace["_paper_figure_claims_json"](self.figures))
        self.assertEqual(rows, self.expected)
        self.assertIsNone(rows[1]["composite_path"])
        self.assertNotIn("composite_path", rows[2])
        self.assertEqual(self.figures, self.original)

    def test_json_preserves_claim_path_binding_without_mutating_input(self):
        self.assert_claim_contract(self.namespace)

    def test_brief_embeds_round_trippable_claims_and_abstract(self):
        abstract = '摘要 "sample"'
        task = self.namespace["paper_brief_task"](abstract, self.figures)
        self.assertIn(abstract, task)
        rows = json.loads(task.split("```json\n", 1)[1].split("\n```", 1)[0])
        self.assertEqual(rows, self.expected)
        self.assertEqual(self.figures, self.original)

    def test_claim_contract_detects_namespace_local_wrong_path(self):
        wrong_rows = copy.deepcopy(self.expected)
        wrong_rows[0]["composite_path"] = "wrong-fixture-path.png"
        self.namespace["_paper_figure_claims_json"] = lambda figures: json.dumps(wrong_rows)
        with self.assertRaises(AssertionError):
            self.assert_claim_contract(self.namespace)


class LiteratureReviewTests(unittest.TestCase):
    def setUp(self):
        self.namespace = load_bundled_helper("literature-review")

    def assert_safe_failure(self, call, http, reason=None):
        try:
            call()
        except RuntimeError as error:
            rendered = "".join(traceback.format_exception(type(error), error, error.__traceback__))
            request_url = http.call_args.args[0].full_url
            for private in ("FIXTURE_SECRET", FAKE_ENV["WISP_LITERATURE_CONTACT_EMAIL"],
                            request_url, "FIXTURE_ORIGINAL_EXCEPTION"):
                self.assertNotIn(private, rendered)
            if reason is not None:
                self.assertIn(reason, str(error))
        else:
            self.fail("failed retrieval must raise RuntimeError rather than return results")

    def test_search_success_empty_and_populated_maps_selected_rows(self):
        work = {"doi": "https://doi.org/10.1234/fixture", "title": "科学 fixture",
                "publication_year": 2024, "cited_by_count": 7,
                "primary_location": {"source": {"display_name": "Fixture Journal"}},
                "open_access": {"oa_url": "https://example.invalid/paper"}}
        with offline_helper_call(), patch("urllib.request.urlopen", side_effect=[
                FakeHttpResponse(b'{"results": []}'),
                FakeHttpResponse(json.dumps({"results": [work, {}]}).encode("utf-8")),
        ]) as http:
            self.assertEqual(self.namespace["search_openalex"]("zero hits"), [])
            self.assertEqual(self.namespace["search_openalex"]("science 中文", n=1, filters="type:article"), [
                {"doi": "10.1234/fixture", "title": "科学 fixture", "year": 2024, "cited_by": 7,
                 "venue": "Fixture Journal", "oa_url": "https://example.invalid/paper"},
            ])
            request = http.call_args.args[0]
            self.assertEqual(urllib.parse.urlsplit(request.full_url).netloc, "api.openalex.org")
            params = urllib.parse.parse_qs(urllib.parse.urlsplit(request.full_url).query)
            self.assertEqual(params["search"], ["science 中文"])
            self.assertEqual(params["per-page"], ["1"])
            self.assertEqual(params["filter"], ["type:article"])
            self.assertEqual(params["sort"], ["cited_by_count:desc"])
            self.assertEqual(params["api_key"], [FAKE_ENV["OPENALEX_API_KEY"]])
            self.assertEqual(params["mailto"], [FAKE_ENV["WISP_LITERATURE_CONTACT_EMAIL"]])
            self.assertEqual(http.call_args.kwargs, {"timeout": 15})

    def test_search_http_failures_are_safe_errors_not_empty_results(self):
        url = "https://api.openalex.org/works?api_key=FIXTURE_SECRET_OPENALEX"
        for status in (401, 404, 500):
            with self.subTest(status=status), offline_helper_call(), \
                    patch("urllib.request.urlopen", side_effect=urllib.error.HTTPError(
                        url, status, "FIXTURE_ORIGINAL_EXCEPTION", {}, None)) as http, \
                    patch("time.sleep") as sleep:
                self.assert_safe_failure(lambda: self.namespace["search_openalex"]("x"), http, f"HTTP {status}")
                self.assertEqual(http.call_count, 1)
                sleep.assert_not_called()

    def test_search_connection_and_decoding_failures_hide_full_error_chain(self):
        url = "https://api.openalex.org/works?api_key=FIXTURE_SECRET_OPENALEX"
        failures = (
            ("timeout", TimeoutError("FIXTURE_ORIGINAL_EXCEPTION " + url), "connection failed or timed out"),
            ("url", urllib.error.URLError("FIXTURE_ORIGINAL_EXCEPTION " + url), "connection failed or timed out"),
            ("utf8", FakeHttpResponse(b"\xff"), "invalid JSON response"),
            ("json", FakeHttpResponse(b"FIXTURE_ORIGINAL_EXCEPTION"), "invalid JSON response"),
        )
        for label, failure, reason in failures:
            with self.subTest(failure=label), offline_helper_call(), \
                    patch("urllib.request.urlopen", side_effect=[failure]) as http, patch("time.sleep") as sleep:
                self.assert_safe_failure(lambda: self.namespace["search_openalex"]("x"), http, reason)
                self.assertEqual(http.call_count, 1)
                sleep.assert_not_called()

    def test_search_rejects_invalid_results_and_work_records(self):
        for payload in (None, [], {}, {"results": None}, {"results": {}}, {"results": [None]},
                        {"results": ["FIXTURE_SECRET_OPENALEX"]}):
            with self.subTest(payload_type=type(payload).__name__), offline_helper_call(), \
                    patch("urllib.request.urlopen", return_value=FakeHttpResponse(json.dumps(payload).encode())) as http:
                self.assert_safe_failure(lambda: self.namespace["search_openalex"]("x"), http)

    def test_429_retries_exactly_once_and_sleeps_once(self):
        url = "https://api.openalex.org/works?api_key=FIXTURE_SECRET_OPENALEX"
        for recovered in (True, False):
            error = urllib.error.HTTPError(url, 429, "FIXTURE_ORIGINAL_EXCEPTION", {}, None)
            second = FakeHttpResponse(b'{"results": []}') if recovered else error
            with self.subTest(recovered=recovered), offline_helper_call(), \
                    patch("urllib.request.urlopen", side_effect=[error, second]) as http, patch("time.sleep") as sleep:
                if recovered:
                    self.assertEqual(self.namespace["search_openalex"]("x"), [])
                else:
                    self.assert_safe_failure(lambda: self.namespace["search_openalex"]("x"), http, "HTTP 429")
                self.assertEqual(http.call_count, 2)
                self.assertEqual(http.call_args_list[0].args[0].full_url, http.call_args_list[1].args[0].full_url)
                sleep.assert_called_once_with(2)

    def test_get_json_tolerant_failure_remains_unknown(self):
        with offline_helper_call(), patch("urllib.request.urlopen", side_effect=urllib.error.URLError(
                "FIXTURE_ORIGINAL_EXCEPTION")) as http:
            self.assertIsNone(self.namespace["_get_json"]("https://example.invalid/fixture", timeout=3))
            self.assertEqual(http.call_args.kwargs, {"timeout": 3})

    def test_citation_expansion_preserves_backward_and_forward_directions(self):
        responses = [FakeHttpResponse(b'{"id": "https://openalex.org/W123"}'),
                     FakeHttpResponse(b'{"results": [{"doi": "https://doi.org/10.1234/back", "title": "Reference", "publication_year": 2020, "cited_by_count": 5}]}'),
                     FakeHttpResponse(b'{"results": [{"doi": "https://doi.org/10.1234/forward", "title": "Citing work", "publication_year": 2025, "cited_by_count": 2}]}')]
        with offline_helper_call(), patch("urllib.request.urlopen", side_effect=responses) as http:
            self.assertEqual(self.namespace["expand_citations"]("10.1234/source", n_backward=120, n_forward=4), {
                "references": [{"doi": "10.1234/back", "title": "Reference", "year": 2020, "cited_by": 5}],
                "cited_by": [{"doi": "10.1234/forward", "title": "Citing work", "year": 2025, "cited_by": 2}],
            })
            requests = [call.args[0] for call in http.call_args_list]
            self.assertEqual(urllib.parse.urlsplit(requests[0].full_url).path, "/works/doi:10.1234/source")
            for request, direction, limit in zip(requests[1:], ("cited_by:W123", "cites:W123"), ("100", "4")):
                params = urllib.parse.parse_qs(urllib.parse.urlsplit(request.full_url).query)
                self.assertEqual(params["filter"], [direction])
                self.assertEqual(params["per-page"], [limit])
            for request in requests:
                params = urllib.parse.parse_qs(urllib.parse.urlsplit(request.full_url).query)
                self.assertEqual(params["api_key"], [FAKE_ENV["OPENALEX_API_KEY"]])

    def test_citation_expansion_empty_requires_two_successful_directions(self):
        with offline_helper_call(), patch("urllib.request.urlopen", side_effect=[
                FakeHttpResponse(b'{"id": "https://openalex.org/W123"}'),
                FakeHttpResponse(b'{"results": []}'), FakeHttpResponse(b'{"results": []}'),
        ]) as http:
            self.assertEqual(self.namespace["expand_citations"]("10.1234/source"), {"references": [], "cited_by": []})
            self.assertEqual(http.call_count, 3)

    def test_citation_expansion_failure_never_returns_partial_graph(self):
        url = "https://api.openalex.org/works?api_key=FIXTURE_SECRET_OPENALEX"
        for failed_request in (0, 1, 2):
            responses = [FakeHttpResponse(b'{"id": "https://openalex.org/W123"}'),
                         FakeHttpResponse(b'{"results": [{"title": "Existing reference"}]}'),
                         FakeHttpResponse(b'{"results": []}')]
            responses[failed_request] = urllib.error.HTTPError(url, 500, "FIXTURE_ORIGINAL_EXCEPTION", {}, None)
            with self.subTest(failed_request=failed_request), offline_helper_call(), \
                    patch("urllib.request.urlopen", side_effect=responses) as http:
                self.assert_safe_failure(lambda: self.namespace["expand_citations"]("10.1234/source"), http, "HTTP 500")
                self.assertEqual(http.call_count, failed_request + 1)

    def test_doi_crossref_miss_and_unavailable_head_remain_unknown(self):
        doi = "10.1234/unknown"
        for head_failure in (TimeoutError("fixture timeout"), urllib.error.URLError("fixture unavailable"),
                             urllib.error.HTTPError("https://doi.org/fixture", 500, "fixture failure", {}, None)):
            opener = types.SimpleNamespace(open=Mock(side_effect=head_failure))
            with self.subTest(failure=type(head_failure).__name__), offline_helper_call(), \
                    patch("urllib.request.urlopen", side_effect=urllib.error.HTTPError(
                        "https://api.crossref.org/works/fixture", 404, "fixture miss", {}, None)) as http, \
                    patch("urllib.request.build_opener", return_value=opener), patch("time.sleep") as sleep:
                verified = self.namespace["verify_dois"]([doi])
                self.assertEqual(verified, {doi: {"ok": None, "error": "unverified (network)", "retracted": None}})
                self.assertEqual(http.call_count, 1)
                self.assertEqual(opener.open.call_args.args[0].get_method(), "HEAD")
                sleep.assert_called_once_with(0.06)

    def test_doi_registry_status_and_credentials_stay_service_specific(self):
        doi = "10.1234/status"
        for status, expected in ((200, True), (204, True), (302, True), (399, True), (404, False)):
            head = (urllib.error.HTTPError("https://doi.org/fixture", status, "fixture status", {}, None)
                    if status >= 300 else FakeHttpResponse(b"", status))
            opener = types.SimpleNamespace(open=Mock(side_effect=[head]))
            with self.subTest(status=status), offline_helper_call(), \
                    patch("urllib.request.urlopen", return_value=FakeHttpResponse(b"{}")) as http, \
                    patch("urllib.request.build_opener", return_value=opener), patch("time.sleep") as sleep:
                result = self.namespace["verify_dois"]([doi])[doi]
                self.assertIs(result["ok"], expected)
                if expected:
                    self.assertEqual(result, {"ok": True, "registry": "non-crossref", "retracted": None})
                for request, host in ((http.call_args.args[0], "api.crossref.org"),
                                      (opener.open.call_args.args[0], "doi.org")):
                    self.assertEqual(urllib.parse.urlsplit(request.full_url).netloc, host)
                    self.assertNotIn(FAKE_ENV["OPENALEX_API_KEY"], request.full_url)
                    self.assertNotIn(FAKE_ENV["OPENALEX_API_KEY"], str(request.headers))
                    self.assertNotIn("api_key", request.full_url)
                self.assertEqual(opener.open.call_args.kwargs, {"timeout": 10})
                sleep.assert_called_once_with(0.06)

    def test_contract_rejects_namespace_local_swallowed_http_error_and_restores(self):
        original = self.namespace["search_openalex"]

        def swallowed_error(query):
            try:
                return original(query)
            except RuntimeError:
                return []

        try:
            self.namespace["search_openalex"] = swallowed_error
            with offline_helper_call(), patch("urllib.request.urlopen", side_effect=urllib.error.HTTPError(
                    "https://api.openalex.org/fixture", 500, "fixture failure", {}, None)) as http:
                with self.assertRaises(AssertionError):
                    self.assert_safe_failure(lambda: self.namespace["search_openalex"]("x"), http)
        finally:
            self.namespace["search_openalex"] = original
        self.assertIs(self.namespace["search_openalex"], original)
        with offline_helper_call(), patch("urllib.request.urlopen", return_value=FakeHttpResponse(b'{"results": []}')):
            self.assertEqual(self.namespace["search_openalex"]("x"), [])

    def test_http_fixtures_restore_globals_after_success_and_failure(self):
        environment, modules = dict(os.environ), dict(sys.modules)
        original_urlopen, original_build_opener = urllib.request.urlopen, urllib.request.build_opener
        original_opener_open, original_sleep = urllib.request.OpenerDirector.open, time.sleep
        for failure in (False, True):
            response = (urllib.error.URLError("fixture unavailable") if failure
                        else FakeHttpResponse(b'{"results": []}'))
            with self.subTest(failure=failure), offline_helper_call(), \
                    patch("urllib.request.urlopen", side_effect=[response]) as http, \
                    patch("urllib.request.build_opener"), patch("time.sleep"):
                if failure:
                    self.assert_safe_failure(lambda: self.namespace["search_openalex"]("x"), http)
                else:
                    self.assertEqual(self.namespace["search_openalex"]("x"), [])
            self.assertTrue(dict(os.environ) == environment, "environment was not restored")
            self.assertTrue(dict(sys.modules) == modules, "modules were not restored")
            self.assertIs(urllib.request.urlopen, original_urlopen)
            self.assertIs(urllib.request.build_opener, original_build_opener)
            self.assertIs(urllib.request.OpenerDirector.open, original_opener_open)
            self.assertIs(time.sleep, original_sleep)


class FigureStyleTests(unittest.TestCase):
    def setUp(self):
        self.namespace = load_bundled_helper("figure-style")

    def test_self_check_applies_style_only_when_called(self):
        _, modules = make_panel_fixture("y", True)
        mpl = modules["matplotlib"]
        fonts = types.ModuleType("matplotlib.font_manager")
        fonts.fontManager = types.SimpleNamespace(
            ttflist=[types.SimpleNamespace(name="Microsoft YaHei", fname="fixture-font")],
            addfont=Mock(side_effect=AssertionError("no real font registration")),
        )
        modules["matplotlib.font_manager"] = mpl.font_manager = fonts
        self.assertNotIn("font.sans-serif", mpl.rcParams)
        with offline_helper_call(modules), patch("platform.system", return_value="Windows"), \
                patch("os.path.isfile", return_value=False), patch("os.path.isdir", return_value=False):
            result = self.namespace["figure_style_self_check"]()
        self.assertEqual(result, {"font.sans-serif": ["Microsoft YaHei", "DejaVu Sans", "Liberation Sans", "Arial"],
                                  "axes.unicode_minus": False})
        self.assertEqual(mpl.rcParams["savefig.dpi"], 300)
        self.assertEqual(mpl.rcParams["savefig.bbox"], "tight")
        self.assertFalse(mpl.rcParams["axes.spines.top"])
        mpl.use.assert_not_called()

    def test_shared_axes_merge_only_in_correct_row_or_column_and_clamp(self):
        for shared_axis in ("y", "x"):
            for same_slot, expected in ((True, (4, 0, 200, 96)), (False, (4, 14, 66, 86))):
                with self.subTest(shared_axis=shared_axis, same_slot=same_slot):
                    fig, modules = make_panel_fixture(shared_axis, same_slot)
                    with offline_helper_call(modules):
                        crops = self.namespace["panel_crops"](fig)
                    self.assertEqual(crops, {"a": expected})

    def test_top_origin_scaling_and_all_image_boundaries(self):
        fig, modules = make_panel_fixture("none", False)
        fig.axes = fig.axes[:1]
        with offline_helper_call(modules):
            self.assertEqual(self.namespace["panel_crops"](fig, dpi=200, pad_px=0), {"a": (20, 40, 120, 160)})
            fig.axes[0].box = FakeBbox(-20, -10, 220, 120)
            self.assertEqual(self.namespace["panel_crops"](fig), {"a": (0, 0, 200, 100)})

    def test_explicit_saved_frame_and_tight_padding(self):
        fig, modules = make_panel_fixture("none", False)
        fig.axes = fig.axes[:1]
        with offline_helper_call(modules):
            explicit = FakeBbox(0.1, 0.1, 1.1, 0.9)
            self.assertEqual(self.namespace["panel_crops"](fig, bbox_inches=explicit, pad_px=0),
                             {"a": (0, 10, 50, 70)})
            self.assertEqual(self.namespace["panel_crops"](fig, bbox_inches="tight", pad_inches=0.1, pad_px=0),
                             {"a": (10, 20, 60, 80)})


class HelperDiscoveryTests(unittest.TestCase):
    def test_all_three_helper_behavior_groups_are_discoverable_and_nonempty(self):
        self.assertTrue(Path(__file__).is_file())
        loader = unittest.TestLoader()
        for skill_name, class_name in (
                ("paper-narrative", "PaperNarrativeTests"),
                ("literature-review", "LiteratureReviewTests"),
                ("figure-style", "FigureStyleTests")):
            with self.subTest(skill=skill_name):
                self.assertTrue(_HELPERS[skill_name].is_file())
                discovered = loader.loadTestsFromName(class_name, sys.modules[__name__])
                self.assertEqual(loader.errors, [])
                self.assertGreater(discovered.countTestCases(), 0)
                for case in discovered:
                    self.assertEqual(type(case).__name__, class_name)

    def test_independent_failure_is_recorded_as_unsuccessful(self):
        # Local to this test: normal discovery must never run this failing case.
        class DeliberatelyFailingCase(unittest.TestCase):
            def test_failure(self):
                self.assertEqual(1, 2, "controlled unittest failure")

        result = unittest.TestResult()
        unittest.TestLoader().loadTestsFromTestCase(DeliberatelyFailingCase).run(result)
        self.assertEqual(result.testsRun, 1)
        self.assertFalse(result.wasSuccessful())
        self.assertEqual(len(result.failures), 1)
        self.assertEqual(result.errors, [])
        self.assertEqual(result.skipped, [])


if __name__ == "__main__":
    unittest.main()
