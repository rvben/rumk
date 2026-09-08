#!/usr/bin/env python3
"""Reject incomplete or mismatched Marketplace release sets before publishing."""
import argparse
import hashlib
import json
from pathlib import Path
import xml.etree.ElementTree as ET
import zipfile

TARGETS = {
    'darwin-arm64': 'aarch64-apple-darwin',
    'darwin-x64': 'x86_64-apple-darwin',
    'linux-arm64': 'aarch64-unknown-linux-musl',
    'linux-x64': 'x86_64-unknown-linux-musl',
    'win32-x64': 'x86_64-pc-windows-msvc',
}


def verify(directory, version, revision):
    found = set()
    for package in sorted(directory.glob('*.vsix')):
        with zipfile.ZipFile(package) as archive:
            manifest = json.loads(archive.read('extension/package.json'))
            bundle = json.loads(archive.read('extension/bundled/metadata.json'))
            xml = ET.fromstring(archive.read('extension.vsixmanifest'))
            identity = next(element for element in xml.iter() if element.tag.endswith('}Identity'))
            properties = {element.attrib['Id']: element.attrib['Value'] for element in xml.iter() if element.tag.endswith('}Property')}
            target = identity.attrib['TargetPlatform']
            if target not in TARGETS or target in found:
                raise ValueError(f'Unexpected or duplicate target: {target}')
            if (manifest['publisher'], manifest['name'], manifest['version']) != ('rvben', 'rumk', version):
                raise ValueError(f'Unexpected extension identity: {package}')
            if (identity.attrib['Publisher'], identity.attrib['Id'], identity.attrib['Version']) != ('rvben', 'rumk', version):
                raise ValueError(f'Unexpected VSIX identity: {package}')
            if properties.get('Microsoft.VisualStudio.Code.PreRelease') != 'true':
                raise ValueError(f'Not a preview package: {package}')
            if bundle['target'] != target or bundle['rustTarget'] != TARGETS[target]:
                raise ValueError(f'Mismatched bundled target: {package}')
            if bundle['revision'] != revision or bundle['sourceDirty']:
                raise ValueError(f'Package does not contain the clean release revision: {package}')
            binary_name = 'rumk.exe' if target.startswith('win32-') else 'rumk'
            binary = archive.getinfo(f'extension/bundled/{binary_name}')
            if hashlib.sha256(archive.read(binary)).hexdigest() != bundle['sha256']:
                raise ValueError(f'Binary checksum mismatch: {package}')
            if not target.startswith('win32-') and not ((binary.external_attr >> 16) & 0o111):
                raise ValueError(f'Binary is not executable: {package}')
            for notice in ['bundled/THIRD_PARTY_NOTICES.txt', 'dist/THIRD_PARTY_NOTICES.txt']:
                if not archive.read('extension/' + notice):
                    raise ValueError(f'Missing license notices: {package}')
            found.add(target)
    if found != set(TARGETS):
        raise ValueError(f'Missing platform packages: {sorted(set(TARGETS) - found)}')
    print(f'Verified all {len(found)} platform packages for rvben.rumk {version} at {revision}.')


if __name__ == '__main__':
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('directory', type=Path)
    parser.add_argument('--revision', required=True)
    args = parser.parse_args()
    extension_manifest = Path(__file__).resolve().parents[1] / 'package.json'
    verify(args.directory, json.loads(extension_manifest.read_text())['version'], args.revision)
