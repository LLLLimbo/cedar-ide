#!/usr/bin/env python3
"""Synthetic Linux/POSIX stdio task lifecycle; no user servers or credentials."""
import json,pathlib,subprocess,sys,tempfile,time
agent=str(pathlib.Path(sys.argv[1]).resolve())
with tempfile.TemporaryDirectory(prefix='cedar-task-bridge-') as tmp:
    p=subprocess.Popen([agent,'--root',tmp,'--allow-run'],stdin=subprocess.PIPE,stdout=subprocess.PIPE,stderr=subprocess.PIPE,text=True)
    counter=0
    def call(kind,ok=True,**kw):
        global counter
        counter+=1;p.stdin.write(json.dumps({'id':counter,'op':{'type':kind,**kw}})+'\n');p.stdin.flush()
        reply=json.loads(p.stdout.readline());assert reply['id']==counter
        result=reply['result'];assert ('Ok' in result)==ok,result
        return result.get('Ok',result.get('Err'))
    def poll_until(task_id, predicate, seconds=3):
        deadline=time.monotonic()+seconds
        while time.monotonic()<deadline:
            snapshot=call('run_poll',task_id=task_id)['snapshot']
            if predicate(snapshot):return snapshot
            time.sleep(.01)
        raise AssertionError(f'task failed to reach expected state: {snapshot}')
    started=time.monotonic()
    first=call('run_start',program='sh',args=['-c','printf live; sleep 20'],timeout_secs=30)['snapshot']
    assert time.monotonic()-started<2
    task=first['id']
    poll_until(task,lambda s:'live' in s['stdout'])
    assert call('run_start',ok=False,program='echo',args=['second'],timeout_secs=1)['code']=='task_busy'
    call('write',path='during.txt',text='editing while the task runs',expected_revision=None)
    assert call('read',path='during.txt')['text']=='editing while the task runs'
    call('run_cancel',task_id=task)
    cancelled=poll_until(task,lambda s:s['state']=='cancelled')
    assert 'live' in cancelled['stdout']
    assert call('run_cancel',task_id=task)['snapshot']['state']=='cancelled'
    second=call('run_start',program='sh',args=['-c','printf finished'],timeout_secs=1)['snapshot']
    done=poll_until(second['id'],lambda s:s['state']=='succeeded')
    assert done['stdout']=='finished' and done['exit_code']==0
    assert call('run_cancel',task_id=second['id'])['snapshot']['state']=='succeeded'
    missing=call('run_start',program='cedar-deliberately-nonexistent-program',args=[],timeout_secs=1)['snapshot']
    assert poll_until(missing['id'],lambda s:s['state']=='spawn_failed')['error']
    assert call('run_poll',ok=False,task_id=2**63)['code']=='unknown_task'
    p.stdin.close();assert p.wait(timeout=3)==0
print('PASS: async agent start/live output/concurrent file editing/busy gate/cancel/repeated cancel/success/spawn failure/unknown task/EOF')
