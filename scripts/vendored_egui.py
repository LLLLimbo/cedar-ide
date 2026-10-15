"""Exact egui 0.31.1 source/license identity shared by collection and export.

This is a single reviewed path dependency, never a generic vendoring exemption.
Update the pinned provenance only after reviewing a replacement upstream delta.
"""
import hashlib
import json
from pathlib import Path, PurePosixPath

ROOT = 'vendor/egui-0.31.1'
NAME = 'egui'
VERSION = '0.31.1'
ARCHIVE_SHA256 = '25dd34cec49ab55d85ebf70139cb1ccd29c977ef6b6ba4fe85489d6877ee9ef3'
COMMIT = '1669e52a7ccfc3489c1b0999b9ed48894a0b3887'
LICENSE_HASHES = {
    'LICENSE-MIT': '95ca92f5f8ea5231f1580b3a2a799e8260af3114b900e1def5355a7f44bcf60c',
    'LICENSE-APACHE': '8173d5c29b4f956d532781d2b86e4e30f83e6b7878dce18c919451d6ba707c90',
}
ADDITIONS = {
    **LICENSE_HASHES,
    'vendor-upstream.json': 'eb66e075a8eb6f21ac6a8ec1bd076b27cbc1fc1a922ca98ce69bfe329a5be228',
    'license-provenance.json': '283925d428a2b402efb361636737ec1178161960685e22f95698bae40ae768e1',
    'egui-observation.patch': '57bea44014be09db590e211618359bbfaedfed08df8142aebde60eba783e1049',
    'CEDAR-NOTICE.md': 'c6b9ec4ba9b96a8f72567c422b2a3881da084833aff3ab6633892391daa41a75',
}
NOTICE_SECTION = '''## Vendored egui 0.31.1

The source tree vendors the complete official egui 0.31.1 crate with a narrow
Cedar IME branch-receipt observation patch, under MIT OR Apache-2.0. This is the
same egui dependency listed above, not a second package or a version upgrade.
Upstream tag: 0.31.1; archive VCS commit: 1669e52a7ccfc3489c1b0999b9ed48894a0b3887.
Archive SHA-256: 25dd34cec49ab55d85ebf70139cb1ccd29c977ef6b6ba4fe85489d6877ee9ef3.
The source export retains all 92 archive members, the exact three-file patch,
upstream member hashes, modification notice and official license provenance in
`vendor/egui-0.31.1/`. The two unmodified upstream license texts are also included
once under `third-party-licenses/egui-0.31.1/` in the existing bundle inventory.
No language-server distribution or JDK is vendored.
'''


def require(condition, message):
    if not condition:
        raise ValueError('Vendored egui: ' + message)


def patch_selected(cargo):
    """Historical manifests have no patch; any other egui patch fails closed."""
    patch = cargo.get('patch', {}).get('crates-io', {}).get(NAME)
    if patch is None:
        return False
    require(patch == {'path': ROOT}, 'unapproved Cargo patch identity/path')
    return True


def validated_bytes(paths, read):
    """Read exact relative vendor names through a caller-supplied trusted reader."""
    paths = set(paths)
    data = {}
    for name, expected in ADDITIONS.items():
        require(name in paths, 'missing approved file ' + name)
        data[name] = read(name)
        require(hashlib.sha256(data[name]).hexdigest() == expected,
                'unapproved bytes in ' + name)
    provenance = json.loads(data['vendor-upstream.json'])
    require(provenance['upstream'] == 'https://github.com/emilk/egui'
            and provenance['tag'] == VERSION
            and provenance['registry_sha256'] == ARCHIVE_SHA256
            and provenance['vcs'] == {'git': {'sha1': COMMIT}, 'path_in_vcs': 'crates/egui'},
            'upstream identity mismatch')
    original = {item['path']: item for item in provenance['members']}
    modified = {item['path']: item for item in provenance['cedar_patch']['modified_members']}
    require(len(original) == 92 and set(modified) == {
        'src/widgets/text_edit/builder.rs', 'src/widgets/text_edit/mod.rs',
        'src/widgets/text_edit/output.rs'}, 'unexpected original or modified members')
    require(paths == original.keys() | ADDITIONS.keys(), 'unexpected or missing source file set')
    for name, item in original.items():
        path = PurePosixPath(name)
        require(not path.is_absolute() and '..' not in path.parts, 'unsafe member path')
        item = modified.get(name, item)
        content = read(name)
        require(len(content) == item['bytes']
                and hashlib.sha256(content).hexdigest() == item['sha256'],
                'unapproved bytes in ' + name)
        data[name] = content
    return data


def filesystem_bytes(root):
    """Reject symlinks/escape paths before reading the exact local source set."""
    root = Path(root).resolve(strict=True)
    base = root / ROOT
    for part in (root / 'vendor', base):
        require(not part.is_symlink() and part.is_dir(), 'unsafe vendor directory')
    files = []
    for path in base.rglob('*'):
        require(not path.is_symlink(), 'symlink in vendor tree')
        if path.is_file():
            require(path.resolve(strict=True).is_relative_to(base), 'source path escapes vendor')
            files.append(path.relative_to(base).as_posix())
        else:
            require(path.is_dir(), 'non-regular vendor member')
    return validated_bytes(files, lambda name: (base / name).read_bytes())


def validate_package_licenses(cargo, payload):
    """Binary bundles retain one existing package identity, without source expansion."""
    if not patch_selected(cargo):
        return
    for name, digest in LICENSE_HASHES.items():
        path = 'third-party-licenses/egui-0.31.1/' + name
        require(path in payload and hashlib.sha256(payload[path]).hexdigest() == digest,
                'missing or unapproved bundled license ' + path)
