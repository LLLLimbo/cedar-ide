#!/usr/bin/env python3
"""Small repeatable Linux agent RSS sample; not a full IDE comparison benchmark."""
import hashlib,json,pathlib,platform,subprocess,sys,tempfile,time
exe=str(pathlib.Path(sys.argv[1]).resolve())
with tempfile.TemporaryDirectory(prefix='cedar-resource-') as tmp:
    root=pathlib.Path(tmp)
    text='// synthetic resource fixture\n'+'class Example { int counter; }\n'*64
    for i in range(500): (root/f'File{i:03}.java').write_text(text)
    start=time.monotonic();p=subprocess.Popen([exe,'--root',tmp],stdin=subprocess.PIPE,stdout=subprocess.PIPE,stderr=subprocess.PIPE,text=True)
    counter=0
    def request(op):
        global counter
        counter+=1;p.stdin.write(json.dumps({'id':counter,'op':op})+'\n');p.stdin.flush();res=json.loads(p.stdout.readline());assert 'Ok' in res['result'];return res['result']['Ok']
    request({'type':'hello'});request({'type':'list','path':''})
    for i in range(10):request({'type':'read','path':f'File{i:03}.java'})
    request({'type':'search','query':'counter','limit':500})
    values={}
    for line in pathlib.Path(f'/proc/{p.pid}/status').read_text().splitlines():
        if line.startswith(('VmRSS:','VmHWM:','Threads:')):
            k,v=line.split(':',1);values[k]=v.strip()
    p.stdin.close();exit_code=p.wait(timeout=3)
    print(json.dumps({'platform':platform.platform(),'fixture_files':500,'fixture_utf8_bytes':500*len(text.encode()),'requests':counter,'elapsed_seconds':round(time.monotonic()-start,4),'exit_code':exit_code,**values,'note':'release agent only; no GUI/JVM/SSH/network or IDEA baseline'},indent=2))
