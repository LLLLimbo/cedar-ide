#!/usr/bin/env python3
"""Collect notices from Cargo's exact locked graph, including pinned local egui."""
import hashlib
import json
import io
import os
import re
import stat
import tarfile
import tempfile
from pathlib import Path
import shutil
import subprocess
import tomllib

import vendored_egui
import license_materials as materials


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


def render_notices(root, packages, vendored, unicode_notice, review):
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
    if unicode_notice is not None:
        lines += ['', UNICODE_NOTICE_SECTION.rstrip(), '']
    if vendored:
        lines += ['', vendored_egui.NOTICE_SECTION.rstrip(), '']
    lines += ['', '## Apache Maven Surefire report fixture', '',
              'The unchanged Surefire 3.5.4 XML regression fixture under',
              '`crates/app/tests/fixtures/surefire-3.5.4/` is licensed under Apache-2.0.',
              'See its provenance README and `third-party-licenses/apache-maven-surefire-3.5.4/LICENSE`.',
              'Only text fixture data is included; no Surefire/JUnit executable is redistributed.', '']
    fonts = root / 'crates/app/assets/fonts'
    manifest = json.loads(materials.regular_bytes(fonts / 'manifest.json'))
    materials.require(type(manifest.get('assets')) is list and len(manifest['assets']) <= 32, 'Invalid font manifest')
    lines += ['', '## Embedded fonts', '',
              'Unmodified Regular faces embedded by Cedar, licensed under SIL Open Font License 1.1. OS CJK fallback fonts are not redistributed.', '']
    for asset in manifest['assets']:
        materials.relative(asset['license_file'])
        lines += [f"### {asset['family']}", '',
                  f"Upstream release: {asset['upstream_release']}; actual face version: {asset['font_version']}",
                  f"Source: {asset['download_url']}", f"SHA-256: {asset['sha256']}", '',
                  materials.regular_bytes(fonts / asset['license_file']).decode('utf-8'), '']
    rendered = '\n'.join(line.rstrip() for line in '\n'.join(lines).splitlines()).rstrip() + '\n'
    heading = '# Third-party dependencies\n\n'
    materials.require(rendered.startswith(heading), 'Unexpected notice heading')
    return (heading + materials.notice_status_text(review) + rendered[len(heading):]).encode('utf-8')

# This file is an external draft until explicitly integrated.
MAX_ARCHIVE_COMPRESSED = 16 * 1024 * 1024
MAX_ARCHIVE_EXPANDED = 256 * 1024 * 1024
MAX_ARCHIVE_TOTAL = 1024 * 1024 * 1024
MAX_PACKAGE_FILES = 8192
MAX_SOURCE_BYTES = 16 * 1024 * 1024


def candidate_path(package, name):
    """Discovery is not admission; every candidate needs an exact review entry."""
    parts = name.split('/')
    base = parts[-1]
    return (name == package.get('license_file')
            or (package['name'] == 'epaint_default_fonts' and len(parts) == 2
                and parts[0] == 'fonts' and base.endswith('.txt'))
            or any(base.upper().startswith(prefix)
                   for prefix in ('LICENSE', 'LICENCE', 'COPYING', 'NOTICE', 'AUTHORS'))
            or re.search(r'(^|[-_.])(licen[cs]e|copying|copyright|notice|authors|unlicen[cs]e|ofl)([-_.]|$)', base, re.I)
            or any(re.fullmatch(r'licen[cs]es?|notices?|copyrights?', part, re.I)
                   for part in parts[:-1]))


def inventory_path(name):
    materials.require(type(name) is str and 0 < len(name.encode('utf-8')) <= 1024
                      and 1 <= len(name.split('/')) <= 32
                      and all(part not in ('', '.', '..') for part in name.split('/'))
                      and not any(ord(char) < 32 or char == '\\' for char in name),
                      'Unsafe source inventory path')


def checked_tree(base, limit=MAX_PACKAGE_FILES):
    """Bounded no-link inventory; does not read ordinary source file contents."""
    base = Path(base)
    pending, files, directories = [base], {}, 0
    while pending:
        directory = pending.pop()
        info = directory.lstat()
        materials.require(stat.S_ISDIR(info.st_mode) and not (getattr(info, 'st_file_attributes', 0) & 0x400),
                          'Unsafe material directory: ' + str(directory))
        directories += 1
        materials.require(directories <= limit, 'Material directory count exceeds limit')
        with os.scandir(directory) as entries:
            for entry in entries:
                path = Path(entry.path)
                rel = path.relative_to(base).as_posix()
                inventory_path(rel)
                info = entry.stat(follow_symlinks=False)
                materials.require(not stat.S_ISLNK(info.st_mode)
                                  and not (getattr(info, 'st_file_attributes', 0) & 0x400),
                                  'Linked material input: ' + str(path))
                if stat.S_ISDIR(info.st_mode):
                    pending.append(path)
                else:
                    materials.require(stat.S_ISREG(info.st_mode), 'Nonregular material input: ' + str(path))
                    materials.require(len(files) < limit, 'Material file count exceeds limit')
                    files[rel] = info.st_size
    return files


def archive_material_sources(package, reviewed, needed, total):
    manifest = Path(package['manifest_path'])
    base = manifest.parent
    ident = package['name'] + '-' + package['version']
    materials.require(manifest.is_absolute() and manifest.name == 'Cargo.toml'
                      and base.name == ident and base.parent.parent.name == 'src',
                      'Unexpected registry package path: ' + str(manifest))
    # Cargo registry/src/<index>/<crate> has a sibling registry/cache/<index>.
    archive = base.parent.parent.parent / 'cache' / base.parent.name / (ident + '.crate')
    raw = materials.regular_bytes(archive, MAX_ARCHIVE_COMPRESSED)
    materials.require(materials.digest(raw) == reviewed['checksum'], 'Registry archive checksum differs: ' + ident)
    cached = checked_tree(base)
    candidates = {name for name in cached if candidate_path(package, name)}
    needed = set(needed) | candidates | {'Cargo.toml', '.cargo_vcs_info.json'}
    extracted, archive_candidates, seen, expanded = {}, set(), set(), 0
    with tarfile.open(fileobj=io.BytesIO(raw), mode='r:gz') as source:
        for member in source:
            materials.require(member.name.startswith(ident + '/'), 'Archive member outside exact package root')
            name = member.name[len(ident) + 1:].rstrip('/')
            if not name and member.isdir():
                continue
            inventory_path(name)
            materials.require(name not in seen and len(seen) < MAX_PACKAGE_FILES,
                              'Duplicate or oversized archive member inventory')
            seen.add(name)
            materials.require(member.isfile() or member.isdir(), 'Linked or special archive member')
            if member.isdir():
                continue
            expanded += member.size
            materials.require(0 <= member.size <= MAX_ARCHIVE_EXPANDED and expanded <= MAX_ARCHIVE_EXPANDED
                              and total[0] + expanded <= MAX_ARCHIVE_TOTAL,
                              'Expanded registry archive exceeds limit')
            if candidate_path(package, name):
                archive_candidates.add(name)
                needed.add(name)
            if name in needed:
                materials.require(0 < member.size <= MAX_SOURCE_BYTES, 'Original material source exceeds byte limit')
                handle = source.extractfile(member)
                materials.require(handle is not None, 'Missing archive member content')
                with handle:
                    data = handle.read(MAX_SOURCE_BYTES + 1)
                materials.require(len(data) == member.size, 'Incomplete registry archive member')
                extracted[name] = data
    total[0] += expanded
    materials.require(total[0] <= MAX_ARCHIVE_TOTAL, 'Total expanded registry inputs exceed limit')
    materials.require(candidates == archive_candidates, 'Cached candidate inventory differs from checksum-pinned archive')
    for name, data in extracted.items():
        materials.require(name in cached and materials.regular_bytes(base / name, MAX_SOURCE_BYTES) == data,
                          'Cached original differs from checksum-pinned archive: ' + ident + '/' + name)
    materials.require('Cargo.toml' in extracted, 'Missing archived manifest')
    vcs = json.loads(extracted['.cargo_vcs_info.json']) if '.cargo_vcs_info.json' in extracted else {}
    materials.require(vcs.get('git', {}).get('sha1') == reviewed['vcs_commit'], 'Archive VCS revision differs from review')
    return extracted, archive_candidates


def validate_package_metadata(package, reviewed, manifest):
    materials.require(all(package.get(key) == reviewed[key] for key in
                          ('name', 'version', 'source', 'license', 'license_file', 'repository')),
                      'Cargo metadata identity differs from material review')
    materials.require(materials.digest(manifest) == reviewed['manifest_sha256'], 'Package manifest hash differs from review')
    parsed = tomllib.loads(manifest.decode('utf-8'))['package']
    materials.require(all(parsed.get(key) == reviewed[key] for key in ('name', 'version', 'license', 'repository'))
                      and parsed.get('license-file') == reviewed['license_file'],
                      'Archived package declarations differ from review')


def prepare_materials(root, packages, review, review_raw):
    pins = materials.source_input_names(review)
    inputs = {name: materials.regular_bytes(root / name, MAX_SOURCE_BYTES) for name in pins}
    materials.validate_source_inputs(inputs, review, review_raw)
    locked = tomllib.loads(inputs['Cargo.lock'].decode('utf-8'))['package']
    registry = {(item['name'], item['version']): item for item in locked if item.get('source')}
    expected_registry = {(item['name'], item['version']): item for item in review['packages'] if item['source']}
    materials.require(set(registry) == set(expected_registry), 'Review does not cover the exact locked registry inventory')
    for key, item in expected_registry.items():
        materials.require(registry[key]['source'] == item['source'] and registry[key]['checksum'] == item['checksum'],
                          'Locked source/checksum differs from review')
    by_id = {item['id']: item for item in review['packages']}
    actual_ids = [package['name'] + '@' + package['version'] for package in packages]
    materials.require(len(actual_ids) == len(set(actual_ids)) and set(actual_ids) == set(by_id),
                      'Cargo dependency identities differ from review')
    origins = [origin for item in review['materials'] for origin in item['origins']]
    originals, discovered, total = {}, {}, [0]
    for package in packages:
        identity = package['name'] + '@' + package['version']
        reviewed = by_id[identity]
        needed = {o['path'] for o in origins if o['kind'] == 'archive' and o['package'] == identity}
        if package['source'] is None:
            base = root / vendored_egui.ROOT
            original = {name: materials.regular_bytes(base / name, MAX_SOURCE_BYTES)
                        for name in ('Cargo.toml', *vendored_egui.LICENSE_HASHES)}
            candidate_names = set(vendored_egui.LICENSE_HASHES)
        else:
            original, candidate_names = archive_material_sources(package, reviewed, needed, total)
        validate_package_metadata(package, reviewed, original['Cargo.toml'])
        if package.get('license_file'):
            materials.relative(package['license_file'])
            materials.require(package['license_file'] in original, 'Missing declared material source')
        originals[identity] = original
        for name in candidate_names:
            data = original[name]
            discovered[identity + '/' + name] = (len(data), materials.digest(data))
    expected = {entry['package'] + '/' + entry['path']: (entry['bytes'], entry['sha256'])
                for entry in review['discovery']}
    materials.require(discovered == expected, 'New, missing, or changed unreviewed notice candidate')
    output = {}
    for item in review['materials']:
        value = None
        for origin in item['origins']:
            if origin['kind'] == 'archive':
                data = originals[origin['package']].get(origin['path'])
                materials.require(data is not None, 'Missing original archive source')
            else:
                data = materials.regular_bytes(root / origin['stored_path'], MAX_SOURCE_BYTES)
                if origin['blob'] is not None:
                    blob = hashlib.sha1(b'blob ' + str(len(data)).encode() + b'\0' + data).hexdigest()
                    materials.require(blob == origin['blob'], 'Retained original Git blob differs from review')
            materials.require(len(data) == origin['bytes'] and materials.digest(data) == origin['sha256'],
                              'Original source bytes differ from review')
            portion = data[origin['start']:origin['end']]
            materials.require(len(portion) == item['bytes'] and materials.digest(portion) == item['sha256'],
                              'Original source slice differs from material')
            materials.require(value is None or value == portion, 'Shared material origins disagree')
            value = portion
        output[item['path']] = value
    return output


def output_snapshot(root, plan, review):
    output = root / 'third-party-licenses'
    current = {}
    if os.path.lexists(output):
        files = checked_tree(output, materials.MAX_MATERIALS)
        allowed_directories = {'/'.join(name.split('/')[:index])
                               for name in plan if name.startswith('third-party-licenses/')
                               for index in range(1, len(name.split('/')))}
        actual_directories = {path.relative_to(root).as_posix() for path in output.rglob('*') if path.is_dir()}
        materials.require(actual_directories <= allowed_directories, 'Stale or unknown material directory')
        expected = {name.removeprefix('third-party-licenses/'): data for name, data in plan.items()
                    if name.startswith('third-party-licenses/')}
        materials.require(set(files) <= set(expected), 'Stale or unknown material output; explicit recovery required')
        for name in files:
            data = materials.regular_bytes(output / name)
            materials.require(data == expected[name], 'Existing material bytes differ; explicit recovery required')
            current['third-party-licenses/' + name] = data
    for name in (materials.NOTICE_PATH, materials.COVERAGE_PATH, materials.README_PATH):
        if os.path.lexists(root / name):
            current[name] = materials.regular_bytes(root / name, materials.MAX_JSON_BYTES)
    if materials.README_PATH in current:
        materials.require(current[materials.README_PATH] == plan[materials.README_PATH], 'Unexpected existing material-status README')
    if materials.NOTICE_PATH in current:
        notice = current[materials.NOTICE_PATH]
        materials.require(notice == plan[materials.NOTICE_PATH]
                          or (materials.COVERAGE_PATH not in current and review['legacy_notices_sha256'] is not None
                              and materials.digest(notice) == review['legacy_notices_sha256']),
                          'Unreviewed legacy notices cannot be overwritten')
    if materials.COVERAGE_PATH in current:
        materials.require(current == plan, 'Incomplete or stale material generation; explicit recovery required')
    return current


def publish_plan(root, plan, review):
    """Preflight then staged writes with rollback, not multi-path crash atomicity."""
    marker = root / materials.MARKER_PATH
    materials.require(not os.path.lexists(marker), 'Incomplete prior material transaction requires recovery')
    before = output_snapshot(root, plan, review)
    if before == plan:
        return
    stage = Path(tempfile.mkdtemp(prefix='.license-materials-stage-', dir=root))
    changed, created_dirs = [], []
    owned_marker = None
    preserve_stage = False
    marker_bytes = materials.canonical({'schema_version': 1, 'stage': stage.name,
                                        'state': 'incomplete-material-generation'})
    def release_marker():
        info = marker.lstat()
        materials.require(owned_marker == (info.st_dev, info.st_ino)
                          and materials.regular_bytes(marker) == marker_bytes,
                          'Recovery marker ownership changed; explicit recovery required')
        marker.unlink()
    try:
        for name, data in plan.items():
            path = stage / 'new' / name
            path.parent.mkdir(parents=True, exist_ok=True)
            path.write_bytes(data)
        for name, data in before.items():
            path = stage / 'old' / name
            path.parent.mkdir(parents=True, exist_ok=True)
            path.write_bytes(data)
        materials.require(output_snapshot(root, plan, review) == before, 'Output changed during preflight')
        descriptor = os.open(marker, os.O_WRONLY | os.O_CREAT | os.O_EXCL
                             | getattr(os, 'O_BINARY', 0) | getattr(os, 'O_NOFOLLOW', 0), 0o600)
        with os.fdopen(descriptor, 'wb') as handle:
            info = os.fstat(handle.fileno())
            owned_marker = (info.st_dev, info.st_ino)
            handle.write(marker_bytes)
            handle.flush()
            os.fsync(handle.fileno())
        for name, data in plan.items():
            if before.get(name) == data:
                continue
            destination = root / name
            missing = []
            parent = destination.parent
            while parent != root and not parent.exists():
                missing.append(parent)
                parent = parent.parent
            for directory in reversed(missing):
                directory.mkdir()
                created_dirs.append(directory)
            # Inspect each actual destination chain immediately before rename.
            for parent in [destination.parent, *destination.parent.parents]:
                if parent == root.parent:
                    break
                info = parent.lstat()
                materials.require(stat.S_ISDIR(info.st_mode) and not (getattr(info, 'st_file_attributes', 0) & 0x400),
                                  'Unsafe output parent during material transaction')
            if os.path.lexists(destination):
                materials.require(name in before and materials.regular_bytes(destination, materials.MAX_JSON_BYTES) == before[name],
                                  'Output changed during material transaction')
            else:
                materials.require(name not in before, 'Output disappeared during material transaction')
            os.replace(stage / 'new' / name, destination)
            changed.append(name)
        materials.require(output_snapshot(root, plan, review) == plan, 'Published material generation is incomplete')
        release_marker()
        owned_marker = None
    except Exception as error:
        rollback_error = None
        try:
            for name in reversed(changed):
                materials.require(materials.regular_bytes(root / name, materials.MAX_JSON_BYTES) == plan[name],
                                  'Output ownership changed during rollback')
                if name in before:
                    os.replace(stage / 'old' / name, root / name)
                else:
                    (root / name).unlink()
            for directory in reversed(created_dirs):
                directory.rmdir()
            if owned_marker is not None:
                release_marker()
                owned_marker = None
        except (OSError, materials.MaterialError) as failure:
            rollback_error = failure
        if rollback_error is not None:
            preserve_stage = True
            raise materials.MaterialError('Material update and rollback failed; retained stage/marker require explicit recovery') from error
        raise
    finally:
        if not preserve_stage and owned_marker is None:
            shutil.rmtree(stage)


def collect(root, metadata):
    root = Path(os.path.abspath(root))
    try:
        materials.require(not os.path.lexists(root / materials.MARKER_PATH),
                          'Incomplete prior material transaction requires recovery')
        packages, vendored = dependency_packages(root, metadata)
        unicode_notice = reviewed_unicode_notice(root, packages)
        review, review_raw = materials.load_review(root)
        plan = prepare_materials(root, packages, review, review_raw)
        notices = render_notices(root, packages, vendored, unicode_notice, review)
        materials.require(materials.digest(notices) == review['notices_sha256'], 'Rendered notices differ from reviewed bytes')
        plan[materials.NOTICE_PATH] = notices
        plan[materials.COVERAGE_PATH] = materials.coverage_bytes(review, review_raw)
        plan[materials.README_PATH] = materials.readme_bytes(review)
        for scope in materials.SCOPES:
            materials.validate_payload(plan, scope, review, review_raw)
        publish_plan(root, plan, review)
        return len(packages)
    except (OSError, KeyError, TypeError, json.JSONDecodeError, tomllib.TOMLDecodeError,
            UnicodeError, tarfile.TarError) as error:
        raise materials.MaterialError('Material collection failed before a complete generation: ' + str(error)) from error

def main():
    root = Path(__file__).resolve().parent.parent
    metadata = json.loads(subprocess.check_output(
        ['cargo', 'metadata', '--format-version', '1', '--locked', '--offline'], cwd=root))
    count = collect(root, metadata)
    review, _ = materials.load_review(root)
    print(f'Collected notices for {count} locked packages; engineering inventory only')
    print(json.dumps([materials.distribution_status(review, scope) for scope in materials.SCOPES], sort_keys=True))


if __name__ == '__main__':
    main()
