import importlib.util
import json
from pathlib import Path
import shutil
import tempfile
import unittest

ROOT = Path(__file__).resolve().parents[1]
SPEC = importlib.util.spec_from_file_location("extension_release", ROOT / "scripts/extension-release.py")
RELEASE = importlib.util.module_from_spec(SPEC)
SPEC.loader.exec_module(RELEASE)


class ExtensionReleaseTest(unittest.TestCase):
    def setUp(self):
        directory = tempfile.TemporaryDirectory(prefix="rumk-extension-release-")
        self.addCleanup(directory.cleanup)
        self.extension = Path(directory.name)
        for name in ["package.json", "package-lock.json"]:
            shutil.copy(ROOT / "editors/vscode" / name, self.extension / name)
        self.version = json.loads((self.extension / "package.json").read_text())["version"]
        self.write_changelog(f"## {self.version}\n\n- Fix a bug.\n")

    def write_changelog(self, sections):
        (self.extension / "CHANGELOG.md").write_text(f"# Changelog\n\n{sections}")

    def changelog(self):
        return (self.extension / "CHANGELOG.md").read_text()

    def versions(self):
        lock = json.loads((self.extension / "package-lock.json").read_text())
        return (json.loads((self.extension / "package.json").read_text())["version"],
                lock["version"], lock["packages"][""]["version"])

    def test_bump_adds_a_section_naming_the_rumk_release(self):
        new = RELEASE.bump(self.extension, "9.8.7")
        self.assertEqual(new, RELEASE.next_patch(self.version))
        self.assertEqual(self.versions(), (new, new, new))
        self.assertEqual(self.changelog(), (
            f"# Changelog\n\n## {new}\n\n{RELEASE.bundle_line('9.8.7')}\n\n"
            f"## {self.version}\n\n- Fix a bug.\n"))

    def test_bump_releases_unreleased_notes_under_the_new_version(self):
        self.write_changelog(f"## Unreleased\n\n- Keep edits.\n\n## {self.version}\n\n- Fix a bug.\n")
        new = RELEASE.bump(self.extension, "9.8.7")
        self.assertEqual(self.changelog(), (
            f"# Changelog\n\n## {new}\n\n{RELEASE.bundle_line('9.8.7')}\n- Keep edits.\n\n"
            f"## {self.version}\n\n- Fix a bug.\n"))

    def test_bump_runs_again_without_bumping_twice(self):
        new = RELEASE.bump(self.extension, "9.8.7")
        changelog = self.changelog()
        self.assertEqual(RELEASE.bump(self.extension, "9.8.7"), new)
        self.assertEqual(self.versions(), (new, new, new))
        self.assertEqual(self.changelog(), changelog)

    def test_bump_leaves_dependency_versions_alone(self):
        before = json.loads((self.extension / "package-lock.json").read_text())
        RELEASE.bump(self.extension, "9.8.7")
        after = json.loads((self.extension / "package-lock.json").read_text())
        for name, package in before["packages"].items():
            if name:
                self.assertEqual(after["packages"][name], package)

    def test_check_rejects_a_release_the_extension_does_not_bundle(self):
        with self.assertRaisesRegex(ValueError, "bundles Rumk 9.8.7"):
            RELEASE.check(self.extension, "9.8.7")

    def test_check_ignores_a_bundle_line_that_is_not_yet_released(self):
        self.write_changelog(f"## Unreleased\n\n{RELEASE.bundle_line('9.8.7')}\n\n## {self.version}\n\n- Fix a bug.\n")
        with self.assertRaisesRegex(ValueError, "bundles Rumk 9.8.7"):
            RELEASE.check(self.extension, "9.8.7")

    def test_check_rejects_a_version_without_a_changelog_section(self):
        self.write_changelog(f"## 0.0.0\n\n{RELEASE.bundle_line('9.8.7')}\n")
        with self.assertRaisesRegex(ValueError, f"no section for extension {self.version}"):
            RELEASE.check(self.extension, "9.8.7")

    def test_check_rejects_a_lockfile_that_disagrees(self):
        RELEASE.bump(self.extension, "9.8.7")
        lock = self.extension / "package-lock.json"
        lock.write_text(lock.read_text().replace(f'"version": "{self.versions()[0]}"', '"version": "0.0.0"', 1))
        with self.assertRaisesRegex(ValueError, "package-lock.json names"):
            RELEASE.check(self.extension, "9.8.7")

    def test_repository_extension_bundles_the_current_rumk_release(self):
        RELEASE.check()

    def test_rumk_version_reads_the_package_section(self):
        with tempfile.TemporaryDirectory() as directory:
            (Path(directory) / "Cargo.toml").write_text(
                '[workspace]\nversion = "1.0.0"\n\n[package]\nname = "rumk"\nversion = "2.3.4"\n')
            self.assertEqual(RELEASE.rumk_version(Path(directory)), "2.3.4")


if __name__ == "__main__":
    unittest.main()
