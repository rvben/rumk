#!/usr/bin/env python3
"""Tie a VS Code extension release to each Rumk release.

`bump` gives the extension the next patch version and a changelog section
naming the Rumk release it bundles; it is idempotent, so an interrupted
release can run it again. `check` verifies that the extension files agree and
that a released extension version bundles the given Rumk version.
"""
import argparse
import json
from pathlib import Path
import re
import sys

ROOT = Path(__file__).resolve().parents[1]
EXTENSION = ROOT / "editors/vscode"


def rumk_version(root=ROOT):
    manifest = (root / "Cargo.toml").read_text()
    package = re.search(r'^\[package\]\n(?:(?!\[).*\n)*?version = "([^"]+)"', manifest, re.MULTILINE)
    if not package:
        raise ValueError("Cargo.toml has no package version")
    return package.group(1)


def next_patch(version):
    match = re.fullmatch(r"(\d+)\.(\d+)\.(\d+)", version)
    if not match:
        raise ValueError(f"extension version is not MAJOR.MINOR.PATCH: {version}")
    major, minor, patch = (int(part) for part in match.groups())
    return f"{major}.{minor}.{patch + 1}"


def bundle_line(version):
    return (f"- Bundle Rumk {version}. See the [Rumk {version} release notes]"
            f"(https://github.com/rvben/rumk/releases/tag/v{version}).")


def bundles(text, version):
    return re.search(rf"^- Bundle Rumk {re.escape(version)}\.", text, re.MULTILINE) is not None


def replace_once(text, old, new, description):
    if text.count(old) != 1:
        raise ValueError(f"expected exactly one {description}, found {text.count(old)}")
    return text.replace(old, new)


def set_versions(extension, old, new):
    package = extension / "package.json"
    package.write_text(replace_once(package.read_text(), f'\n  "version": "{old}",', f'\n  "version": "{new}",',
                                    "top-level version in package.json"))
    lock = extension / "package-lock.json"
    text = replace_once(lock.read_text(), f'\n  "version": "{old}",', f'\n  "version": "{new}",',
                        "top-level version in package-lock.json")
    root_package = '\n    "": {\n      "name": "rumk",\n      "version": '
    lock.write_text(replace_once(text, f'{root_package}"{old}",', f'{root_package}"{new}",',
                                 "root package version in package-lock.json"))


def release_changelog(text, extension_version, version):
    line = bundle_line(version)
    unreleased = "\n## Unreleased\n\n"
    if unreleased in text:
        return text.replace(unreleased, f"\n## {extension_version}\n\n{line}\n", 1)
    return replace_once(text, "# Changelog\n\n", f"# Changelog\n\n## {extension_version}\n\n{line}\n\n",
                        "changelog title")


def bump(extension=EXTENSION, version=None):
    version = version or rumk_version()
    changelog = extension / "CHANGELOG.md"
    text = changelog.read_text()
    current = json.loads((extension / "package.json").read_text())["version"]
    if bundles(text, version):
        print(f"extension {current} already bundles Rumk {version}")
        return current
    new = next_patch(current)
    set_versions(extension, current, new)
    changelog.write_text(release_changelog(text, new, version))
    check(extension, version)
    print(f"extension {new} bundles Rumk {version}")
    return new


def check(extension=EXTENSION, version=None):
    version = version or rumk_version()
    package = json.loads((extension / "package.json").read_text())["version"]
    lock = json.loads((extension / "package-lock.json").read_text())
    locked = (lock["version"], lock["packages"][""]["version"])
    if locked != (package, package):
        raise ValueError(f"package-lock.json names {locked}, package.json names {package}")
    sections = re.split(r"^## ", (extension / "CHANGELOG.md").read_text(), flags=re.MULTILINE)[1:]
    released = {section.split("\n", 1)[0].strip(): section for section in sections}
    if package not in released:
        raise ValueError(f"editors/vscode/CHANGELOG.md has no section for extension {package}")
    if not any(bundles(section, version) for name, section in released.items() if name != "Unreleased"):
        raise ValueError(f"no released extension version in editors/vscode/CHANGELOG.md bundles Rumk {version}")


if __name__ == "__main__":
    parser = argparse.ArgumentParser(description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter)
    parser.add_argument("command", choices=["bump", "check"])
    parser.add_argument("--version", help="Rumk version (default: the version in Cargo.toml)")
    args = parser.parse_args()
    try:
        (bump if args.command == "bump" else check)(version=args.version)
    except ValueError as error:
        print(f"extension release: {error}", file=sys.stderr)
        sys.exit(1)
