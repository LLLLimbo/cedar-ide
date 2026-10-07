#!/usr/bin/env python3
"""Synthetic profile persistence/execution through an actual stdio agent.

Usage: task_profiles_smoke.py AGENT DEMO_BINARY
The frontend parser/state is tested in Rust; this checks the shared remote-host
Read/Write/Run path with literal arguments and revision conflicts. No SSH claim.
"""
import json
from pathlib import Path
import shutil
import sys
import tempfile
import time

from agent_harness import Agent


def terminal(agent, task_id, wanted, timeout=10):
    deadline = time.monotonic() + timeout
    last = None
    while time.monotonic() < deadline:
        last = agent.call('run_poll', task_id=task_id)['snapshot']
        if last['state'] in wanted:
            return last
        time.sleep(.02)
    raise AssertionError(f'Task did not reach {wanted}: {last}')


def main():
    if len(sys.argv) != 3 or sys.platform == 'win32':
        raise SystemExit(__doc__)
    binary, demo = map(lambda value: Path(value).resolve(), sys.argv[1:])
    assert demo.is_file()
    source = Path(__file__).resolve().parent.parent / 'examples/task-profiles-demo'
    with tempfile.TemporaryDirectory(prefix='cedar-profiles-') as tmp:
        root = Path(tmp) / 'project'
        shutil.copytree(source, root, ignore=shutil.ignore_patterns('target', 'sentinel.txt'))
        config_path = root / 'cedar.tasks.json'
        config = json.loads(config_path.read_text())
        for profile in config['profiles']:
            # This test invokes the prebuilt helper directly, with the exact argv
            # after Cargo's separator. The public sample stays toolchain-portable.
            assert profile['program'] == 'cargo' and profile['args'][:4] == ['run', '--offline', '--quiet', '--']
            profile['program'] = str(demo)
            profile['args'] = profile['args'][4:]
        config_path.write_text(json.dumps(config, ensure_ascii=False, indent=2) + '\n')
        agent = Agent(binary, root, allow_run=False)
        try:
            loaded = agent.call('read', path='cedar.tasks.json')
            assert json.loads(loaded['text']) == config
            first = config['profiles'][0]
            assert agent.call('run_start', ok=False, program=first['program'], args=first['args'], timeout_secs=first['timeout_secs'])['code'] == 'run_disabled'
            config['profiles'][0]['name'] = 'Literal arguments verified'
            written = agent.call('write', path='cedar.tasks.json', text=json.dumps(config, ensure_ascii=False, indent=2) + '\n', expected_revision=loaded['revision'])
            reloaded = agent.call('read', path='cedar.tasks.json')
            assert reloaded['revision'] == written['revision'] and json.loads(reloaded['text']) == config
            assert not (root / 'sentinel.txt').exists(), 'Load/save must not execute a profile'
            assert not (root / 'not-a-shell').exists()
            # External changes fail closed, preserving external disk and caller's draft.
            external = json.loads(reloaded['text'])
            external['profiles'][0]['name'] = 'External edit'
            config_path.write_text(json.dumps(external) + '\n')
            assert agent.call('write', ok=False, path='cedar.tasks.json', text=reloaded['text'], expected_revision=reloaded['revision'])['code'] == 'conflict'
            assert json.loads(config_path.read_text()) == external
        finally:
            agent.close()
        assert agent.process.returncode == 0
        # New explicit trusted connection; no config operation automatically runs.
        agent = Agent(binary, root, allow_run=True)
        try:
            snapshot = agent.call('read', path='cedar.tasks.json')
            profiles = json.loads(snapshot['text'])['profiles']
            assert not (root / 'sentinel.txt').exists()
            profile = profiles[0]
            task = agent.call('run_start', program=profile['program'], args=profile['args'], timeout_secs=profile['timeout_secs'])['snapshot']
            done = terminal(agent, task['id'], {'succeeded'})
            output = json.loads(done['stdout'].splitlines()[0])
            assert output['argv'] == profile['args']
            assert Path(output['cwd']).resolve() == root.resolve()
            assert not (root / 'not-a-shell').exists(), 'Shell-looking text must remain literal'
            assert not (root / 'sentinel.txt').exists()
            profile = profiles[1]
            task = agent.call('run_start', program=profile['program'], args=profile['args'], timeout_secs=profile['timeout_secs'])['snapshot']
            deadline = time.monotonic() + 5
            while True:
                live = agent.call('run_poll', task_id=task['id'])['snapshot']
                if 'Ready for cancellation' in live['stdout']:
                    break
                assert time.monotonic() < deadline, live
                time.sleep(.02)
            agent.call('run_cancel', task_id=task['id'])
            assert terminal(agent, task['id'], {'cancelled'})['state'] == 'cancelled'
            profile = profiles[2]
            task = agent.call('run_start', program=profile['program'], args=profile['args'], timeout_secs=profile['timeout_secs'])['snapshot']
            terminal(agent, task['id'], {'succeeded'})
            assert (root / 'sentinel.txt').read_text() == 'Created only by the explicitly launched demo\n'
        finally:
            agent.close()
        assert agent.process.returncode == 0
    print('PASS: saved profile load/save/reload/trust gate/conflict/literal argv/cwd/explicit run/cancel/sentinel via real agent')


if __name__ == '__main__':
    main()
