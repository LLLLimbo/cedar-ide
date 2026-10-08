#!/usr/bin/env python3
"""Collect numeric-only, bounded evidence from ONE already-exited owned JVM.

API: collect(owned_root, selection_path, expected_selection_sha256=None).
The private selection file must be
inside the JVM working directory (owned_root), with exactly SELECTION_FIELDS.
It is a driver attestation, not independently authenticated ownership evidence.
The driver must verify PID/creation time/image through its retained process
handle, verify these log names were absent before launch, and observe natural
shutdown before writing the witness. A corroborating sampler supplies the SHA-256
of those exact witness bytes; collection checks it before selecting logs and
again after collection. Only a supplied, matching digest sets
selection_binding_verified. This script never starts or attaches Java.

The only supported logging setting is LOGGING_ARGUMENT. Active + .0 + .1 are
the three possible files; the approximate rotation target is NOT a read bound.
All inputs, paths, PIDs, arbitrary text and exceptions stay private. Output is
rebuilt from fixed labels, bounded numbers and whole-file SHA-256 digests.

Heap numbers are logged integer MiB converted to bytes, rounded DOWN with a
1 MiB quantum. They describe used before/after and capacity AFTER that GC point.
They are not exact byte counts, current/idle/live heap, RSS, or maximum heap.
Pause duration has the logging format's 0.001 ms precision. No-GC means heap
not_observed; it never means zero used heap. Unsupported collectors retain only
their explicit collector identity. A complete report is not an acceptance pass.

Primary format/semantics references (OpenJDK 21):
https://docs.oracle.com/en/java/javase/21/docs/specs/man/java.html
https://github.com/openjdk/jdk21u/blob/master/src/hotspot/share/logging/logFileOutput.cpp
https://github.com/openjdk/jdk21u/blob/master/src/hotspot/share/memory/universe.cpp
https://github.com/openjdk/jdk21u/blob/master/src/hotspot/share/gc/shared/gcTraceTime.cpp
https://github.com/openjdk/jdk21u/blob/master/src/hotspot/share/gc/shared/collectedHeap.hpp
"""
import argparse
import hashlib
import json
import os
from pathlib import Path
import re
import stat
import sys

from collect_java_crash import check_components, check_root_identity, is_link, read_checked


LOGGING_ARGUMENT = '-Xlog:gc=info:file=cedar-gc-%p.log:uptimemillis,level,tags:filecount=2,filesize=64K'
LIMITS = {
    'files': 3, 'file_bytes': 128 * 1024, 'total_bytes': 384 * 1024,
    'selection_bytes': 4096, 'lines': 8192, 'line_bytes': 1024, 'events': 2048,
}
MIB = 1024 * 1024
MAX_INTEGER = (1 << 53) - 1  # Integers remain exact in downstream JSON readers.
SELECTION_TRUE_FIELDS = (
    'owned_jvm_identity_verified', 'root_image_verified',
    'log_files_absent_before_launch', 'root_handle_signaled', 'natural_shutdown_verified',
)
SELECTION_FIELDS = frozenset(SELECTION_TRUE_FIELDS) | {
    'schema_version', 'kind', 'pid', 'creation_time_100ns_since_1601', 'root_exit_code',
}
COLLECTORS = {
    'G1': 'g1', 'Serial': 'serial', 'Parallel': 'parallel', 'ZGC': 'zgc',
    'The Z Garbage Collector': 'zgc', 'Shenandoah': 'shenandoah', 'Epsilon': 'epsilon',
}
SUPPORTED_HEAP_COLLECTORS = {'g1', 'serial', 'parallel'}
EVENT_KINDS = {
    'Pause Young': 'pause_young', 'Pause Full': 'pause_full',
    'Pause Remark': 'pause_remark', 'Pause Cleanup': 'pause_cleanup',
}
DECORATED = re.compile(rb'\[([0-9]{1,16})ms\]\[info *\]\[gc *\] (.*)\Z')
HEAP_EVENT = re.compile(
    rb'GC\(([0-9]{1,10})\) (Pause Young|Pause Full|Pause Remark|Pause Cleanup)'
    rb'(?: \((?:[^\x00-\x1f\x7f()]{1,128}|System\.gc\(\))\)){0,3} '
    rb'([0-9]{1,16})M->([0-9]{1,16})M\(([0-9]{1,16})M\) '
    rb'([0-9]{1,12})\.([0-9]{3})ms\Z')
# These are ordinary G1 info lines without a heap observation. Never interpret
# a concurrent-cycle duration as a stop-the-world pause.
CONCURRENT_EVENT = re.compile(
    rb'GC\([0-9]{1,10}\) Concurrent (?:Mark Cycle|Undo Cycle)'
    rb'(?: [0-9]{1,12}\.[0-9]{3}ms)?\Z')


def empty_report():
    return {
        'schema_version': 1, 'diagnostic_only': True,
        'acceptance_result': 'not_evaluated', 'status': 'rejected', 'issues': [],
        'selection_binding_verified': False, 'selection_sha256': None,
        'collector': 'unknown', 'collector_header_count': 0,
        'heap_observation': 'not_observed', 'heap_value_semantics': 'gc_point_floor_mib',
        'heap_value_quantum_bytes': MIB, 'pause_precision_ms': 0.001,
        'files': [], 'events': [], 'lines_examined': 0, 'omitted_lines': 0,
    }


def rejected(issue):
    report = empty_report()
    report['issues'] = [issue]
    return report


def signature(info):
    return (info.st_dev, info.st_ino, info.st_mode, info.st_nlink, info.st_size,
            info.st_mtime_ns, info.st_ctime_ns,
            getattr(info, 'st_file_attributes', 0))


def checked_read(root, path, root_info, limit):
    check_root_identity(root, root_info)
    expected = check_components(path)
    if (not stat.S_ISREG(expected.st_mode) or is_link(expected)
            or expected.st_nlink != 1 or expected.st_size > limit):
        raise ValueError('input_rejected')
    data = read_checked(root, path, expected, root_info, limit)
    if signature(expected) != signature(check_components(path)):
        raise ValueError('input_changed')
    check_root_identity(root, root_info)
    return data, expected


def unique_object(pairs):
    result = {}
    for key, value in pairs:
        if key in result:
            raise ValueError('duplicate_key')
        result[key] = value
    return result


def read_selection(root, path, root_info, limits):
    """Return validated selection, path, stat and SHA-256 of exact read bytes."""
    if '..' in path.parts:
        raise ValueError('selection_path')
    if not path.is_absolute():
        path = root / path
    relative = path.relative_to(root)
    if not relative.parts or len(relative.parts) > 8:
        raise ValueError('selection_path')
    data, info = checked_read(root, path, root_info, limits['selection_bytes'])
    selection = json.loads(data.decode('utf-8-sig'), object_pairs_hook=unique_object)
    if not isinstance(selection, dict) or set(selection) != SELECTION_FIELDS:
        raise ValueError('selection_fields')
    if type(selection['schema_version']) is not int or selection['schema_version'] != 1:
        raise ValueError('selection_version')
    if selection['kind'] != 'cedar_gc_control_selection':
        raise ValueError('selection_kind')
    for key in SELECTION_TRUE_FIELDS:
        if selection[key] is not True:
            raise ValueError('selection_witness')
    if type(selection['root_exit_code']) is not int or selection['root_exit_code'] != 0:
        raise ValueError('selection_exit')
    for key, maximum in (('pid', (1 << 32) - 1),
                         ('creation_time_100ns_since_1601', (1 << 64) - 1)):
        if type(selection[key]) is not int or not 0 < selection[key] <= maximum:
            raise ValueError('selection_identity')
    return selection, path, info, hashlib.sha256(data).hexdigest()


def parse_logs(files, limits=None):
    """Parse bounded (slot, bytes) pairs; no raw strings can enter the report."""
    limits = LIMITS if limits is None else limits
    report = empty_report()
    report['status'] = 'complete'
    issues = set()
    headers = []
    events = []
    total_bytes = 0
    if len(files) > limits['files']:
        return rejected('file_count_limit')
    for slot, data in files:
        if slot not in ('active', 'rotation_0', 'rotation_1') or type(data) is not bytes:
            return rejected('invalid_parser_input')
        if len(data) > limits['file_bytes']:
            return rejected('file_bytes_limit')
        total_bytes += len(data)
        if total_bytes > limits['total_bytes']:
            return rejected('total_bytes_limit')
        report['files'].append({'slot': slot, 'bytes': len(data),
                                'sha256': hashlib.sha256(data).hexdigest()})
        if data and not data.endswith(b'\n'):
            issues.add('unterminated_line')
        previous_uptime = None
        for line in data.splitlines(keepends=True):
            if report['lines_examined'] >= limits['lines']:
                issues.add('line_count_limit')
                break
            report['lines_examined'] += 1
            if not line.endswith(b'\n'):
                report['omitted_lines'] += 1
                continue
            line = line[:-1]
            if line.endswith(b'\r'):
                line = line[:-1]
            if len(line) > limits['line_bytes']:
                issues.add('line_bytes_limit')
                report['omitted_lines'] += 1
                continue
            match = DECORATED.fullmatch(line)
            if not match:
                issues.add('unrecognized_line')
                report['omitted_lines'] += 1
                continue
            uptime, message = int(match[1]), match[2]
            if uptime > MAX_INTEGER:
                issues.add('numeric_range')
                report['omitted_lines'] += 1
                continue
            if previous_uptime is not None and uptime < previous_uptime:
                issues.add('timestamp_order')
            previous_uptime = uptime
            if message.startswith(b'Using '):
                # Decode only for a fixed enum lookup; never retain the value.
                name = COLLECTORS.get(message[6:].decode('ascii', errors='replace'), 'unknown')
                headers.append((uptime, name))
                continue
            event = HEAP_EVENT.fullmatch(message)
            if event:
                gc_id, before, after, capacity = (int(event[index]) for index in (1, 3, 4, 5))
                pause_us = int(event[6]) * 1000 + int(event[7])
                if (gc_id > (1 << 32) - 1 or
                        any(value * MIB > MAX_INTEGER for value in (before, after, capacity)) or
                        pause_us > MAX_INTEGER or capacity == 0 or after > capacity):
                    issues.add('numeric_range')
                    report['omitted_lines'] += 1
                    continue
                # Before may exceed AFTER-GC capacity when a heap shrinks.
                if len(events) >= limits['events']:
                    issues.add('event_count_limit')
                    report['omitted_lines'] += 1
                    continue
                events.append({
                    'uptime_ms': uptime, 'gc_id': gc_id,
                    'event_kind': EVENT_KINDS[event[2].decode('ascii')],
                    'pause_ms': pause_us / 1000,
                    'used_before_bytes': before * MIB, 'used_after_bytes': after * MIB,
                    'heap_capacity_bytes': capacity * MIB,
                })
            elif not CONCURRENT_EVENT.fullmatch(message):
                issues.add('unrecognized_gc_line')
                report['omitted_lines'] += 1
    report['collector_header_count'] = len(headers)
    if len(headers) == 0:
        issues.add('collector_header_missing')
    elif len(headers) != 1:
        issues.add('collector_header_repeated')
    else:
        header_uptime, report['collector'] = headers[0]
        if report['collector'] == 'unknown':
            issues.add('collector_unknown')
        elif report['collector'] not in SUPPORTED_HEAP_COLLECTORS:
            issues.add('collector_heap_format_unsupported')
        elif events and header_uptime > min(event['uptime_ms'] for event in events):
            issues.add('collector_header_order')
        else:
            report['events'] = sorted(events, key=lambda event: (event['uptime_ms'], event['gc_id']))
    keys = [(item['uptime_ms'], item['gc_id'], item['event_kind']) for item in report['events']]
    if len(set(keys)) != len(keys):
        issues.add('duplicate_event')
        report['events'] = []
    if report['events']:
        report['heap_observation'] = 'observed_gc_points'
    if not files:
        issues.add('logs_missing')
    if issues:
        report['status'] = 'partial'
    report['issues'] = sorted(issues)
    return report


def collect(owned_root, selection_path, expected_selection_sha256=None):
    """Read only direct expected filenames selected by the private witness."""
    if expected_selection_sha256 is not None and (
            type(expected_selection_sha256) is not str or
            re.fullmatch(r'[0-9a-f]{64}', expected_selection_sha256) is None):
        return rejected('invalid_selection_sha256')
    try:
        root = Path(owned_root)
        if '..' in root.parts:
            return rejected('root_rejected')
        root = root.absolute()
        root_info = check_components(root)
        if not stat.S_ISDIR(root_info.st_mode):
            return rejected('root_rejected')
    except (OSError, ValueError, TypeError):
        return rejected('root_rejected')
    try:
        selection, selection_file, selection_info, selection_sha256 = read_selection(
            root, Path(selection_path), root_info, LIMITS)
    except (OSError, ValueError, TypeError, RecursionError):
        return rejected('selection_rejected')
    # Bind corroborated ownership to the exact bytes selecting the PID. Do not
    # derive or open any GC filename before this comparison succeeds.
    if expected_selection_sha256 is not None and selection_sha256 != expected_selection_sha256:
        return rejected('selection_binding_mismatch')
    basename = 'cedar-gc-' + str(selection['pid']) + '.log'
    files = []
    snapshots = [(selection_file, selection_info)]
    missing = []
    try:
        for suffix, slot in (('', 'active'), ('.0', 'rotation_0'), ('.1', 'rotation_1')):
            path = root / (basename + suffix)
            try:
                info = path.lstat()
            except FileNotFoundError:
                missing.append(path)
                continue
            if info.st_size > LIMITS['file_bytes']:
                return rejected('file_bytes_limit')
            data, info = checked_read(root, path, root_info, LIMITS['file_bytes'])
            files.append((slot, data))
            snapshots.append((path, info))
        # Check the entire selected set again, including absent rotations and
        # the witness. A replaced/modified input invalidates ALL evidence.
        check_root_identity(root, root_info)
        for path, expected in snapshots:
            if signature(check_components(path)) != signature(expected):
                return rejected('input_changed')
        for path in missing:
            try:
                path.lstat()
            except FileNotFoundError:
                continue
            return rejected('input_changed')
        check_root_identity(root, root_info)
    except (OSError, ValueError):
        return rejected('input_rejected')
    report = parse_logs(files)
    try:
        _, _, final_info, final_sha256 = read_selection(root, selection_file, root_info, LIMITS)
        if final_sha256 != selection_sha256 or (
                expected_selection_sha256 is not None and final_sha256 != expected_selection_sha256):
            return rejected('selection_binding_mismatch')
        if signature(final_info) != signature(selection_info):
            return rejected('input_changed')
    except (OSError, ValueError, TypeError, RecursionError):
        return rejected('selection_rejected')
    if expected_selection_sha256 is not None:
        report['selection_binding_verified'] = True
        report['selection_sha256'] = selection_sha256
    if not any(slot == 'active' for slot, _ in files):
        report['issues'] = sorted(set(report['issues']) | {'active_log_missing'})
        report['status'] = 'partial'
    return report


class PrivateArgumentParser(argparse.ArgumentParser):
    def error(self, message):
        # argparse normally echoes unrecognized path-bearing arguments.
        print(json.dumps(rejected('invalid_arguments'), sort_keys=True))
        raise SystemExit(2)


def main(argv=None):
    parser = PrivateArgumentParser(description='Collect bounded numeric GC diagnostic evidence.')
    parser.add_argument('--owned-root', required=True)
    parser.add_argument('--selection', required=True)
    parser.add_argument('--expected-selection-sha256')
    args = parser.parse_args(argv)
    report = collect(args.owned_root, args.selection, args.expected_selection_sha256)
    print(json.dumps(report, sort_keys=True, allow_nan=False))
    return 0 if report['status'] == 'complete' else 1


if __name__ == '__main__':
    sys.exit(main())
