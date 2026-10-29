"""The installed skill resources use one flat package directory."""

import hashlib
import json
from pathlib import Path
import unittest


ROOT = Path(__file__).resolve().parents[1]
SKILLS = ROOT / "skills"
PROVENANCE = ROOT / "docs/third-party/skills"


class BundledSkillLayoutTests(unittest.TestCase):
    def test_all_packages_are_direct_children_of_skills(self):
        directories = {path.name for path in SKILLS.iterdir() if path.is_dir()}
        self.assertEqual(len(directories), 35)
        for name in directories:
            self.assertTrue((SKILLS / name / "SKILL.md").is_file(), name)
        self.assertFalse((SKILLS / "single-cell").exists())
        self.assertFalse((SKILLS / "wisp-science").exists())

    def test_bundle_preserves_default_and_host_compatibility_metadata(self):
        bundle = json.loads((SKILLS / "BUNDLE.json").read_text(encoding="utf-8"))
        self.assertEqual(len(bundle["default_enabled"]), 18)
        self.assertEqual(len(bundle["categories"]["single_cell"]), 9)
        self.assertEqual(len(bundle["categories"]["wisp_science"]), 26)
        for category in bundle["categories"].values():
            for name in category:
                self.assertTrue((SKILLS / name / "SKILL.md").is_file(), name)

    def test_packaging_keeps_licenses_outside_the_skills_directory(self):
        config = json.loads((ROOT / "src-tauri/tauri.conf.json").read_text(encoding="utf-8"))
        resources = config["bundle"]["resources"]
        self.assertEqual(resources.get("../skills/"), "skills/")
        self.assertEqual(resources.get("../docs/third-party/skills/"), "licenses/skills/")
        self.assertNotIn("../skills/single-cell/", resources)
        self.assertNotIn("../skills/wisp-science/", resources)
        self.assertTrue((PROVENANCE / "single-cell/LICENSE.upstream.md").is_file())
        self.assertTrue((PROVENANCE / "wisp-science/LICENSE.upstream").is_file())
        self.assertTrue((PROVENANCE / "wisp-science/Apache-2.0.upstream.txt").is_file())

    def test_wisp_snapshot_bytes_are_preserved_after_relocation(self):
        source = json.loads((PROVENANCE / "wisp-science/SOURCE.json").read_text(encoding="utf-8"))
        self.assertEqual(len(source["packages"]), 26)
        for package in source["packages"]:
            for file in package["files"]:
                path = SKILLS / file["path"]
                self.assertEqual(hashlib.sha256(path.read_bytes()).hexdigest(), file["sha256"], file["path"])


if __name__ == "__main__":
    unittest.main()
