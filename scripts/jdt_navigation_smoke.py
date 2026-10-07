#!/usr/bin/env python3
"""Opt-in Linux agent -> installed JDT LS formatting/navigation smoke.

Usage: jdt_navigation_smoke.py AGENT JDTLS_DIRECTORY OUTPUT_JSON [JAVA]
Uses a fresh synthetic project/data directory, literal argv, no source saves,
no downloads, no server commands, and no new authentication material.
"""
import hashlib
import json
import os
from pathlib import Path
import selectors
import shutil
import signal
import subprocess
import sys
import tempfile
import time

MAX_FRAME = 8 * 1024 * 1024
GREETER_DRAFT = '// 未保存的格式化测试\npublic class Greeter{public static String greeting(String name){return "Hello "+name;} public static int count(){return 1;}}\n'
MAIN_DRAFT = 'public class Main { public static void main(String[] args) { System.out.println(Greeter.greeting("世界")); System.out.println(Greeter.greeting("Cedar")); } }\n'


class AgentError(RuntimeError):
    def __init__(self, kind, detail):
        self.kind, self.detail = kind, detail
        super().__init__(f'{kind}: {detail}')


class Agent:
    def __init__(self, binary, root):
        self.process = subprocess.Popen([str(binary), '--root', str(root), '--allow-run'],
                                        stdin=subprocess.PIPE, stdout=subprocess.PIPE,
                                        stderr=subprocess.PIPE, start_new_session=True)
        self.selector = selectors.DefaultSelector()
        self.selector.register(self.process.stdout, selectors.EVENT_READ, 'stdout')
        self.selector.register(self.process.stderr, selectors.EVENT_READ, 'stderr')
        self.buffer = bytearray()
        self.stderr = bytearray()
        self.counter = 0

    def call(self, kind, *, timeout=20, ok=True, **fields):
        self.counter += 1
        frame = json.dumps({'id': self.counter, 'op': {'type': kind, **fields}}).encode() + b'\n'
        assert len(frame) <= MAX_FRAME
        self.process.stdin.write(frame)
        self.process.stdin.flush()
        deadline = time.monotonic() + timeout
        while b'\n' not in self.buffer:
            remaining = deadline - time.monotonic()
            if remaining <= 0:
                raise TimeoutError(f'{kind}: no agent response within {timeout}s')
            for key, _ in self.selector.select(remaining):
                chunk = os.read(key.fileobj.fileno(), 65536)
                if not chunk:
                    self.selector.unregister(key.fileobj)
                    if key.data == 'stdout':
                        raise RuntimeError(f'Agent EOF during {kind}: {self.stderr.decode(errors="replace")}')
                elif key.data == 'stdout':
                    self.buffer.extend(chunk)
                    if len(self.buffer) > MAX_FRAME:
                        raise RuntimeError('Agent response exceeded wire limit')
                else:
                    self.stderr.extend(chunk)
                    del self.stderr[:-256 * 1024]
        raw, _, remaining = self.buffer.partition(b'\n')
        self.buffer = bytearray(remaining)
        response = json.loads(raw)
        assert response['id'] == self.counter, response
        result = response['result']
        if ('Ok' in result) != ok:
            raise AgentError(kind, result)
        return result.get('Ok', result.get('Err'))

    def close(self):
        self.process.stdin.close()
        try:
            self.process.wait(timeout=8)
        except subprocess.TimeoutExpired:
            # Owned, unreaped child is the session leader. No other process/PID lookup.
            os.killpg(self.process.pid, signal.SIGKILL)
            self.process.wait(timeout=5)
        self.selector.close()
        self.process.stdout.close()
        self.process.stderr.close()


def position(source, needle):
    offset = source.index(needle)
    before = source[:offset]
    return {'line': before.count('\n'),
            'character': len(before.rsplit('\n', 1)[-1].encode('utf-16-le')) // 2}


def scalar_offset(source, pos):
    # This synthetic fixture and JDT formatting output use LF. Production CR/LF,
    # astral and cursor handling are tested by the Rust planner, not this oracle.
    lines = source.split('\n')
    row, target = pos['line'], pos['character']
    assert isinstance(row, int) and 0 <= row < len(lines)
    assert isinstance(target, int) and 0 <= target <= 0x7fffffff
    units = 0
    for index, char in enumerate(lines[row]):
        if units == target:
            return sum(len(line) + 1 for line in lines[:row]) + index
        units += len(char.encode('utf-16-le')) // 2
        assert units <= target, 'Surrogate split in actual formatting response'
    assert units == target
    return sum(len(line) + 1 for line in lines[:row]) + len(lines[row])


def format_oracle(source, edits):
    assert isinstance(edits, list) and 0 < len(edits) <= 1024
    converted = []
    for edit in edits:
        assert set(edit) == {'range', 'newText'}, edit
        start = scalar_offset(source, edit['range']['start'])
        end = scalar_offset(source, edit['range']['end'])
        assert start <= end and '\0' not in edit['newText']
        converted.append((start, end, edit['newText']))
    converted.sort(key=lambda edit: (edit[0], edit[1]))
    for left, right in zip(converted, converted[1:]):
        assert left[1] <= right[0], 'Overlapping actual format edits'
        assert not (left[0] == left[1] == right[0]), 'Ambiguous insertion'
    text = source
    for start, end, replacement in reversed(converted):
        text = text[:start] + replacement + text[end:]
    assert len(text.encode()) <= 1024 * 1024
    return text


def names(symbols):
    result = []
    for item in symbols:
        result.append(item['name'])
        result.extend(names(item.get('children', [])))
    return result


def main():
    if len(sys.argv) not in (4, 5) or sys.platform != 'linux':
        raise SystemExit(__doc__)
    binary, home, output = (Path(value).resolve() for value in sys.argv[1:4])
    java = sys.argv[4] if len(sys.argv) == 5 else 'java'
    jars = list((home / 'plugins').glob('org.eclipse.equinox.launcher_*.jar'))
    assert len(jars) == 1, 'Expected one exact Equinox launcher'
    fixture = Path(__file__).resolve().parent.parent / 'examples/java-navigation-demo'
    evidence = {'date_utc': time.strftime('%Y-%m-%d', time.gmtime()),
                'kind': 'actual JDT via stdio workspace agent, not native GUI',
                'agent_sha256': hashlib.sha256(binary.read_bytes()).hexdigest(),
                'jdtls_directory': str(home), 'checks': []}
    started = time.monotonic()
    with tempfile.TemporaryDirectory(prefix='cedar-jdt-navigation-') as tmp:
        root = Path(tmp) / 'project'
        shutil.copytree(fixture, root, ignore=shutil.ignore_patterns('bin'))
        sources = {path: path.read_bytes() for path in (root / 'src').glob('*.java')}
        agent = Agent(binary, root)
        running = False
        try:
            hello = agent.call('hello')
            assert hello['protocol'] == 4
            arguments = ['-Declipse.application=org.eclipse.jdt.ls.core.id1',
                         '-Dosgi.bundles.defaultStartLevel=4',
                         '-Declipse.product=org.eclipse.jdt.ls.core.product',
                         '-Dlog.level=WARNING', '-Xmx512m', '--add-modules=ALL-SYSTEM',
                         '--add-opens', 'java.base/java.util=ALL-UNNAMED',
                         '--add-opens', 'java.base/java.lang=ALL-UNNAMED',
                         '-jar', str(jars[0]), '-configuration', str(home / 'config_linux'),
                         '-data', str(Path(tmp) / 'jdt-data')]
            initialized = agent.call('language_start', program=java, args=arguments, timeout=75)['value']
            running = True
            evidence['initialize'] = initialized
            evidence['checks'].append('initialize_real_jdt')
            for path, text in [('src/Greeter.java', GREETER_DRAFT), ('src/Main.java', MAIN_DRAFT)]:
                agent.call('language_open', path=path, language_id='java', version=1, text=text)
            evidence['checks'].append('two_unsaved_documents_synchronized')
            # Wait for actual cross-file semantic results, not arbitrary fixed sleep.
            deadline = time.monotonic() + 40
            while True:
                try:
                    refs = agent.call('language_references', path='src/Greeter.java',
                                      **position(GREETER_DRAFT, 'greeting'), include_declaration=True)['value']
                except AgentError as error:
                    if 'timed out' not in str(error) or time.monotonic() >= deadline:
                        raise
                    # Read-only semantic request can time out during initial import.
                    # Retry explicitly within this synthetic readiness deadline.
                    evidence.setdefault('readiness_timeouts', []).append(str(error))
                    refs = None
                if isinstance(refs, list) and len(refs) == 3:
                    break
                if time.monotonic() >= deadline:
                    raise AssertionError(f'Expected declaration plus two unsaved calls: {refs}')
                agent.call('language_events')
                time.sleep(.15)
            evidence['references'] = refs
            resolved = [agent.call('language_resolve_uri', uri=item['uri'])['value']['path'] for item in refs]
            assert resolved.count('src/Main.java') == 2 and resolved.count('src/Greeter.java') == 1
            evidence['checks'].append('cross_file_references_include_unsaved_secondary_document')
            symbols = agent.call('language_document_symbols', path='src/Greeter.java')['value']
            labels = names(symbols)
            assert any('Greeter' in label for label in labels), labels
            assert any('greeting' in label for label in labels), labels
            assert any('count' in label for label in labels), labels
            evidence['outline'] = symbols
            evidence['checks'].append('actual_class_and_method_outline')
            edits = agent.call('language_format', path='src/Greeter.java', version=1,
                               tab_size=4, insert_spaces=True)['value']
            formatted = format_oracle(GREETER_DRAFT, edits)
            assert formatted != GREETER_DRAFT and '未保存的格式化测试' in formatted
            assert 'greeting(String name)' in formatted and 'count()' in formatted
            evidence['format_edits'] = edits
            evidence['formatted_text'] = formatted
            evidence['checks'].append('actual_plain_text_format_edits_preserve_chinese')
            agent.call('language_change', path='src/Greeter.java', version=2, text=formatted)
            stale = agent.call('language_format', ok=False, path='src/Greeter.java', version=1,
                               tab_size=4, insert_spaces=True)
            evidence['stale_version_error'] = stale
            assert 'version' in stale['code'] or 'stale' in stale['message'].lower(), stale
            evidence['checks'].append('stale_format_version_rejected')
            second = agent.call('language_format', path='src/Greeter.java', version=2,
                                tab_size=4, insert_spaces=True)['value']
            assert second is None or second == [] or format_oracle(formatted, second) == formatted
            evidence['checks'].append('formatted_document_is_idempotent')
            outside = Path(tmp) / 'outside.java'
            outside.write_text('class Outside {}\n')
            assert agent.call('language_resolve_uri', ok=False, uri=outside.as_uri())['code'] == 'invalid_path'
            evidence['checks'].append('outside_workspace_navigation_rejected')
            for path in ('src/Greeter.java', 'src/Main.java'):
                agent.call('language_close', path=path)
            agent.call('language_stop')
            running = False
            evidence['checks'].append('did_close_and_shutdown')
            assert all(path.read_bytes() == before for path, before in sources.items())
            evidence['checks'].append('all_project_source_disk_bytes_unchanged')
            evidence['source_sha256'] = {path.name: hashlib.sha256(data).hexdigest() for path, data in sources.items()}
        finally:
            if running:
                try:
                    agent.call('language_stop', timeout=15)
                except Exception:
                    pass
            agent.close()
        assert agent.process.returncode == 0, agent.process.returncode
    evidence['temporary_fixture_removed'] = not root.exists()
    evidence['elapsed_seconds'] = round(time.monotonic() - started, 3)
    evidence['commands_or_workspace_apply_edit_executed'] = False
    output.parent.mkdir(parents=True, exist_ok=True)
    output.write_text(json.dumps(evidence, ensure_ascii=False, indent=2) + '\n')
    print(f'PASS: {len(evidence["checks"])} real-JDT protocol checks; evidence {output}')


if __name__ == '__main__':
    main()
