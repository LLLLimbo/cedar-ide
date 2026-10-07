#!/usr/bin/env python3
"""Collect SPDX metadata and bundled upstream notices from Cargo's exact locked graph."""
import json, pathlib, shutil, subprocess
root=pathlib.Path(__file__).resolve().parent.parent
metadata=json.loads(subprocess.check_output(['cargo','metadata','--format-version','1','--locked','--offline'],cwd=root))
out=root/'third-party-licenses';out.mkdir(exist_ok=True)
lines=['# Third-party dependencies','','Generated from the exact Cargo.lock dependency graph. Includes target-specific and optional packages; not all are linked on every platform. Each dependency retains its upstream license. Font notices from epaint_default_fonts are included. No third-party source code, server distribution, or JDK is vendored here.','','| Package | Version | License expression | Repository |','|---|---|---|---|']
for p in sorted(metadata['packages'],key=lambda p:(p['name'],p['version'])):
    if p['source'] is None: continue
    lines.append('| {name} | {version} | {license} | {repository} |'.format(**{**p,'license':p.get('license') or 'See bundled license file','repository':p.get('repository') or ''}))
    base=pathlib.Path(p['manifest_path']).parent;dest=out/(p['name']+'-'+p['version']);files=set()
    for item in base.iterdir():
        if item.is_file() and any(item.name.upper().startswith(x) for x in ['LICENSE','LICENCE','COPYING','NOTICE','AUTHORS']): files.add(item)
    if p.get('license_file'): files.add(base/p['license_file'])
    if p['name']=='epaint_default_fonts': files.update((base/'fonts').glob('*.txt'))
    for item in sorted(files):
        if not item.is_file(): continue
        dest.mkdir(exist_ok=True);shutil.copyfile(item,dest/item.name)
(root/'THIRD_PARTY_NOTICES.md').write_text('\n'.join(lines)+'\n')
print(f'Collected notices for {len(lines)-6} locked packages')
