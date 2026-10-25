"""Fixed-parameter live FreeCAD demo. No model planning or engineering audit."""
import hashlib,json,os,signal,subprocess,sys,time
from pathlib import Path
from plan import parse_design
from timing import TimingTrace
ROOT=Path(__file__).resolve().parents[2]

def parameters(request):
    rows=[line.partition(':')[2].strip() for line in request.splitlines() if line.startswith('WRIGHT_DESIGN_JSON:')]
    if len(rows)!=1:raise ValueError('Fast demo requires exactly one WRIGHT_DESIGN_JSON line')
    values=json.loads(rows[0])
    return parse_design('DESIGN = '+repr(values),appearance=True)

def run(brief,request,output,pdf_manifest=None):
    if pdf_manifest:raise ValueError('Fixed-parameter live demo does not interpret PDFs. Use the normal PDF workflow.')
    design=parameters(request);out=Path(output).resolve();out.mkdir(parents=True,exist_ok=True)
    if (out/'events.jsonl').exists():raise ValueError('Use a fresh run directory')
    trace=TimingTrace(out);start=time.monotonic();child=None;complete=False
    def emit(kind,**fields):
        with (out/'events.jsonl').open('a') as f:f.write(json.dumps(dict(type=kind,run=out.name,**trace.stamp(),**fields))+'\n')
    def message(text):emit('agent_message',agent='FreeCAD live demo',text=text)
    def stop(signum,frame):raise SystemExit(128+signum)
    previous=signal.signal(signal.SIGTERM,stop)
    metadata=dict(status='running',execution_kind='fixed_parameter_live_native_recipe',model_calls=0,engineering_checks='not_run',request=request,started_at=time.time())
    try:
        emit('run_started',domain='aircraft',request=request,execution_kind=metadata['execution_kind'])
        message('Using the exact supplied dimensions. Opening live FreeCAD; no model parameter planning or engineering audit.')
        (out/'request.json').write_text(json.dumps(metadata,indent=2));(out/'design-parameters.json').write_text(json.dumps(design,indent=2))
        (out/'BUILD_PROMPT.txt').write_text(Path(brief).read_text())
        cad=out/'cad';cad.mkdir()
        command=[str(ROOT/'cad/freecad/run.sh'),str(Path(brief).parent/'build.py'),'--output-dir',str(cad),'--design',str(out/'design-parameters.json'),'--package',str(Path(brief).parent/'package-v1'),'--live-demo','--skip-renders']
        with (out/'freecad.log').open('w') as log:
            child=subprocess.Popen(command,cwd=ROOT,stdout=log,stderr=subprocess.STDOUT,start_new_session=True)
        seen=set();last=None
        with trace.span('live_native_build'):
            while True:
                p=cad/'progress.json'
                try:
                    phase=json.loads(p.read_text()).get('status')
                    if phase!=last:message(phase);last=phase
                except (OSError,ValueError):pass
                for image in sorted(cad.glob('live-*.png')):
                    if image.name not in seen and image.stat().st_size:
                        emit('render',view=image.stem,path=str(image));seen.add(image.name)
                marker=cad/'live-complete.json'
                if marker.exists():
                    receipt=json.loads(marker.read_text());complete=True;break
                if child.poll() is not None:raise RuntimeError('Live FreeCAD exited before completion; inspect freecad.log')
                if time.monotonic()-start>240:raise TimeoutError('Live demo exceeded 240s; not a successful two-minute run')
                time.sleep(.2)
        native=cad/'assembly.FCStd';digest=hashlib.sha256(native.read_bytes()).hexdigest()
        if receipt['native_sha256']!=digest:raise ValueError('Live completion revision mismatch')
        elapsed=time.monotonic()-start
        metadata.update(status='demo_built_unverified',elapsed_s=elapsed,under_two_minutes=elapsed<120,native_sha256=digest,live_receipt=receipt)
        (out/'cad-ready.json').write_text(json.dumps(metadata,indent=2))
        for path in [native,cad/'open.FCMacro',cad/'artifact-manifest.json',out/'cad-ready.json']:
            emit('artifact',path=str(path),label=path.name)
        message(f'Live editable CAD completed in {elapsed:.2f}s. Collision, simulation, fit and export-equivalence checks were not run. FreeCAD remains open for editing.')
        emit('run_finished',passed=False,objective=elapsed,objective_label='Live CAD ready; engineering checks not run')
    except BaseException as error:
        metadata.update(status='failed',error=str(error));raise
    finally:
        if child is not None and not complete and child.poll() is None:
            try:os.killpg(child.pid,signal.SIGKILL)
            except ProcessLookupError:pass
            child.wait()
        signal.signal(signal.SIGTERM,previous)
        metadata['finished_at']=time.time();(out/'request.json').write_text(json.dumps(metadata,indent=2));trace.finish(metadata['status'])
