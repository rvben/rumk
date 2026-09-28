#!/usr/bin/env python3
"""Publish the platform packages the Marketplace does not have yet.

A release that failed partway, or runs again, publishes only the targets that
are missing for this version instead of failing on the ones already there.
"""
import argparse
import json
from pathlib import Path
import subprocess

EXTENSION = Path(__file__).resolve().parents[1]
VSCE = EXTENSION / 'node_modules/.bin/vsce'


def target(package, version):
    prefix = f'rumk-{version}-'
    if not package.name.startswith(prefix) or package.suffix != '.vsix':
        raise ValueError(f'Unexpected package name for {version}: {package.name}')
    return package.name[len(prefix):-len('.vsix')]


def unpublished(packages, version, listing):
    published = {entry.get('targetPlatform') for entry in listing.get('versions', []) if entry.get('version') == version}
    return [package for package in packages if target(package, version) not in published]


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('directory', type=Path)
    args = parser.parse_args()
    version = json.loads((EXTENSION / 'package.json').read_text())['version']
    packages = sorted(args.directory.glob('*.vsix'))
    if not packages:
        raise SystemExit(f'No packages in {args.directory}')
    listing = json.loads(subprocess.run([VSCE, 'show', 'rvben.rumk', '--json'], check=True, capture_output=True, text=True).stdout)
    missing = unpublished(packages, version, listing)
    for package in sorted(set(packages) - set(missing)):
        print(f'rvben.rumk ({target(package, version)}) {version} is already published; skipping')
    if not missing:
        return
    subprocess.run([VSCE, 'verify-pat', 'rvben'], check=True)
    subprocess.run([VSCE, 'publish', '--packagePath', *map(str, missing)], check=True)


if __name__ == '__main__':
    main()
