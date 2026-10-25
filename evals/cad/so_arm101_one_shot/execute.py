"""Execute one frozen model script, once, with oracle/source write isolation."""
import argparse,hashlib,json,os,subprocess,time
from pathlib import Path
ROOT=Path(__file__).resolve().parents[3]
def sha(path):return hashlib.sha256(Path(path).read_bytes()).hexdigest()
def profile(inputs,out):
    quoted=lambda p:json.dumps(str(Path(p).resolve()))
    return '\n'.join(['(version 1)','(allow default)','(deny network*)','(deny process-fork)','(deny file-write*)',f'(allow file-write* (subpath {quoted(out)}))',f'(deny file-read* (subpath {quoted(ROOT)}))',f'(allow file-read* (subpath {quoted(inputs)}))',f'(allow file-read* (subpath {quoted(out)}))',f'(deny file-read* (subpath {quoted(Path.home()/".codex")}))',f'(deny file-read* (subpath {quoted(Path.home()/".ssh")}))'])
def execute(inputs,out,python=None,timeout=300):
    inputs=Path(inputs).resolve();out=Path(out).resolve()
    if out.joinpath('execution-started.json').exists():raise ValueError('candidate was already attempted; no automatic retry')
    files={str(p.relative_to(inputs)):sha(p) for p in inputs.rglob('*') if p.is_file()}
    script=out/'candidate.py'
    if not script.is_file():raise ValueError('missing model candidate')
    out.joinpath('execution-started.json').write_text(json.dumps({'candidate_sha256':sha(script),'source_hashes':files,'timeout_seconds':timeout},indent=2))
    try:compile(script.read_text(),str(script),'exec')
    except SyntaxError as error:
        status={'exit_code':None,'syntax_error':str(error),'candidate_sha256':sha(script),'independent_score_pending':True}
        (out/'execution-result.json').write_text(json.dumps(status,indent=2));return status
    runtime=Path.home()/'Applications/FreeCAD.app/Contents/Resources'
    executable=Path(python) if python else runtime/'bin/python'
    for directory in ('home','tmp'):out.joinpath(directory).mkdir(exist_ok=True)
    policy=out/'sandbox.sb';policy.write_text(profile(inputs,out))
    env={'PATH':'/usr/bin:/bin','HOME':str(out/'home'),'TMPDIR':str(out/'tmp'),'PYTHONPATH':str(runtime/'lib'),'QT_QPA_PLATFORM':'offscreen','PYTHONUNBUFFERED':'1'}
    start=time.monotonic()
    with (out/'execution.log').open('w') as log:
        try:
            result=subprocess.run(['/usr/bin/sandbox-exec','-f',str(policy),str(executable),str(script),str(inputs),str(out)],cwd=out,env=env,stdout=log,stderr=subprocess.STDOUT,timeout=timeout)
            status={'exit_code':result.returncode,'timed_out':False}
        except subprocess.TimeoutExpired:status={'exit_code':None,'timed_out':True}
    status.update(elapsed_seconds=time.monotonic()-start,source_unchanged=all(sha(inputs/p)==v for p,v in files.items()),candidate_sha256=sha(script),artifacts=[p.name for p in out.iterdir() if p.is_file()],independent_score_pending=True)
    (out/'execution-result.json').write_text(json.dumps(status,indent=2));return status
if __name__=='__main__':
    p=argparse.ArgumentParser();p.add_argument('inputs');p.add_argument('output');a=p.parse_args();print(json.dumps(execute(a.inputs,a.output),indent=2))
