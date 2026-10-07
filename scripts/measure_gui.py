#!/usr/bin/env python3
"""Interactive Linux GUI measurement wrapper. Close Cedar normally to finish.
/proc samples isolate the frontend PID from JDK/adapters. GPU-service RSS is excluded.
"""
import json,pathlib,platform,resource,subprocess,sys,time
exe=str(pathlib.Path(sys.argv[1]).resolve());out=pathlib.Path(sys.argv[2]);started=time.monotonic()
p=subprocess.Popen([exe]);samples=0;max_rss=0;max_hwm=0;max_threads=0
while p.poll() is None:
    try:
        status={}
        for line in pathlib.Path(f'/proc/{p.pid}/status').read_text().splitlines():
            if line.startswith(('VmRSS:','VmHWM:','Threads:')):
                k,v=line.split(':',1);status[k]=int(v.strip().split()[0])
        if 'VmRSS' in status:
            samples+=1;max_rss=max(max_rss,status['VmRSS']);max_hwm=max(max_hwm,status.get('VmHWM',0));max_threads=max(max_threads,status.get('Threads',0))
    except (FileNotFoundError,ProcessLookupError): pass
    time.sleep(0.1)
status=p.wait();usage=resource.getrusage(resource.RUSAGE_CHILDREN)
out.write_text(json.dumps({'platform':platform.platform(),'executable':exe,'elapsed_seconds':round(time.monotonic()-started,3),'exit_code':status,'frontend_pid_samples':samples,'frontend_max_sampled_rss_kib':max_rss if samples else None,'frontend_max_observed_hwm_kib':max_hwm if samples else None,'frontend_max_threads':max_threads if samples else None,'child_rusage_max_rss_kib':usage.ru_maxrss,'child_user_cpu_seconds':usage.ru_utime,'child_system_cpu_seconds':usage.ru_stime,'note':'Frontend /proc samples exclude separate JVM/adapter processes. Child rusage may include descendants and is not a concurrent tree total. GPU-service memory excluded. No IDEA comparison.'},indent=2)+'\n')
sys.exit(status)
