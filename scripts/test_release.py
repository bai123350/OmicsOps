"""Offline release-helper contract tests; no native build or network is required."""

import argparse
import contextlib
import hashlib
import importlib.util
import io
import json
from pathlib import Path
import shutil
import subprocess
import sys
import tempfile
import unittest
from unittest import mock


SCRIPT = Path(__file__).with_name("release.py")
MODULE_SPEC = importlib.util.spec_from_file_location("release", SCRIPT)
release = importlib.util.module_from_spec(MODULE_SPEC)
MODULE_SPEC.loader.exec_module(release)
VERSION = "1.2.3"
COMMIT = "0123456789abcdef0123456789abcdef01234567"
TARGETS = {
    "windows-x64": [("nsis", ".exe"), ("msi", ".msi")],
    "linux-x64": [("deb", ".deb"), ("appimage", ".AppImage")],
    "macos-arm64": [("dmg", ".dmg")],
    "macos-x64": [("dmg", ".dmg")],
}


class ReleaseTests(unittest.TestCase):
    def setUp(self):
        self.temp = tempfile.TemporaryDirectory()
        self.addCleanup(self.temp.cleanup)
        self.root = Path(self.temp.name)
        self.repo = self.root / "repo"
        self.repo.mkdir()
        self.write_repo()

    def write_json(self, path, value):
        path.parent.mkdir(parents=True, exist_ok=True)
        path.write_text(json.dumps(value), encoding="utf-8")

    def write_repo(self, version=VERSION):
        self.write_json(self.repo / "package.json", {"version": version})
        self.write_json(self.repo / "package-lock.json", {
            "version": version, "packages": {"": {"version": version}},
        })
        self.write_json(self.repo / "src-tauri/tauri.conf.json", {"version": version})
        (self.repo / "Cargo.toml").write_text(
            '[workspace]\nmembers = ["crates/*", "src-tauri"]\n'
            '[workspace.package]\nversion = "' + version + '"\n',
            encoding="utf-8",
        )
        for member, name in [("crates/core", "omicsops-core"), ("src-tauri", "omicsops")]:
            path = self.repo / member / "Cargo.toml"
            path.parent.mkdir(parents=True, exist_ok=True)
            path.write_text(
                f'[package]\nname = "{name}"\nversion.workspace = true\n',
                encoding="utf-8",
            )
        (self.repo / "Cargo.lock").write_text(
            'version = 4\n'
            f'[[package]]\nname = "omicsops-core"\nversion = "{version}"\n'
            f'[[package]]\nname = "omicsops"\nversion = "{version}"\n'
            '[[package]]\nname = "dependency"\nversion = "9.8.7"\n'
            'source = "registry+https://example.invalid"\n',
            encoding="utf-8",
        )

    def run_cli(self, *args, ok=True):
        result = subprocess.run(
            [sys.executable, "-B", str(SCRIPT), *map(str, args)],
            cwd=self.root, capture_output=True, text=True, check=False,
        )
        if ok:
            self.assertEqual(result.returncode, 0, result.stderr)
        else:
            self.assertNotEqual(result.returncode, 0, result.stdout)
            self.assertIn("release error:", result.stderr)
        return result

    def check_version(self, tag="v1.2.3", **kwargs):
        return self.run_cli("check-version", "--root", self.repo, "--tag", tag, **kwargs)

    def bundle(self, target):
        bundle = self.root / "bundles" / target
        for folder, extension in TARGETS[target]:
            installer = bundle / folder / ("original" + extension)
            installer.parent.mkdir(parents=True, exist_ok=True)
            installer.write_bytes((target + extension).encode())
        return bundle

    def stage(self, target, *, bundle=None, output=None, **kwargs):
        bundle = bundle or self.bundle(target)
        output = output or self.root / "staged" / target
        self.run_cli(
            "stage", "--target", target, "--bundle-dir", bundle,
            "--output-dir", output, "--version", VERSION, "--commit", COMMIT,
            **kwargs,
        )
        return output

    def assembly_input(self):
        incoming = self.root / "incoming"
        incoming.mkdir()
        for target in TARGETS:
            for asset in self.stage(target).iterdir():
                shutil.copyfile(asset, incoming / asset.name)
        return incoming

    def assemble(self, incoming, *, output=None, **kwargs):
        output = output or self.root / "release"
        self.run_cli(
            "assemble", "--input-dir", incoming, "--output-dir", output,
            "--version", VERSION, "--commit", COMMIT, **kwargs,
        )
        return output

    def mutate_manifest(self, incoming, mutate, target="windows-x64"):
        path = incoming / f"manifest-{target}.json"
        manifest = json.loads(path.read_text(encoding="utf-8"))
        mutate(manifest)
        self.write_json(path, manifest)

    def assert_no_output(self):
        self.assertFalse((self.root / "release").exists())

    def test_check_version_validates_all_sources_and_emits_actions_outputs(self):
        output = self.root / "github-output"
        output.write_text("existing=value\n", encoding="utf-8")
        self.run_cli(
            "check-version", "--root", self.repo, "--tag", "v1.2.3",
            "--github-output", output,
        )
        self.assertEqual(output.read_text(encoding="utf-8"),
                         "existing=value\nversion=1.2.3\ntag=v1.2.3\ncreate_release=true\n")

    def test_empty_tag_builds_artifacts_only(self):
        output = self.root / "github-output"
        self.run_cli(
            "check-version", "--root", self.repo, "--tag", "",
            "--github-output", output,
        )
        self.assertEqual(output.read_text(encoding="utf-8"),
                         "version=1.2.3\ntag=\ncreate_release=false\n")

    def test_tag_mismatch_does_not_write_outputs(self):
        output = self.root / "github-output"
        self.run_cli(
            "check-version", "--root", self.repo, "--tag", "v1.2.4",
            "--github-output", output, ok=False,
        )
        self.assertFalse(output.exists())

    def test_version_drift_in_each_source_is_rejected(self):
        for filename, mutate in [
            ("package.json", lambda data: data.update(version="1.2.4")),
            ("package-lock.json", lambda data: data.update(version="1.2.4")),
            ("package-lock.json", lambda data: data["packages"][""].update(version="1.2.4")),
            ("src-tauri/tauri.conf.json", lambda data: data.update(version="1.2.4")),
        ]:
            with self.subTest(filename=filename, mutate=mutate):
                self.write_repo()
                path = self.repo / filename
                data = json.loads(path.read_text(encoding="utf-8"))
                mutate(data)
                self.write_json(path, data)
                self.check_version(ok=False)
        for filename in ["Cargo.toml", "Cargo.lock"]:
            with self.subTest(filename=filename):
                self.write_repo()
                path = self.repo / filename
                path.write_text(path.read_text(encoding="utf-8").replace(VERSION, "1.2.4", 1),
                                encoding="utf-8")
                self.check_version(ok=False)

    def test_missing_workspace_lock_package_is_rejected(self):
        path = self.repo / "Cargo.lock"
        path.write_text('version = 4\n[[package]]\nname = "omicsops"\nversion = "1.2.3"\n',
                        encoding="utf-8")
        self.check_version(ok=False)

    def test_duplicate_workspace_lock_package_is_rejected(self):
        path = self.repo / "Cargo.lock"
        with path.open("a", encoding="utf-8") as lock:
            lock.write('[[package]]\nname = "omicsops"\nversion = "1.2.3"\n')
        self.check_version(ok=False)

    def test_malformed_versions_and_msi_overflow_are_rejected(self):
        for version in ["1.2", "1.2.3.4", "01.2.3", "v1.2.3", "1.2.3-beta.1",
                        "1.2.3+build", "256.0.0", "1.256.0", "1.2.65536", "1.2.3\n"]:
            with self.subTest(version=version):
                self.write_repo(version)
                self.check_version(tag="", ok=False)

    def test_msi_boundary_version_is_accepted(self):
        self.write_repo("255.255.65535")
        self.check_version("v255.255.65535")

    def test_stage_normalizes_names_and_records_content(self):
        staged = self.stage("windows-x64")
        self.assertEqual(sorted(p.name for p in staged.iterdir()), [
            "OmicsOps_1.2.3_x64-setup.exe", "OmicsOps_1.2.3_x64_en-US.msi",
            "manifest-windows-x64.json",
        ])
        manifest = json.loads((staged / "manifest-windows-x64.json").read_text(encoding="utf-8"))
        self.assertEqual((manifest["schema_version"], manifest["version"],
                          manifest["commit"], manifest["target"]),
                         (1, VERSION, COMMIT, "windows-x64"))
        for entry in manifest["files"]:
            payload = (staged / entry["name"]).read_bytes()
            self.assertEqual(entry["size"], len(payload))
            self.assertEqual(entry["sha256"], hashlib.sha256(payload).hexdigest())

    def test_stage_rejects_missing_duplicate_and_empty_installers_before_copying(self):
        for case in ["missing", "duplicate", "empty"]:
            with self.subTest(case=case):
                bundle = self.bundle("windows-x64")
                installer = bundle / "msi/original.msi"
                if case == "missing":
                    installer.unlink()
                elif case == "duplicate":
                    (installer.parent / "duplicate.msi").write_bytes(b"duplicate")
                else:
                    (installer.parent / "duplicate.msi").unlink(missing_ok=True)
                    installer.write_bytes(b"")
                output = self.root / f"bad-{case}"
                self.stage("windows-x64", bundle=bundle, output=output, ok=False)
                self.assertFalse(output.exists())

    def test_stage_rejects_nonempty_destination(self):
        output = self.root / "existing"
        output.mkdir()
        (output / "keep.txt").write_text("untouched", encoding="utf-8")
        self.stage("macos-arm64", output=output, ok=False)
        self.assertEqual([p.name for p in output.iterdir()], ["keep.txt"])

    def test_stage_ignores_appimage_working_tree_and_internal_links(self):
        bundle = self.bundle("linux-x64")
        app_dir = bundle / "appimage/OmicsOps.AppDir"
        desktop_file = app_dir / "usr/share/applications/OmicsOps.desktop"
        desktop_file.parent.mkdir(parents=True)
        desktop_file.write_text("[Desktop Entry]\nName=OmicsOps\n", encoding="utf-8")
        (app_dir / "OmicsOps.png").write_bytes(b"icon")
        links = {app_dir / ".DirIcon", app_dir / "OmicsOps.desktop"}
        for path in links:
            path.write_bytes(b"link placeholder")
        (app_dir / "usr/embedded.AppImage").write_bytes(b"internal resource")
        original_is_symlink = Path.is_symlink
        output = self.root / "appimage-staged"
        args = argparse.Namespace(target="linux-x64", bundle_dir=bundle,
                                  output_dir=output, version=VERSION, commit=COMMIT)
        # Only replace link metadata; all enumeration, validation, hashing and copying
        # use real fixture files. This also runs on Windows without symlink privilege.
        with mock.patch.object(Path, "is_symlink", lambda path: path in links or original_is_symlink(path)):
            with contextlib.redirect_stdout(io.StringIO()):
                try:
                    release.stage(args)
                except release.ReleaseError as error:
                    self.fail(f"native AppDir contents must not block top-level installers: {error}")
        self.assertEqual(sorted(path.name for path in output.iterdir()), [
            "OmicsOps_1.2.3_amd64.AppImage", "OmicsOps_1.2.3_amd64.deb",
            "manifest-linux-x64.json",
        ])
        self.assertEqual((output / "OmicsOps_1.2.3_amd64.AppImage").read_bytes(),
                         b"linux-x64.AppImage")

    def test_stage_does_not_use_nested_file_as_missing_top_level_installer(self):
        bundle = self.bundle("linux-x64")
        (bundle / "appimage/original.AppImage").unlink()
        nested = bundle / "appimage/OmicsOps.AppDir/usr/embedded.AppImage"
        nested.parent.mkdir(parents=True)
        nested.write_bytes(b"internal resource")
        output = self.root / "nested-only"
        self.stage("linux-x64", bundle=bundle, output=output, ok=False)
        self.assertFalse(output.exists())

    def test_stage_rejects_top_level_link_metadata_without_symlink_privilege(self):
        bundle = self.bundle("linux-x64")
        installer = bundle / "appimage/original.AppImage"
        for method in ["is_symlink", "is_junction"]:
            with self.subTest(method=method):
                output = self.root / f"linked-{method}"
                args = argparse.Namespace(target="linux-x64", bundle_dir=bundle,
                                          output_dir=output, version=VERSION, commit=COMMIT)
                original = getattr(Path, method)
                with mock.patch.object(Path, method, lambda path: path == installer or original(path)):
                    with self.assertRaises(release.ReleaseError):
                        release.stage(args)
                self.assertFalse(output.exists())

    def test_stage_rejects_symlink_installer(self):
        bundle = self.bundle("windows-x64")
        installer = bundle / "msi/original.msi"
        installer.unlink()
        try:
            installer.symlink_to(bundle / "nsis/original.exe")
        except OSError as error:
            self.skipTest(f"OS does not permit creating symlinks: {error}")
        output = self.root / "bad-link"
        self.stage("windows-x64", bundle=bundle, output=output, ok=False)
        self.assertFalse(output.exists())

    def test_stage_rejects_invalid_commit(self):
        self.run_cli(
            "stage", "--target", "macos-x64", "--bundle-dir", self.bundle("macos-x64"),
            "--output-dir", self.root / "invalid-commit", "--version", VERSION,
            "--commit", "not-a-sha", ok=False,
        )
        self.assertFalse((self.root / "invalid-commit").exists())

    def test_complete_assembly_has_only_installers_checksums_and_manifest(self):
        output = self.assemble(self.assembly_input())
        expected = [
            "OmicsOps_1.2.3_aarch64.dmg", "OmicsOps_1.2.3_amd64.AppImage",
            "OmicsOps_1.2.3_amd64.deb", "OmicsOps_1.2.3_x64-setup.exe",
            "OmicsOps_1.2.3_x64.dmg", "OmicsOps_1.2.3_x64_en-US.msi",
        ]
        self.assertEqual(sorted(p.name for p in output.iterdir()),
                         expected + ["SHA256SUMS.txt", "release-manifest.json"])
        expected_checksums = "".join(
            hashlib.sha256((output / name).read_bytes()).hexdigest() + "  " + name + "\n"
            for name in expected + ["release-manifest.json"]
        )
        self.assertEqual((output / "SHA256SUMS.txt").read_text(encoding="utf-8"), expected_checksums)
        manifest = json.loads((output / "release-manifest.json").read_text(encoding="utf-8"))
        self.assertEqual((manifest["schema_version"], manifest["version"], manifest["commit"]),
                         (1, VERSION, COMMIT))
        self.assertEqual(sorted(item["target"] for item in manifest["targets"]), sorted(TARGETS))

    def test_assembly_rejects_missing_platform(self):
        incoming = self.assembly_input()
        (incoming / "manifest-macos-x64.json").unlink()
        self.assemble(incoming, ok=False)
        self.assert_no_output()

    def test_assembly_rejects_missing_installer(self):
        incoming = self.assembly_input()
        (incoming / "OmicsOps_1.2.3_x64_en-US.msi").unlink()
        self.assemble(incoming, ok=False)
        self.assert_no_output()

    def test_assembly_rejects_extra_file_or_duplicate_manifest(self):
        incoming = self.assembly_input()
        for filename in ["server.log", "manifest-copy.json"]:
            with self.subTest(filename=filename):
                extra = incoming / filename
                shutil.copyfile(incoming / "manifest-macos-x64.json", extra)
                self.assemble(incoming, ok=False)
                self.assert_no_output()
                extra.unlink()

    def test_assembly_rejects_nested_directory(self):
        incoming = self.assembly_input()
        (incoming / "unexpected").mkdir()
        self.assemble(incoming, ok=False)
        self.assert_no_output()

    def test_assembly_rejects_wrong_metadata_and_duplicate_entries(self):
        incoming = self.assembly_input()
        path = incoming / "manifest-windows-x64.json"
        original = path.read_bytes()
        for mutate in [
            lambda data: data.update(version="1.2.4"),
            lambda data: data.update(commit="a" * 40),
            lambda data: data.update(target="macos-x64"),
            lambda data: data.update(schema_version=2),
            lambda data: data["files"].append(data["files"][0]),
            lambda data: data["files"].pop(),
            lambda data: data["files"][0].update(size=True),
        ]:
            with self.subTest(mutation=mutate):
                path.write_bytes(original)
                self.mutate_manifest(incoming, mutate)
                self.assemble(incoming, ok=False)
                self.assert_no_output()

    def test_assembly_rejects_unsafe_or_unnormalized_names(self):
        incoming = self.assembly_input()
        path = incoming / "manifest-windows-x64.json"
        original = path.read_bytes()
        for name in ["../outside.exe", "..\\outside.exe", "C:\\outside.exe", "/outside.exe",
                     "line\nname.exe", "arbitrary.exe", "installer.exe:secret"]:
            with self.subTest(name=name):
                path.write_bytes(original)
                self.mutate_manifest(incoming, lambda data: data["files"][0].update(name=name))
                self.assemble(incoming, ok=False)
                self.assert_no_output()

    def test_assembly_rejects_tampered_or_empty_bytes(self):
        incoming = self.assembly_input()
        installer = incoming / "OmicsOps_1.2.3_x64.dmg"
        original = installer.read_bytes()
        for data in [b"x" * len(original), original + b"extra", b""]:
            with self.subTest(payload=data):
                installer.write_bytes(data)
                self.assemble(incoming, ok=False)
                self.assert_no_output()

    def test_assembly_rejects_nonempty_destination(self):
        incoming = self.assembly_input()
        output = self.root / "release"
        output.mkdir()
        (output / "keep.txt").write_text("untouched", encoding="utf-8")
        self.assemble(incoming, output=output, ok=False)
        self.assertEqual([p.name for p in output.iterdir()], ["keep.txt"])

    def test_assembly_rejects_symlink_asset(self):
        incoming = self.assembly_input()
        installer = incoming / "OmicsOps_1.2.3_x64.dmg"
        payload = self.root / "payload.dmg"
        shutil.copyfile(installer, payload)
        installer.unlink()
        try:
            installer.symlink_to(payload)
        except OSError as error:
            self.skipTest(f"OS does not permit creating symlinks: {error}")
        self.assemble(incoming, ok=False)
        self.assert_no_output()


if __name__ == "__main__":
    unittest.main()
