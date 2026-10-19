"""Bounded engineering material validation; never a legal or publication approval."""
import hashlib
import json
import os
from pathlib import Path
import re
import stat
import tomllib

REVIEW_PATH = 'scripts/license_materials_review.json'
COVERAGE_PATH = 'LICENSE-MATERIAL-COVERAGE.json'
README_PATH = 'LICENSE-MATERIALS-README.md'
NOTICE_PATH = 'THIRD_PARTY_NOTICES.md'
MARKER_PATH = '.license-materials-transaction.json'
MAX_JSON_BYTES = 2 * 1024 * 1024
MAX_JSON_DEPTH = 16
MAX_JSON_NODES = 150000
MAX_MATERIAL_BYTES = 2 * 1024 * 1024
MAX_MATERIALS = 1600
MAX_PACKAGES = 1000
MAX_MATERIAL_TOTAL = 16 * 1024 * 1024
SCOPES = ('linux-agent', 'linux-desktop', 'windows-agent', 'windows-desktop')
HASH = re.compile(r'[0-9a-f]{64}\Z')
COMMIT = re.compile(r'[0-9a-f]{40}\Z')
COMPONENT = re.compile(r'[A-Za-z0-9._+-]+\Z')
DEVICE = re.compile(r'(CON|PRN|AUX|NUL|COM[1-9]|LPT[1-9])(?:\.|$)', re.I)


class MaterialError(ValueError):
    """Material evidence is missing, inconsistent, unsafe, or out of bounds."""


def require(condition, message):
    if not condition:
        raise MaterialError(message)


def keys(value, expected, label):
    require(type(value) is dict and set(value) == set(expected), f'Invalid {label} fields')


def digest(data):
    return hashlib.sha256(data).hexdigest()


def hash_value(value, label):
    require(type(value) is str and HASH.fullmatch(value), f'Invalid {label} SHA-256')


def text(value, label, maximum=4096, empty=False):
    require(type(value) is str and (empty or value) and len(value.encode('utf-8')) <= maximum
            and '\x00' not in value, f'Invalid {label} text')


def relative(value, output=False):
    text(value, 'path', 220 if output else 1024)
    parts = value.split('/')
    require(1 <= len(parts) <= 32 and all(
        COMPONENT.fullmatch(part) and part not in ('.', '..') and not part.endswith('.')
        and not DEVICE.match(part) for part in parts), f'Unsafe material path: {value!r}')
    if output:
        require(all(part[0].isalnum() for part in parts), f'Unsafe output path: {value!r}')
    return value


def unique_sorted(values, label):
    require(type(values) is list and all(type(x) is str for x in values)
            and values == sorted(set(values)), f'Invalid sorted {label}')


def duplicate_keys(pairs):
    value = {}
    for key, item in pairs:
        require(key not in value, f'Duplicate JSON key: {key}')
        value[key] = item
    return value


def bounded_shape(value):
    pending = [(value, 0)]
    scheduled = 1
    while pending:
        item, depth = pending.pop()
        require(depth <= MAX_JSON_DEPTH, 'JSON nesting exceeds material limit')
        if type(item) in (dict, list):
            children = len(item) * 2 if type(item) is dict else len(item)
            scheduled += children
            require(scheduled <= MAX_JSON_NODES, 'JSON node count exceeds material limit')
            if type(item) is dict:
                for key, child in item.items():
                    require(type(key) is str and len(key) <= 256, 'Invalid JSON field name')
                    pending.append((key, depth + 1))
                    pending.append((child, depth + 1))
            else:
                pending.extend((child, depth + 1) for child in item)
        elif type(item) is str:
            try:
                require(len(item.encode('utf-8')) <= 65536, 'JSON string exceeds material limit')
            except UnicodeError as error:
                raise MaterialError('Invalid Unicode scalar in material JSON') from error
        elif type(item) is int:
            require(-(2 ** 63) <= item < 2 ** 63, 'JSON integer exceeds material limit')
        else:
            require(item is None or type(item) is bool, 'Unsupported JSON scalar')


def canonical(value):
    bounded_shape(value)
    encoded = bytearray()
    encoder = json.JSONEncoder(ensure_ascii=False, sort_keys=True, indent=2)
    for part in encoder.iterencode(value):
        raw = part.encode('utf-8')
        require(len(encoded) + len(raw) < MAX_JSON_BYTES, 'Canonical material JSON exceeds byte limit')
        encoded.extend(raw)
    encoded.extend(b'\n')
    return bytes(encoded)


def parse(data, label):
    require(type(data) is bytes and 0 < len(data) <= MAX_JSON_BYTES, f'Oversized {label}')
    try:
        result = json.loads(data.decode('utf-8'), object_pairs_hook=duplicate_keys,
                            parse_constant=lambda value: (_ for _ in ()).throw(MaterialError('Non-finite JSON')))
    except MaterialError:
        raise
    except (UnicodeError, ValueError, RecursionError) as error:
        raise MaterialError(f'Invalid {label} JSON') from error
    require(data == canonical(result), f'Noncanonical {label} JSON')
    return result


def regular_bytes(path, limit=MAX_MATERIAL_BYTES):
    """Read without following links, checking the same identity before and after."""
    path = Path(os.path.abspath(path))
    def checked():
        for component in [*reversed(path.parents), path]:
            info = component.lstat()
            require(not stat.S_ISLNK(info.st_mode) and not (getattr(info, 'st_file_attributes', 0) & 0x400),
                    f'Linked material input: {component}')
            require(stat.S_ISREG(info.st_mode) if component == path else stat.S_ISDIR(info.st_mode),
                    f'Invalid material input type: {component}')
        return path.lstat()
    def identity(info):
        return info.st_dev, info.st_ino, stat.S_IFMT(info.st_mode), info.st_size, info.st_mtime_ns
    before = checked()
    require(0 < before.st_size <= limit, f'Material input exceeds limit: {path}')
    descriptor = os.open(path, os.O_RDONLY | getattr(os, 'O_BINARY', 0)
                         | getattr(os, 'O_NOFOLLOW', 0) | getattr(os, 'O_NONBLOCK', 0))
    with os.fdopen(descriptor, 'rb') as source:
        opened = os.fstat(source.fileno())
        require(identity(opened) == identity(before), f'Material input changed: {path}')
        data = source.read(limit + 1)
        after = os.fstat(source.fileno())
    require(identity(before) == identity(after) == identity(checked()) and len(data) == before.st_size,
            f'Material input changed while reading: {path}')
    return data


def validate_origin(origin, package_ids):
    keys(origin, ('kind', 'package', 'path', 'bytes', 'sha256', 'start', 'end',
                  'repository', 'commit', 'blob', 'stored_path'), 'origin')
    require(origin['kind'] in ('archive', 'upstream', 'source'), 'Unknown material origin')
    relative(origin['path'])
    require(type(origin['bytes']) is int and 0 < origin['bytes'] <= 16 * 1024 * 1024,
            'Invalid original-source byte count')
    hash_value(origin['sha256'], 'original source')
    start, end = origin['start'], origin['end']
    require(type(start) is int and type(end) is int and 0 <= start < end <= origin['bytes']
            and end - start <= MAX_MATERIAL_BYTES, 'Invalid original byte range')
    if origin['kind'] == 'archive':
        require(type(origin['package']) is str and origin['package'] in package_ids and all(origin[x] is None for x in
                ('repository', 'commit', 'blob', 'stored_path')), 'Invalid archive origin')
    elif origin['kind'] == 'upstream':
        require(type(origin['package']) is str and origin['package'] in package_ids, 'Unbound upstream package')
        text(origin['repository'], 'upstream repository')
        require(origin['repository'].startswith('https://') and type(origin['commit']) is str
                and COMMIT.fullmatch(origin['commit']) and type(origin['blob']) is str
                and COMMIT.fullmatch(origin['blob']), 'Unpinned upstream origin')
        relative(origin['stored_path'])
        require(origin['stored_path'].startswith('scripts/license-material-sources/'),
                'Upstream bytes are outside retained input namespace')
    else:
        require(origin['package'] in (None, 'egui@0.31.1'), 'Invalid source origin package')
        provenance = [origin[key] for key in ('repository', 'commit', 'blob')]
        if any(value is not None for value in provenance):
            # An imported, already-reviewed fixture material is pinned to its
            # actual Cedar source revision, not claimed as upstream license proof.
            require(origin['package'] is None
                    and origin['repository'] == 'https://github.com/LLLLimbo/cedar-ide'
                    and type(origin['commit']) is str and COMMIT.fullmatch(origin['commit'])
                    and type(origin['blob']) is str and COMMIT.fullmatch(origin['blob'])
                    and origin['stored_path'].startswith('scripts/license-material-sources/fixtures/'),
                    'Unpinned imported fixture source')
        relative(origin['stored_path'])
    return origin


def _validate_review(review):
    keys(review, ('schema_version', 'kind', 'lock_sha256', 'source_pins', 'packages', 'materials',
                  'discovery', 'scopes', 'policy_issues', 'release_hold', 'notices_sha256'), 'review')
    require(type(review['schema_version']) is int and review['schema_version'] == 1
            and review['kind'] == 'cedar-license-material-review', 'Unsupported review schema')
    hash_value(review['lock_sha256'], 'Cargo.lock')
    hash_value(review['notices_sha256'], 'notices')
    pins = review['source_pins']
    require(type(pins) is list and 1 <= len(pins) <= 512, 'Invalid source pin count')
    for pin in pins:
        keys(pin, ('path', 'bytes', 'sha256'), 'source pin')
        relative(pin['path'])
        require(pin['path'] not in (REVIEW_PATH, COVERAGE_PATH, README_PATH, NOTICE_PATH)
                and not pin['path'].startswith('third-party-licenses/'), 'Cyclic/output source pin')
        require(type(pin['bytes']) is int and 0 < pin['bytes'] <= 16 * 1024 * 1024, 'Invalid source pin size')
        hash_value(pin['sha256'], 'source pin')
    unique_sorted([pin['path'] for pin in pins], 'source pins')
    pin_paths = {pin['path'] for pin in pins}
    require(len({path.casefold() for path in pin_paths}) == len(pin_paths), 'Case-colliding source pins')
    require({'Cargo.lock', 'Cargo.toml', '.github/workflows/ci.yml'} <= pin_paths,
            'Missing graph-defining root source pins')
    require(any(pin['path'] == 'Cargo.lock' and pin['sha256'] == review['lock_sha256'] for pin in pins),
            'Cargo.lock is not bound by a source pin')
    packages = review['packages']
    require(type(packages) is list and 1 <= len(packages) <= MAX_PACKAGES, 'Invalid package count')
    ids = []
    for package in packages:
        keys(package, ('id', 'name', 'version', 'source', 'checksum', 'license', 'license_file',
                       'repository', 'manifest_sha256', 'vcs_commit', 'state', 'bindings',
                       'unresolved_reason'), 'package')
        for key in ('name', 'version'):
            relative(package[key])
            require('/' not in package[key], 'Invalid package component')
        require(package['id'] == package['name'] + '@' + package['version'], 'Invalid package identity')
        ids.append(package['id'])
        if package['source'] is None:
            require(package['name'] == 'egui' and package['version'] == '0.31.1'
                    and package['checksum'] is None, 'Unreviewed local dependency')
        else:
            require(package['source'] == 'registry+https://github.com/rust-lang/crates.io-index',
                    'Unreviewed dependency source')
            hash_value(package['checksum'], 'archive')
        for key in ('license', 'license_file', 'repository'):
            if package[key] is not None:
                text(package[key], key)
        if package['license_file'] is not None:
            relative(package['license_file'])
        hash_value(package['manifest_sha256'], 'package manifest')
        require(package['vcs_commit'] is None or (type(package['vcs_commit']) is str
                and COMMIT.fullmatch(package['vcs_commit'])), 'Invalid package VCS commit')
        require(package['state'] in ('direct', 'shared', 'unresolved'), 'Unknown material state')
        require(type(package['bindings']) is list and len(package['bindings']) <= 128, 'Invalid bindings')
        if package['state'] == 'unresolved':
            text(package['unresolved_reason'], 'unresolved reason')
        else:
            require(package['bindings'] and package['unresolved_reason'] is None, 'False material coverage')
    unique_sorted(ids, 'package identities')
    package_ids = set(ids)
    materials = review['materials']
    require(type(materials) is list and 1 <= len(materials) <= MAX_MATERIALS, 'Invalid material count')
    paths, total = [], 0
    for material in materials:
        keys(material, ('path', 'bytes', 'sha256', 'origins'), 'material')
        path = relative(material['path'], output=True)
        require(path.startswith('third-party-licenses/') and len(path.split('/')) >= 3, 'Invalid material namespace')
        paths.append(path)
        require(type(material['bytes']) is int and 0 < material['bytes'] <= MAX_MATERIAL_BYTES, 'Invalid material size')
        total += material['bytes']
        hash_value(material['sha256'], 'material')
        require(type(material['origins']) is list and 1 <= len(material['origins']) <= 64, 'Invalid origins')
        for origin in material['origins']:
            validate_origin(origin, package_ids)
            require(origin['end'] - origin['start'] == material['bytes'], 'Origin size does not match material')
            if origin['start'] == 0 and origin['end'] == origin['bytes']:
                require(origin['sha256'] == material['sha256'], 'Full original hash differs from material')
    unique_sorted(paths, 'material paths')
    require(len({x.casefold() for x in paths}) == len(paths), 'Case-colliding material outputs')
    folded = {x.casefold() for x in paths}
    require(not any('/'.join(path.casefold().split('/')[:n]) in folded
                    for path in paths for n in range(1, len(path.split('/')))),
            'File/directory material path collision')
    require(total <= MAX_MATERIAL_TOTAL, 'Material inventory exceeds total byte budget')
    by_path = {material['path']: material for material in materials}
    for package in packages:
        seen = []
        for binding in package['bindings']:
            keys(binding, ('material', 'origin'), 'membership binding')
            require(type(binding['material']) is str and binding['material'] in by_path, 'Membership names an unknown material')
            origin = validate_origin(binding['origin'], package_ids)
            require(origin['package'] == package['id'], 'Membership is not bound to its package')
            require(origin in by_path[binding['material']]['origins'], 'Membership has no matching material origin')
            if origin['kind'] == 'upstream':
                require(origin['commit'] == package['vcs_commit'], 'Upstream membership commit differs from package VCS')
            require(origin['end'] - origin['start'] == by_path[binding['material']]['bytes'], 'Membership size mismatch')
            seen.append(binding['material'])
        unique_sorted(seen, 'membership material paths')
    discovery = review['discovery']
    require(type(discovery) is list and len(discovery) <= 4096, 'Invalid discovery inventory')
    discovered = []
    for entry in discovery:
        keys(entry, ('package', 'path', 'bytes', 'sha256', 'decision', 'material', 'reason'), 'discovery entry')
        require(type(entry['package']) is str and entry['package'] in package_ids, 'Unknown discovered package')
        relative(entry['path'])
        require(type(entry['bytes']) is int and 0 <= entry['bytes'] <= 16 * 1024 * 1024, 'Invalid discovered size')
        hash_value(entry['sha256'], 'discovered source')
        require(entry['decision'] in ('retain', 'excluded', 'unresolved'), 'Unreviewed discovery decision')
        if entry['decision'] == 'retain':
            require(type(entry['material']) is str and entry['material'] in by_path
                    and entry['reason'] is None, 'Invalid retained discovery')
            require(any(origin['kind'] in ('archive', 'source')
                        and origin['package'] == entry['package'] and origin['path'] == entry['path']
                        and origin['bytes'] == entry['bytes'] and origin['sha256'] == entry['sha256']
                        for origin in by_path[entry['material']]['origins']),
                    'Discovery has no matching retained source origin')
        else:
            require(entry['material'] is None, 'Uncovered discovery has an output')
            text(entry['reason'], 'discovery decision reason')
        discovered.append(entry['package'] + '/' + entry['path'])
    unique_sorted(discovered, 'discovered candidates')
    scopes = review['scopes']
    require(type(scopes) is list and all(type(x) is dict for x in scopes)
            and [x.get('id') for x in scopes] == list(SCOPES), 'Invalid distribution scopes')
    for scope in scopes:
        keys(scope, ('id', 'target', 'roots', 'invocation', 'graph_hashes', 'packages'), 'scope')
        require(scope['target'] == ('x86_64-unknown-linux-gnu' if scope['id'].startswith('linux')
                else 'x86_64-pc-windows-msvc'), 'Invalid scope target')
        unique_sorted(scope['roots'], 'scope roots')
        require(scope['roots'] == (['cedar-agent'] if scope['id'].endswith('agent')
                else ['cedar-agent', 'cedar-app']), 'Invalid shipping roots')
        require(type(scope['invocation']) is list and scope['invocation'], 'Missing graph invocation')
        for argument in scope['invocation']:
            text(argument, 'graph argument')
        keys(scope['graph_hashes'], ('full', 'normal', 'runtime'), 'graph hashes')
        for value in scope['graph_hashes'].values():
            hash_value(value, 'graph evidence')
        require(type(scope['packages']) is list and all(type(x) is dict for x in scope['packages'])
                and [x.get('package') for x in scope['packages']] == ids,
                'Scope does not classify every reviewed package exactly once')
        for entry in scope['packages']:
            keys(entry, ('package', 'role', 'features', 'witness', 'reason'), 'scope package')
            require(entry['role'] in ('normal', 'proc-macro', 'build', 'excluded'), 'Unknown scope role')
            unique_sorted(entry['features'], 'selected features')
            require(type(entry['witness']) is list and all(type(x) is str for x in entry['witness']), 'Invalid scope witness')
            if entry['role'] == 'excluded':
                text(entry['reason'], 'scope exclusion reason')
                require(not entry['features'], 'Excluded package has selected features')
            else:
                require(entry['reason'] is None and entry['witness']
                        and entry['witness'][-1] == entry['package'], 'Missing selected path witness')
    require(type(review['policy_issues']) is list and len(review['policy_issues']) <= 256, 'Invalid policy issue count')
    issue_ids = []
    for issue in review['policy_issues']:
        keys(issue, ('id', 'status', 'scopes', 'description', 'evidence'), 'policy issue')
        text(issue['id'], 'issue id'); issue_ids.append(issue['id'])
        require(issue['status'] in ('pending', 'reviewed'), 'Unknown review status')
        unique_sorted(issue['scopes'], 'issue scopes')
        require(issue['scopes'] and set(issue['scopes']) <= set(SCOPES), 'Invalid issue scope')
        text(issue['description'], 'policy issue description')
        require(type(issue['evidence']) is list and issue['evidence'], 'Missing policy evidence')
        for evidence in issue['evidence']:
            text(evidence, 'policy evidence')
    unique_sorted(issue_ids, 'policy issues')
    keys(review['release_hold'], ('active', 'reason'), 'release hold')
    require(type(review['release_hold']['active']) is bool, 'Invalid release hold')
    text(review['release_hold']['reason'], 'release-hold reason')
    return review


def validate_review(review):
    try:
        bounded_shape(review)
        return _validate_review(review)
    except (TypeError, AttributeError, KeyError, OverflowError, UnicodeError, RecursionError) as error:
        raise MaterialError('Malformed reviewed material schema') from error


def load_review(root=None):
    root = Path(root) if root is not None else Path(__file__).absolute().parent.parent
    raw = regular_bytes(root / REVIEW_PATH, MAX_JSON_BYTES)
    return validate_review(parse(raw, 'trusted review')), raw


def material_inventory(review):
    return [{key: item[key] for key in ('path', 'bytes', 'sha256')} for item in review['materials']]


def distribution_status(review, scope_id):
    require(scope_id in SCOPES, 'Unknown distribution scope')
    scope = next(item for item in review['scopes'] if item['id'] == scope_id)
    selected = {item['package'] for item in scope['packages'] if item['role'] != 'excluded'}
    reasons = []
    for package in review['packages']:
        if package['id'] in selected and package['state'] == 'unresolved':
            reasons.append('unresolved-material:' + package['id'])
    for entry in review['discovery']:
        if entry['package'] in selected and entry['decision'] == 'unresolved':
            reasons.append('unresolved-notice:' + entry['package'] + '/' + entry['path'])
    for issue in review['policy_issues']:
        if scope_id in issue['scopes'] and issue['status'] == 'pending':
            reasons.append('policy-review:' + issue['id'])
    if review['release_hold']['active']:
        reasons.append('release-hold:' + review['release_hold']['reason'])
    return {'scope': scope_id, 'distribution_ready': not reasons, 'blocking_reasons': sorted(reasons)}


def coverage_bytes(review, review_raw):
    return canonical({'schema_version': 1, 'kind': 'cedar-license-material-coverage',
        'review_sha256': digest(review_raw), 'lock_sha256': review['lock_sha256'],
        'source_pins': review['source_pins'],
        'material_inventory': material_inventory(review),
        'packages': [{'id': p['id'], 'state': p['state'],
                      'material_paths': [binding['material'] for binding in p['bindings']],
                      'unresolved_reason': p['unresolved_reason']} for p in review['packages']],
        'scopes': review['scopes'], 'policy_issues': review['policy_issues'],
        'release_hold': review['release_hold'],
        'distribution_status': [distribution_status(review, scope) for scope in SCOPES]})


def readme_bytes(review):
    lines = ['# License-material status', '',
        'This development bundle is an engineering diagnostic artifact.',
        'Material integrity success is not distribution approval or a legal compliance finding.', '',
        '## Distribution readiness', '']
    for scope in SCOPES:
        status = distribution_status(review, scope)
        lines += [scope + ': distribution_ready=' + str(status['distribution_ready']).lower()]
        lines += ['- ' + reason for reason in status['blocking_reasons']]
        lines += ['']
    lines += ['Before delivery, run the matching package helper with',
        '`verify --require-distribution-ready ARCHIVE`. A nonzero result blocks delivery.',
        'Ordinary build/verify and BUNDLE_VERIFICATION success cover engineering checks only.',
        'Historical packages without the mandatory material receipt do not imply readiness.', '',
        'The all-lock package inventory includes optional and other-target packages.',
        'An explicit scope exclusion does not supply missing license material.',
        'The trusted review records exact upstream bytes and shared membership; SPDX alone is not coverage.',
        'Wayland generated-input policy review is separate from retaining its original notices.', '',
        'See LICENSE-MATERIAL-COVERAGE.json and THIRD_PARTY_NOTICES.md for the exact inventory.', '']
    return '\n'.join(lines).encode('utf-8')


def validate_payload(payload, scope_id, review=None, review_raw=None):
    if review is None:
        review, review_raw = load_review()
    else:
        validate_review(review)
        require(review_raw == canonical(review), 'Unbound review bytes')
    expected = {item['path']: item for item in review['materials']}
    actual = {name for name in payload if name.startswith('third-party-licenses/')}
    require(actual == set(expected), 'Material payload inventory differs from trusted review')
    for name, item in expected.items():
        data = payload[name]
        require(type(data) is bytes and len(data) == item['bytes'] and digest(data) == item['sha256'],
                f'Material bytes differ from trusted review: {name}')
    require(payload.get(COVERAGE_PATH) == coverage_bytes(review, review_raw), 'Untrusted or missing material coverage receipt')
    require(payload.get(README_PATH) == readme_bytes(review), 'Missing or contradictory material-status README')
    require(type(payload.get(NOTICE_PATH)) is bytes and digest(payload[NOTICE_PATH]) == review['notices_sha256'],
            'Notices differ from trusted review')
    return distribution_status(review, scope_id)


def require_distribution_ready(status):
    require(status['distribution_ready'] is True, 'Distribution blocked: ' + '; '.join(status['blocking_reasons']))


def source_input_names(review):
    return {REVIEW_PATH, *(pin['path'] for pin in review['source_pins'])}


def validate_source_inputs(inputs, review, review_raw):
    validate_review(review)
    require(review_raw == canonical(review), 'Unbound source review bytes')
    require(set(inputs) == source_input_names(review), 'Source-only input inventory differs from review')
    require(inputs[REVIEW_PATH] == review_raw, 'Source checkout review differs from trusted tool review')
    for pin in review['source_pins']:
        data = inputs[pin['path']]
        require(type(data) is bytes and len(data) == pin['bytes'] and digest(data) == pin['sha256'],
                'Source-only input changed: ' + pin['path'])

    try:
        cargo = tomllib.loads(inputs['Cargo.toml'].decode('utf-8'))
        members = cargo.get('workspace', {}).get('members', [])
        require(type(members) is list and all(type(member) is str for member in members),
                'Invalid reviewed workspace members')
        expected = {relative(member + '/Cargo.toml') for member in members}
        patches = cargo.get('patch', {}).get('crates-io', {})
        require(type(patches) is dict, 'Invalid reviewed Cargo patches')
        for patch in patches.values():
            if type(patch) is dict and 'path' in patch:
                expected.add(relative(patch['path'] + '/Cargo.toml'))
        require(expected <= set(inputs), 'Missing graph-defining workspace/vendor manifest pins')
    except (UnicodeError, tomllib.TOMLDecodeError) as error:
        raise MaterialError('Invalid pinned Cargo root manifest') from error
