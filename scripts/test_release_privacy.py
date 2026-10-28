"""Privacy gate contracts use synthetic secrets and temporary resource trees only."""

import importlib.util
import json
from pathlib import Path
import subprocess
import sys
import tempfile
import unittest
from unittest import mock


SCRIPT = Path(__file__).with_name("check_release_privacy.py")
SPEC = importlib.util.spec_from_file_location("release_privacy", SCRIPT)
privacy = importlib.util.module_from_spec(SPEC)
SPEC.loader.exec_module(privacy)


class PrivacyTests(unittest.TestCase):
    def setUp(self):
        self.temp = tempfile.TemporaryDirectory()
        self.addCleanup(self.temp.cleanup)
        # Windows short names and macOS /var aliases may differ from resolve().
        self.root = Path(self.temp.name).resolve()
        self.resource = self.root / "skills/single-cell/example/SKILL.md"
        self.resource.parent.mkdir(parents=True)
        self.resource.write_text("# Generic skill\n", encoding="utf-8")
        self.config = self.root / "src-tauri/tauri.conf.json"
        self.config.parent.mkdir()
        self.write_config({"../skills/single-cell/": "skills/single-cell/"})

    def write_config(self, resources):
        self.config.write_text(json.dumps({
            "build": {"frontendDist": "../dist"},
            "bundle": {"resources": resources},
        }), encoding="utf-8")

    def audit(self, **kwargs):
        return privacy.audit_repo(self.root, tracked_files={
            "skills/single-cell/example/SKILL.md",
        }, forbidden_prefixes=[], **kwargs)

    def rules(self, findings):
        return {rule for _, rule in findings}

    def test_clean_resource_passes(self):
        self.assertEqual(self.audit(), [])

    def test_release_notes_document_is_scanned_without_echoing_credentials(self):
        subprocess.run(["git", "init", "-q", str(self.root)], check=True, capture_output=True)
        subprocess.run(["git", "-C", str(self.root), "add", "skills"], check=True, capture_output=True)
        notes = self.root / "release-notes.md"
        secret = "sk-proj-" + "K7" * 30
        for content, expected in [("# Release preview", 0), (secret, 1)]:
            notes.write_text(content, encoding="utf-8")
            result = subprocess.run(
                [sys.executable, "-B", str(SCRIPT), "--root", str(self.root), "--document", str(notes)],
                capture_output=True, text=True,
            )
            self.assertEqual(result.returncode, expected, result.stderr)
            self.assertNotIn(secret, result.stdout + result.stderr)
            if expected:
                self.assertIn("credential-token", result.stderr)

    def test_extra_resource_mapping_and_glob_rejected(self):
        for source in ("../", "../private.env", "../skills/**"):
            with self.subTest(source=source):
                self.write_config({source: "data/"})
                self.assertIn("resource-mapping", self.rules(self.audit()))

    def test_redirected_destination_rejected(self):
        self.write_config({"../skills/single-cell/": "user-data/"})
        self.assertIn("resource-mapping", self.rules(self.audit()))

    def test_platform_override_cannot_add_personal_files_or_external_binary(self):
        platform = self.config.with_name("tauri.windows.conf.json")
        for bundle in ({"resources": ["../personal"]}, {"externalBin": ["../tool"]}):
            platform.write_text(json.dumps({"bundle": bundle}), encoding="utf-8")
            self.assertTrue(self.audit())

    def test_frontend_cannot_point_outside_dist(self):
        self.config.write_text(json.dumps({"build": {"frontendDist": "../../personal"}}), encoding="utf-8")
        self.assertIn("frontend-directory", self.rules(self.audit()))

    def test_alternate_config_and_platform_file_maps_are_rejected(self):
        alternate = self.config.with_name("tauri.conf.json5")
        alternate.write_text("{}", encoding="utf-8")
        self.assertIn("alternate-configuration", self.rules(self.audit()))
        alternate.unlink()
        platform = self.config.with_name("tauri.linux.conf.json")
        platform.write_text(json.dumps({"bundle": {"linux": {"deb": {"files": {"/private": "../personal"}}}}}), encoding="utf-8")
        self.assertIn("additional-bundle-input", self.rules(self.audit()))

    def test_bundle_hook_cannot_add_inputs_after_the_scan(self):
        self.config.write_text(json.dumps({"build": {"beforeBundleCommand": "copy-private-config"}}), encoding="utf-8")
        self.assertIn("additional-build-input", self.rules(self.audit()))

    def test_all_additional_bundle_inputs_require_review(self):
        examples = [
            {"licenseFile": "../private.txt"},
            {"linux": {"deb": {"postInstallScript": "../personal.sh"}}},
            {"linux": {"rpm": {"desktopTemplate": "../personal.desktop"}}},
            {"windows": {"nsis": {"uninstallerIcon": "../private.ico"}}},
            {"windows": {"wix": {"language": {"en-US": {"localePath": "../private.xml"}}}}},
            {"macOS": {"dmg": {"background": "../photo.png"}}},
            {"unknownFutureBundlerInput": "../private.txt"},
        ]
        platform = self.config.with_name("tauri.windows.conf.json")
        for bundle in examples:
            with self.subTest(field=next(iter(bundle))):
                platform.write_text(json.dumps({"bundle": bundle}), encoding="utf-8")
                self.assertIn("additional-bundle-input", self.rules(self.audit()))

    def test_implicit_info_plist_is_rejected(self):
        self.config.with_name("Info.plist").write_text("<plist/>", encoding="utf-8")
        self.assertIn("implicit-bundle-input", self.rules(self.audit()))

    def test_config_metadata_is_scanned(self):
        config = json.loads(self.config.read_text(encoding="utf-8"))
        config["productName"] = "sk-proj-" + "A9" * 30
        self.config.write_text(json.dumps(config), encoding="utf-8")
        self.assertIn("credential-token", self.rules(self.audit()))

    def test_untracked_resource_is_rejected_even_if_harmless(self):
        (self.resource.parent / "private-notes.md").write_text("notes", encoding="utf-8")
        self.assertIn("untracked-resource", self.rules(self.audit()))

    def test_secret_and_generated_file_names_rejected(self):
        for name in (".env", ".env.production", "local.db", "id_rsa", "local.pem", "client.pfx", "cache.pyc", "run.log", "symbols.pdb"):
            with self.subTest(name=name):
                file = self.resource.parent / name
                file.write_text("placeholder", encoding="utf-8")
                self.assertIn("private-resource-file", self.rules(self.audit()))
                file.unlink()

    def test_resource_link_rejected_without_reading_target(self):
        real_check = privacy.is_link
        with mock.patch.object(privacy, "is_link", side_effect=lambda path: path == self.resource or real_check(path)):
            self.assertIn("resource-link", self.rules(self.audit()))

    def test_provider_tokens_are_detected_without_reporting_value(self):
        tokens = ["sk-proj-" + "q7" * 24, "ghp_" + "aB8" * 12, "AIza" + "c9" * 18]
        for token in tokens:
            with self.subTest(kind=token[:4]):
                findings = privacy.scan_bytes(token.encode(), [])
                self.assertIn("credential-token", findings)
                self.assertNotIn(token, str(findings))

    def test_complete_private_key_detected_but_library_header_is_allowed(self):
        header = "-----BEGIN OPENSSH PRIVATE KEY-----"
        self.assertEqual(privacy.scan_bytes(header.encode(), []), set())
        body = header + "\n" + "abcd1234" * 16 + "\n-----END OPENSSH PRIVATE KEY-----"
        self.assertIn("private-key", privacy.scan_bytes(body.encode(), []))
        self.assertIn("private-key", privacy.scan_bytes(json.dumps({"private_key": body}).encode(), []))

    def test_literal_credentials_and_authenticated_url_detected(self):
        for text in ('{"api_key":"not-a-real-key-123"}', 'password = "hunter-fixture"', 'token = "opaque-fixture-token-123"', 'https://researcher:fixture-pass@service.invalid/api'):
            with self.subTest(text=text.split(":")[0]):
                self.assertTrue(privacy.scan_bytes(text.encode(), []))

    def test_authenticated_urls_preserve_unicode_percent_colon_and_empty_user(self):
        values = [
            "https://fixture-user:fixture-pass@service.invalid/api",
            "ssh://fixture%3Auser:fixture%40pass@service.invalid",
            "postgresql://fixture-user:part:two'!$&()*+,;=@service.invalid/db",
            "mysql://fixture-user:合成口令🙂@service.invalid/db",
            "redis://:fixture-pass@service.invalid",
            "http://fixture-user:fixture-pass@",
            "PoStGrEs://fixture-user:fixture-pass@service.invalid/db",
        ]
        for value in values:
            for encoding in ("utf-8", "utf-16le", "utf-16be"):
                with self.subTest(index=values.index(value), encoding=encoding):
                    self.assertIn("authenticated-url", privacy.scan_bytes(value.encode(encoding), []))

    def test_authenticated_urls_do_not_join_binary_strings_or_cross_url_boundaries(self):
        for separator in (b"\x00", b"\x01", b"\x1f", b"\x7f", b"?", b"#", b'"', b"\\"):
            value = b"https://fixture-user:" + separator + b"fixture-pass@service.invalid"
            with self.subTest(separator=separator.hex()):
                self.assertNotIn("authenticated-url", privacy.scan_bytes(value, []))
        self.assertNotIn("authenticated-url", privacy.scan_bytes(
            b"https://service.invalid?query:piece@tail.invalid", []))
        for encoding in ("utf-16le", "utf-16be"):
            for boundary in ("\x00", "\x01", "\x7f", "?", "#"):
                value = ("https://fixture-user:" + boundary + "fixture-pass@service.invalid").encode(encoding)
                with self.subTest(encoding=encoding, boundary=ord(boundary)):
                    self.assertNotIn("authenticated-url", privacy.scan_bytes(value, []))

    def test_authenticated_url_diagnostics_report_structure_only_and_are_bounded(self):
        value = "https://fixture-user:合成口令🙂@service.invalid"
        for encoding in ("utf-16le", "utf-16be"):
            data = b"MZ\x81" + value.encode(encoding) + b"\x00\x00"
            diagnostics = []
            self.assertIn("authenticated-url", privacy.scan_bytes(data, [], diagnostics=diagnostics))
            self.assertTrue(diagnostics)
            self.assertTrue(any(item["view"] == encoding and item["offset"] == 3 for item in diagnostics))
            for item in diagnostics:
                self.assertEqual(set(item), {"view", "offset", "length", "control_count", "query_fragment_count", "non_ascii_count", "utf8_valid"})
                self.assertEqual(item["control_count"], 0)
                self.assertEqual(item["query_fragment_count"], 0)
                self.assertGreater(item["length"], 0)
                self.assertTrue(item["utf8_valid"])
            self.assertNotIn(value, str(diagnostics))
            self.assertNotIn("fixture-user", str(diagnostics))
            self.assertNotIn("service.invalid", str(diagnostics))
        invalid_utf8 = []
        privacy.scan_bytes(b"http://fixture-user:fixture-\xff-pass@", [], diagnostics=invalid_utf8)
        self.assertFalse(invalid_utf8[0]["utf8_valid"])
        bounded = []
        privacy.scan_bytes((b"https://fixture-user:fixture-pass@service.invalid\x00" * 100), [], diagnostics=bounded)
        self.assertEqual(len(bounded), 20)

    def test_authenticated_url_cli_diagnostics_never_echo_credentials_or_hostname(self):
        subprocess.run(["git", "init", "-q", str(self.root)], check=True, capture_output=True)
        subprocess.run(["git", "-C", str(self.root), "add", "skills"], check=True, capture_output=True)
        secret = "https://fixture-user:fixture-pass@service.invalid/api"
        self.resource.write_text(secret, encoding="utf-8")
        result = subprocess.run([sys.executable, "-B", str(SCRIPT), "--root", str(self.root)], capture_output=True, text=True)
        self.assertEqual(result.returncode, 1)
        output = result.stdout + result.stderr
        for value in (secret, "fixture-user", "fixture-pass", "service.invalid", str(self.root)):
            self.assertNotIn(value, output)
        rows = [json.loads(line.removeprefix("privacy detail: ")) for line in output.splitlines() if line.startswith("privacy detail: ")]
        self.assertTrue(rows)
        self.assertEqual(rows[0]["path"], "skills/single-cell/example/SKILL.md")
        self.assertEqual(rows[0]["view"], "raw")

    def test_examples_public_attribution_and_variable_access_are_allowed(self):
        text = '\n'.join([
            '"ZOTERO_API_KEY": "<your zotero.org API key with library write access>"',
            'api_key = "${API_KEY}"', 'password = "REDACTED"',
            'token = "identifier"', 'value = os.environ.get("API_KEY")',
            'Author: Public Contributor <author@example.org>',
            'https://matplotlib.org/stable/users/explain/figure/backends.html',
            'label=zh?"密码":"Password":"API key / token"',
            'task-abcdefghijklmnopqrstuvwxyza',
        ])
        self.assertEqual(privacy.scan_bytes(text.encode(), []), set())

    def test_local_paths_detected_as_utf8_escaped_and_utf16(self):
        prefix = r"C:\Users\fixture-developer"
        for value, encoding in ((prefix, "utf-8"), (prefix.replace("\\", "/"), "utf-8"), (prefix.replace("\\", "\\\\"), "utf-8"), (prefix, "utf-16le")):
            with self.subTest(encoding=encoding, escaped="\\\\" in value):
                self.assertIn("local-build-path", privacy.scan_bytes(value.encode(encoding), [prefix]))

    def test_dist_and_uncompressed_binary_scanned(self):
        dist = self.root / "dist"
        dist.mkdir()
        (dist / "app.js").write_text('api_key="synthetic-dist-secret"', encoding="utf-8")
        binary = self.root / "app.exe"
        binary.write_bytes(b"MZ\x00" + b"ghp_" + b"Z7" * 20)
        findings = self.audit(dist=dist, binaries=[binary])
        self.assertIn(("dist/app.js", "credential-literal"), findings)
        self.assertIn(("app.exe", "credential-token"), findings)

    def test_missing_dist_or_binary_fails(self):
        self.assertIn("missing-input", self.rules(self.audit(dist=self.root / "missing")))
        self.assertIn("missing-input", self.rules(self.audit(binaries=[self.root / "missing.exe"])))

    def test_root_alias_preserves_relative_build_inputs_and_missing_files(self):
        alias = self.root.parent / "repository-alias"
        dist = self.root / "dist"
        dist.mkdir()
        (dist / "app.js").write_text('api_key="synthetic-dist-secret"', encoding="utf-8")
        binary = self.root / "app.exe"
        binary.write_bytes(b"ghp_" + b"Z7" * 20)
        real_resolve = Path.resolve
        with mock.patch.object(Path, "resolve", autospec=True, side_effect=lambda path, **kwargs:
                               self.root if path == alias else real_resolve(path, **kwargs)):
            findings = privacy.audit_repo(alias, dist=alias / "dist", binaries=[
                alias / "app.exe", alias / "missing.exe",
            ], tracked_files={"skills/single-cell/example/SKILL.md"}, forbidden_prefixes=[])
        self.assertEqual(set(findings), {
            ("dist/app.js", "credential-literal"),
            ("app.exe", "credential-token"),
            ("missing.exe", "missing-input"),
        })

    def test_root_alias_does_not_resolve_away_descendant_links(self):
        alias = self.root.parent / "repository-alias"
        linked = self.root / "dist"
        real_resolve = Path.resolve
        real_link = privacy.is_link
        with mock.patch.object(Path, "resolve", autospec=True, side_effect=lambda path, **kwargs:
                               self.root if path == alias else real_resolve(path, **kwargs)), \
             mock.patch.object(privacy, "is_link", side_effect=lambda path:
                               path == linked or real_link(path)):
            findings = privacy.audit_repo(alias, dist=alias / "dist", binaries=[alias / "dist/app.exe"],
                tracked_files={"skills/single-cell/example/SKILL.md"}, forbidden_prefixes=[])
        self.assertEqual(set(findings), {
            ("dist", "resource-link"), ("dist/app.exe", "resource-link"),
        })

    def test_external_and_parent_traversal_inputs_are_rejected(self):
        for candidate in (self.root.parent / "outside", self.root / ".." / "outside"):
            with self.subTest(candidate=candidate.name):
                self.assertEqual(self.rules(self.audit(dist=candidate)), {"external-input"})
                self.assertEqual(self.rules(self.audit(binaries=[candidate])), {"external-input"})

    def test_cli_never_echoes_secret_or_personal_absolute_path(self):
        subprocess.run(["git", "init", "-q", str(self.root)], check=True, capture_output=True)
        subprocess.run(["git", "-C", str(self.root), "add", "skills"], check=True, capture_output=True)
        secret = "sk-proj-" + "S4" * 30
        self.resource.write_text(secret, encoding="utf-8")
        result = subprocess.run([sys.executable, "-B", str(SCRIPT), "--root", str(self.root)], capture_output=True, text=True)
        self.assertEqual(result.returncode, 1)
        output = result.stdout + result.stderr
        self.assertIn("credential-token", output)
        self.assertNotIn(secret, output)
        self.assertNotIn(str(self.root), output)


if __name__ == "__main__":
    unittest.main()
