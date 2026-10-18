#!/usr/bin/env python3
"""Collect notices from Cargo's exact locked graph, including pinned local egui."""
import hashlib
import json
from pathlib import Path
import shutil
import subprocess
import tomllib

import vendored_egui


REGEX_SYNTAX_NAME = 'regex-syntax'
REGEX_SYNTAX_VERSION = '0.8.11'
REGEX_SYNTAX_SOURCE = 'registry+https://github.com/rust-lang/crates.io-index'
UNICODE_NOTICE_PATH = Path('src/unicode_tables/LICENSE-UNICODE')
UNICODE_NOTICE_BYTES = 2847
UNICODE_NOTICE_SHA256 = '74db5baf44a41b1000312c673544b3374e4198af5605c7f9080a402cec42cfa3'
UNICODE_NOTICE_SECTION = '''## Unicode-derived data

The regex-syntax 0.8.11 dependency includes Unicode-derived data, including the
Unicode 16.0.0 simple case-folding table used by Cedar's case-only matcher.
Its additional upstream data/software notice is retained unchanged from
`src/unicode_tables/LICENSE-UNICODE` in the reviewed crate at
`third-party-licenses/regex-syntax-0.8.11/LICENSE-UNICODE`.
Notice SHA-256: 74db5baf44a41b1000312c673544b3374e4198af5605c7f9080a402cec42cfa3.
'''


def reviewed_unicode_notice(root, packages):
    """One reviewed nested notice, never a recursive package-file exemption."""
    candidates = [package for package in packages if package['name'] == REGEX_SYNTAX_NAME]
    if not candidates:
        return None

    def require(condition, message):
        if not condition:
            raise ValueError('regex-syntax Unicode notice: ' + message)

    require(len(candidates) == 1, 'expected exactly one reviewed package')
    package = candidates[0]
    require(package['version'] == REGEX_SYNTAX_VERSION
            and package['source'] == REGEX_SYNTAX_SOURCE
            and package.get('license') == 'MIT OR Apache-2.0'
            and package.get('license_file') is None
            and package.get('repository') == 'https://github.com/rust-lang/regex',
            'unreviewed package identity/version/source')
    manifest = Path(package['manifest_path'])
    base = manifest.parent
    require(manifest.is_absolute() and manifest.name == 'Cargo.toml'
            and base.name == REGEX_SYNTAX_NAME + '-' + REGEX_SYNTAX_VERSION
            and manifest.is_file() and not manifest.is_symlink()
            and manifest.resolve(strict=True) == manifest,
            'unsafe or unexpected package manifest path')
    notice = base / UNICODE_NOTICE_PATH
    require(notice.is_file() and not notice.is_symlink()
            and notice.resolve(strict=True) == notice,
            'missing, unsafe or unexpected nested notice path')
    # A root-level lookalike must not collide with the one approved nested file.
    shadow = base / UNICODE_NOTICE_PATH.name
    require(not shadow.exists() and not shadow.is_symlink(), 'unexpected root notice path')
    require(notice.stat().st_size == UNICODE_NOTICE_BYTES, 'unreviewed nested notice size')
    content = notice.read_bytes()
    require(len(content) == UNICODE_NOTICE_BYTES
            and hashlib.sha256(content).hexdigest() == UNICODE_NOTICE_SHA256,
            'unreviewed nested notice bytes')
    destination = (root / 'third-party-licenses'
                   / (REGEX_SYNTAX_NAME + '-' + REGEX_SYNTAX_VERSION)
                   / UNICODE_NOTICE_PATH.name)
    require(not destination.is_symlink() and destination.resolve() == destination
            and (not destination.exists() or destination.is_file()),
            'unsafe notice destination')
    return destination, content


def workspace_identities(root, cargo):
    """Only the root's explicit, canonical workspace manifests are first-party."""
    workspace = cargo.get('workspace', {})
    identities = {}
    for member in workspace.get('members', []):
        relative = Path(member)
        vendored_egui.require(not relative.is_absolute() and '..' not in relative.parts,
                             'unsafe workspace member path')
        manifest = root / relative / 'Cargo.toml'
        vendored_egui.require(manifest not in identities and not manifest.is_symlink()
                             and manifest.resolve(strict=True) == manifest,
                             'duplicate or symlinked workspace manifest')
        package = tomllib.loads(manifest.read_text(encoding='utf-8')).get('package', {})
        version = package.get('version')
        if version == {'workspace': True}:
            version = workspace.get('package', {}).get('version')
        vendored_egui.require(isinstance(package.get('name'), str) and isinstance(version, str),
                             'invalid workspace package identity')
        identities[manifest] = (package['name'], version)
    return identities


def dependency_packages(root, metadata):
    """Only Cargo workspace members and the exact reviewed egui path may be local."""
    dependencies = []
    local_ids = set()
    local_manifests = set()
    workspace_ids = set(metadata.get('workspace_members', []))
    cargo = tomllib.loads((root / 'Cargo.toml').read_text(encoding='utf-8'))
    members = workspace_identities(root, cargo)
    expects_vendor = vendored_egui.patch_selected(cargo)
    vendored = False
    egui = [p for p in metadata['packages'] if p['name'] == vendored_egui.NAME]
    if expects_vendor or any(p['source'] is None for p in egui):
        vendored_egui.require(len(egui) == 1, 'expected exactly one logical egui dependency')
    for package in metadata['packages']:
        if package['source'] is not None:
            dependencies.append(package)
            continue
        manifest = Path(package.get('manifest_path', ''))
        expected = root / vendored_egui.ROOT / 'Cargo.toml'
        if package['name'] == vendored_egui.NAME or manifest == expected:
            vendored_egui.require(
                expects_vendor and package['name'] == vendored_egui.NAME
                and package['version'] == vendored_egui.VERSION
                and manifest == expected and not manifest.is_symlink()
                and manifest.resolve(strict=True) == expected
                and package.get('license') == 'MIT OR Apache-2.0'
                and package.get('license_file') is None
                and package.get('repository') == 'https://github.com/emilk/egui',
                'unapproved local package identity/path/version')
            vendored_egui.filesystem_bytes(root)
            dependencies.append(package)
            vendored = True
        else:
            package_id = package.get('id')
            vendored_egui.require(package_id in workspace_ids and package_id not in local_ids
                                 and manifest in members and manifest not in local_manifests
                                 and members[manifest] == (package['name'], package['version']),
                                 'unknown, duplicate or mismatched local workspace package')
            local_ids.add(package_id)
            local_manifests.add(manifest)
    vendored_egui.require(not expects_vendor or vendored,
                         'Cargo metadata omitted the selected local egui')
    return sorted(dependencies, key=lambda p: (p['name'], p['version'])), vendored


def collect(root, metadata):
    packages, vendored = dependency_packages(root, metadata)
    # Fail before creating output if the pinned data notice needs fresh review.
    unicode_notice = reviewed_unicode_notice(root, packages)
    out = root / 'third-party-licenses'
    out.mkdir(exist_ok=True)
    description = ('Generated from the exact Cargo.lock dependency graph. Includes target-specific '
                   'and optional packages; not all are linked on every platform. Each dependency '
                   'retains its upstream license. Font notices from epaint_default_fonts are included. ')
    description += ('The source tree vendors the pinned egui 0.31.1 crate described below; no '
                    'language-server distribution or JDK is vendored.' if vendored else
                    'No third-party source code, server distribution, or JDK is vendored here.')
    lines = ['# Third-party dependencies', '', description, '',
             '| Package | Version | License expression | Repository |', '|---|---|---|---|']
    for package in packages:
        lines.append('| {name} | {version} | {license} | {repository} |'.format(**{
            **package, 'license': package.get('license') or 'See bundled license file',
            'repository': package.get('repository') or ''}))
        base = Path(package['manifest_path']).parent
        dest = out / (package['name'] + '-' + package['version'])
        files = {item for item in base.iterdir() if item.is_file()
                 and any(item.name.upper().startswith(prefix)
                         for prefix in ('LICENSE', 'LICENCE', 'COPYING', 'NOTICE', 'AUTHORS'))}
        # Provenance is source metadata, not an additional package license notice.
        if package['source'] is None:
            files = {base / name for name in vendored_egui.LICENSE_HASHES}
        if package.get('license_file'):
            files.add(base / package['license_file'])
        if package['name'] == 'epaint_default_fonts':
            files.update((base / 'fonts').glob('*.txt'))
        for item in sorted(files):
            if item.is_file():
                dest.mkdir(exist_ok=True)
                shutil.copyfile(item, dest / item.name)
    if unicode_notice is not None:
        destination, content = unicode_notice
        destination.parent.mkdir(exist_ok=True)
        destination.write_bytes(content)
        lines += ['', UNICODE_NOTICE_SECTION.rstrip(), '']
    if vendored:
        lines += ['', vendored_egui.NOTICE_SECTION.rstrip(), '']
    lines += ['', '## Apache Maven Surefire report fixture', '',
              'The unchanged Surefire 3.5.4 XML regression fixture under',
              '`crates/app/tests/fixtures/surefire-3.5.4/` is licensed under Apache-2.0.',
              'See its provenance README and `third-party-licenses/apache-maven-surefire-3.5.4/LICENSE`.',
              'Only text fixture data is included; no Surefire/JUnit executable is redistributed.', '']
    fonts = root / 'crates/app/assets/fonts'
    manifest = json.loads((fonts / 'manifest.json').read_text())
    lines += ['', '## Embedded fonts', '',
              'Unmodified Regular faces embedded by Cedar, licensed under SIL Open Font License 1.1. OS CJK fallback fonts are not redistributed.', '']
    for asset in manifest['assets']:
        lines += [f"### {asset['family']}", '',
                  f"Upstream release: {asset['upstream_release']}; actual face version: {asset['font_version']}",
                  f"Source: {asset['download_url']}", f"SHA-256: {asset['sha256']}", '',
                  (fonts / asset['license_file']).read_text(), '']
    (root / 'THIRD_PARTY_NOTICES.md').write_text(
        '\n'.join(line.rstrip() for line in '\n'.join(lines).splitlines()).rstrip() + '\n')
    return len(packages)


def main():
    root = Path(__file__).resolve().parent.parent
    metadata = json.loads(subprocess.check_output(
        ['cargo', 'metadata', '--format-version', '1', '--locked', '--offline'], cwd=root))
    print(f'Collected notices for {collect(root, metadata)} locked packages')


if __name__ == '__main__':
    main()
