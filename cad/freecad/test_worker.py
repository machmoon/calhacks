"""Native persistent worker integration test, invoked with FreeCAD's Python."""
import json
import math
import os
from pathlib import Path
import subprocess
import sys
import threading
import queue

root=Path(__file__).resolve().parent
out=Path(os.environ['WRIGHT_CAD_OUT']).resolve()
out.mkdir(parents=True,exist_ok=True)
with (out/'worker-stderr.log').open('w') as errors:
    proc=subprocess.Popen([sys.executable,str(root/'worker.py')],stdin=subprocess.PIPE,stdout=subprocess.PIPE,stderr=errors,text=True)
    lines=queue.Queue()
    def read():
        for line in proc.stdout:lines.put(line)
        lines.put(None)
    threading.Thread(target=read,daemon=True).start()
    def receive():
        line=lines.get(timeout=60)
        if line is None: raise RuntimeError('worker exited prematurely')
        return json.loads(line)
    def request(i,method,params={}):
        proc.stdin.write(json.dumps(dict(jsonrpc='2.0',id=i,method=method,params=params))+'\n')
        proc.stdin.flush()
        answer=receive()
        assert answer['id']==i,answer
        return answer
    try:
        assert receive()['method']=='ready'
        first=request(1,'ping')['result']
        request(2,'document')['result']
        request(3,'make_part',dict(spec=dict(name='Plate',kind='box',params=dict(x=20,y=10,z=2))))['result']
        request(4,'set_parameter',dict(object_name='Plate',parameter='Length',value=25))['result']
        assert math.isclose(request(5,'inspect')['result'][0]['volume_mm3'],500,abs_tol=1e-7)
        assert request(6,'set_parameter',dict(object_name='Plate',parameter='Length',value=-1))['error']['code']==-32000
        assert math.isclose(request(7,'inspect')['result'][0]['volume_mm3'],500,abs_tol=1e-7)
        proc.stdin.write('{broken\n');proc.stdin.flush()
        assert receive()['error']['code']==-32700
        assert request(8,'unknown')['error']['code']==-32601
        saved=request(9,'save',dict(directory=str(out/'worker-save'),objects=['Plate']))['result']
        assert saved['step_schema']=='AP242'
        assert saved['preview']['meshes']==1
        assert request(10,'ping')['result']['pid']==first['pid']
        proc.stdin.close()
        assert proc.wait(timeout=30)==0
        (out/'worker-tests.json').write_text(json.dumps(dict(passed=True,persistent_pid=first['pid'],backend=first['backend'],requests=10,invalid_requests_survived=True)))
        print('Native worker integration passed')
    finally:
        if proc.poll() is None:
            proc.kill();proc.wait()
