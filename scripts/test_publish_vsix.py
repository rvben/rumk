import importlib.util
from pathlib import Path
import unittest

ROOT = Path(__file__).resolve().parents[1]
SPEC = importlib.util.spec_from_file_location("publish_vsix", ROOT / "editors/vscode/scripts/publish-vsix.py")
PUBLISH = importlib.util.module_from_spec(SPEC)
SPEC.loader.exec_module(PUBLISH)

TARGETS = ["darwin-arm64", "darwin-x64", "linux-arm64", "linux-x64", "win32-x64"]


class PublishVsixTest(unittest.TestCase):
    def packages(self, version):
        return [Path(f"packages/rumk-{version}-{target}.vsix") for target in TARGETS]

    def test_publishes_every_target_of_a_new_version(self):
        listing = {"versions": [{"version": "0.0.3", "targetPlatform": target} for target in TARGETS]}
        self.assertEqual(PUBLISH.unpublished(self.packages("0.0.4"), "0.0.4", listing), self.packages("0.0.4"))

    def test_skips_targets_already_published_for_the_version(self):
        listing = {"versions": [{"version": "0.0.4", "targetPlatform": "darwin-arm64"},
                                {"version": "0.0.4", "targetPlatform": "linux-x64"},
                                {"version": "0.0.3", "targetPlatform": "win32-x64"}]}
        missing = PUBLISH.unpublished(self.packages("0.0.4"), "0.0.4", listing)
        self.assertEqual([PUBLISH.target(package, "0.0.4") for package in missing],
                         ["darwin-x64", "linux-arm64", "win32-x64"])

    def test_nothing_to_publish_when_every_target_is_listed(self):
        listing = {"versions": [{"version": "0.0.4", "targetPlatform": target} for target in TARGETS]}
        self.assertEqual(PUBLISH.unpublished(self.packages("0.0.4"), "0.0.4", listing), [])

    def test_rejects_a_package_for_another_version(self):
        with self.assertRaisesRegex(ValueError, "Unexpected package name"):
            PUBLISH.unpublished(self.packages("0.0.3"), "0.0.4", {"versions": []})


if __name__ == "__main__":
    unittest.main()
