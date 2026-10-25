"""Run a prepared native CAD project, recording its brief and real outputs.

This executes a registered project recipe; it is not a general text-to-CAD model.
Missing engineering gates remain missing even when geometry builds successfully.
"""
import argparse
import hashlib
import json
import subprocess
import sys
import time
import shutil
import os
import signal
from plan import plan
from timing import TimingTrace, span
from pathlib import Path

ROOT = Path(__file__).resolve().parents[2]


def check_outcomes(report):
    outcomes = []
    for name, gate in report['gates'].items():
        detail = gate.get('reason') or gate['requirement']
        status = gate['status']
        if status not in {'passed', 'failed', 'pending', 'inconclusive'}:
            raise ValueError(f'Unknown audit status: {status}')
        evidence = ({'Margin': {'value': 1 if status=='passed' else -1, 'unit': 'predicate'}}
                    if status in {'passed', 'failed'} else {'Missing': {'why': detail}})
        outcomes.append(dict(name=name, source='Requirement', evidence=evidence, detail=detail))
    return outcomes


def build_with_progress(command, output, message, timing=None, on_render=None, on_ready=None, on_tick=None):
    """Expose actual builder phases while retaining a bounded subprocess."""
    published=set()
    ready_published=False
    seen_native_phases=set()
    def publish_complete():
        nonlocal ready_published
        if on_tick is not None:on_tick()
        try:
            rows=[json.loads(line) for line in (output/'cad/build-phases.jsonl').read_text().splitlines() if line.strip()]
            for i,row in enumerate(rows):
                key=(row.get('phase'),row.get('monotonic_seconds'))
                if key in seen_native_phases:continue
                seen_native_phases.add(key)
                duration=rows[i+1]['monotonic_seconds']-row['monotonic_seconds'] if i+1<len(rows) else None
                message(str(row.get('phase','Native phase'))+(f' ({duration:.3f}s)' if duration is not None else ''))
        except (OSError,ValueError,KeyError,TypeError):pass
        if on_ready is not None and not ready_published and (output/"cad/audit-ready.json").is_file():
            on_ready(output/"cad/audit-ready.json");ready_published=True
        if on_render is None:return
        for path in sorted((output/'cad').glob('*.png')):
            if path in published:continue
            try:
                if path.stat().st_size<20:continue
                with path.open('rb') as image:
                    if image.read(8)!=b'\x89PNG\r\n\x1a\n':continue
                    image.seek(-12,2)
                    if image.read()!=b'\x00\x00\x00\x00IEND\xaeB`\x82':continue
            except OSError:continue
            on_render(path);published.add(path)
            if timing:timing.event('render_available',view=path.stem)
    with (output/'freecad.log').open('w') as log:
        child = subprocess.Popen(command, cwd=ROOT, stdout=log, stderr=subprocess.STDOUT,start_new_session=os.name=='posix')
        deadline = time.monotonic()+900
        last_phase = None
        phase_id = None
        try:
            while child.poll() is None:
                publish_complete()
                if time.monotonic() >= deadline:
                    raise TimeoutError('Native build exceeded 900 seconds')
                try:
                    progress = json.loads((output/'cad/progress.json').read_text())
                    phase = progress.get('status')
                    if phase and phase != last_phase:
                        if timing:
                            if phase_id is not None:timing.end(phase_id)
                            phase_id=timing.begin('freecad_progress_phase',label=phase)
                        if not seen_native_phases:message(phase)
                        last_phase = phase
                except (OSError, json.JSONDecodeError):
                    pass  # The builder may be replacing the progress file.
                time.sleep(1)
            publish_complete()
            return child.returncode
        finally:
            if child.poll() is None:
                if os.name=='posix':
                    try:os.killpg(child.pid,signal.SIGKILL)
                    except ProcessLookupError:pass
                else:child.kill()
            child.wait()
            if timing and phase_id is not None:timing.end(phase_id,'succeeded' if child.returncode==0 else 'failed',return_code=child.returncode)


def audit_progress_publisher(output,message):
    seen_start=set();seen_finish=set()
    def publish():
        path=output/'audit/probe-timings.json'
        try:
            if path.stat().st_size>1024*1024:return
            snapshot=json.loads(path.read_text())
            for row in snapshot.get('probes',[]):
                stage=row.get('stage')
                if not isinstance(stage,str):continue
                key=(stage,row.get('started_monotonic_seconds'))
                if row.get('status')=='skipped':
                    if key not in seen_finish:message(f"Audit {stage}: skipped; no completed check claimed.");seen_finish.add(key)
                    continue
                if key not in seen_start:
                    message(f"Audit {stage}: started.");seen_start.add(key)
                if row.get('status')=='running' or key in seen_finish:continue
                duration=row.get('duration_seconds')
                elapsed=f"{duration:.2f}s" if isinstance(duration,(int,float)) else 'unrecorded duration'
                verdict='local result passed' if row.get('engineering_passed') is True else 'local result did not pass' if row.get('engineering_passed') is False else 'no engineering verdict recorded'
                message(f"Audit {stage}: finished in {elapsed}; execution {row.get('status','unknown')}; {verdict}.")
                seen_finish.add(key)
        except (OSError,json.JSONDecodeError,TypeError,ValueError):
            pass  # Progress is advisory; the final native report remains authoritative.
    return publish


def audit_with_progress(command, output, message, timeout_seconds=1800, prestarted=None, publisher=None):
    publish=publisher or audit_progress_publisher(output,message)
    with (output/'audit.log').open('a' if prestarted is not None else 'w') as log:
        child=prestarted;cancelled=False
        previous=signal.getsignal(signal.SIGTERM)
        def terminate(signum,frame):
            nonlocal cancelled
            cancelled=True
            # Do not interrupt Popen before its owned process handle is assigned.
            if child is not None:raise SystemExit(128+signum)
        signal.signal(signal.SIGTERM,terminate)
        try:
            if cancelled:raise SystemExit(128+signal.SIGTERM)
            if child is None:child=subprocess.Popen(command,cwd=ROOT,stdout=log,stderr=subprocess.STDOUT,start_new_session=os.name=='posix')
            if cancelled:raise SystemExit(128+signal.SIGTERM)
            deadline=time.monotonic()+timeout_seconds
            while child.poll() is None:
                publish()
                if time.monotonic()>=deadline:raise subprocess.TimeoutExpired(command,timeout_seconds)
                time.sleep(min(1,max(0,deadline-time.monotonic())))
            publish()
            return subprocess.CompletedProcess(command,child.returncode)
        finally:
            # A repeated stop signal must not interrupt owned-group cleanup.
            signal.signal(signal.SIGTERM,signal.SIG_IGN)
            try:
                if child is not None:
                    if cancelled or child.poll() is None:
                        if os.name=='posix':
                            try:os.killpg(child.pid,signal.SIGKILL)
                            except ProcessLookupError:pass
                        else:child.kill()
                    child.wait()
            finally:signal.signal(signal.SIGTERM,previous)


def resolve_brief(value, root=ROOT):
    path = (root / value).resolve()
    projects = (root / 'cad/projects').resolve()
    if not path.is_relative_to(projects) or path.name != 'BUILD_PROMPT.txt':
        raise ValueError('Choose a BUILD_PROMPT.txt inside cad/projects')
    if not path.is_file():
        raise ValueError(f'Project brief does not exist: {path}')
    return path


def cad_ready(cad, elapsed_s):
    """Fail closed: demo readiness is real artifacts, never engineering acceptance."""
    manifest=json.loads((cad/'artifact-manifest.json').read_text())
    native=cad/'assembly.FCStd'
    digest=hashlib.sha256(native.read_bytes()).hexdigest()
    if manifest.get('native_sha256')!=digest or manifest.get('invalid_physical')!=[]:
        raise ValueError('CAD readiness requires the matching validated native revision')
    views=['assembled','internals','rear-three-quarter','rear','cutaway','exploded']
    for view in views:
        if not (cad/(view+'.png')).read_bytes().startswith(b'\x89PNG\r\n\x1a\n'):
            raise ValueError('Missing native view: '+view)
    if manifest.get('render_failures'):raise ValueError('Native rendering failed')
    for name in ['airframe.step','open.FCMacro',manifest['proxy_module']]:
        path=Path(name);path=path if path.is_absolute() else cad/path
        if not path.is_file() or not path.stat().st_size:raise ValueError('Missing CAD artifact: '+name)
    return dict(status='cad_ready_verification_pending',elapsed_s=elapsed_s,native_sha256=digest,
                views=views,under_two_minutes=elapsed_s<120,engineering_verified=False)


def main():
    parser = argparse.ArgumentParser()
    parser.add_argument('--brief', required=True)
    parser.add_argument('--request', default='')
    parser.add_argument('--output-dir', required=True)
    parser.add_argument('--pdf-manifest')
    parser.add_argument('--cad-first', action='store_true', help='Prioritize editable CAD and views; run the full audit afterward')
    args = parser.parse_args()
    brief = resolve_brief(args.brief)
    if 'WRIGHT_EDIT_SESSION:' in args.request:
        if args.pdf_manifest:raise ValueError('Live parameter edits do not interpret PDF attachments')
        from live_edit import run as live_edit_run
        return live_edit_run(args.request,args.output_dir)
    if 'WRIGHT_FAST_DEMO' in args.request:
        from fast_demo import run as fast_demo_run
        return fast_demo_run(brief,args.request,args.output_dir,args.pdf_manifest)
    if brief.parent.name != 'vtol-demonstrator':
        raise ValueError('No planning contract registered for this project yet')
    output = Path(args.output_dir).resolve()
    output.mkdir(parents=True, exist_ok=True)
    events = output / 'events.jsonl'
    if events.exists():
        raise ValueError('Use a fresh run directory')
    run = output.name
    timing=TimingTrace(output)
    def emit(kind, **fields):
        with events.open('a') as stream:
            stream.write(json.dumps(dict(type=kind, run=run, **timing.stamp(), **fields))+'\n')
    def message(text):
        emit('agent_message', agent='FreeCAD project', text=text)
    def artifact(path, label):
        emit('artifact', path=str(path), label=label)
    published_renders=set()
    def publish_render(path, view=None):
        key=str(path.resolve())
        if key in published_renders:return
        published_renders.add(key)
        artifact(path,path.name)
        emit('render',view=view or path.stem,path=str(path))
    emit('run_started', domain='aircraft', request=args.request or str(brief), execution_kind='native_freecad')
    text = brief.read_text()
    (output/'BUILD_PROMPT.txt').write_text(text)
    recipe = brief.parent/'build.py'
    metadata = dict(brief=str(brief), brief_sha256=hashlib.sha256(brief.read_bytes()).hexdigest(),
                    request=args.request, execution_kind='astra_planned_prepared_native_project_recipe',
                    started_at=time.time(), status='running')
    (output/'request.json').write_text(json.dumps(metadata, indent=2))
    artifact(output/'BUILD_PROMPT.txt', 'Executed project brief')
    message('Read project brief from disk. Running its native FreeCAD recipe; custom PCB generation is not part of this path.')
    background_audit=None
    publish_audit=audit_progress_publisher(output,message)
    audit_timing=None
    previous_term=signal.getsignal(signal.SIGTERM)
    def cancel_run(signum,frame):raise SystemExit(128+signum)
    signal.signal(signal.SIGTERM,cancel_run)
    def start_audit(manifest):
        nonlocal background_audit,audit_timing
        frozen=json.loads(manifest.read_text())
        native=output/'cad/assembly.FCStd'
        if frozen.get('native_sha256')!=hashlib.sha256(native.read_bytes()).hexdigest():
            raise RuntimeError('Audit-ready native revision does not match its manifest')
        command=[sys.executable,str(ROOT/'evals/cad/vtol_demonstrator/run.py'),str(native),str(manifest),str(output/'audit')]
        audit_timing=timing.begin('independent_audit')
        with (output/'audit.log').open('w') as log:
            background_audit=subprocess.Popen(command,cwd=ROOT,stdout=log,stderr=subprocess.STDOUT,start_new_session=os.name=='posix')
        message('Saved native revision frozen: independent audit is running while display renders finish.')
    try:
        if not recipe.is_file():
            raise RuntimeError(f'Project has no build.py recipe: {brief.parent}')
        pdf = None
        if args.pdf_manifest:
            from pdf_context import snapshot_pdf
            with timing.span('pdf_freeze'):
                pdf = snapshot_pdf(args.pdf_manifest, output)
            metadata['pdf_sha256']=pdf['sha256']
            metadata['pdf_pages']=pdf['pages']
            artifact(output/'pdf-source/manifest.json', 'Frozen PDF source evidence')
            artifact(output/'pdf-source/source.pdf', 'Attached PDF')
            message(f"Reading all {pdf['pages']} PDF pages as images plus extracted text. Unsupported requirements stop before CAD generation.")
        message('Asking the signed-in Astra subscription for bounded airframe parameters. Hardware and engineering readiness remain separately checked.')
        with timing.span('planning'):
            design, metadata['planner_submission_sha256'] = plan(ROOT, brief, args.request, output, appearance=True, pdf=pdf,timing=timing)
        if pdf is not None: artifact(output/'pdf-interpretation.json', 'PDF requirements and parameter evidence')
        artifact(design, 'Astra airframe parameters')
        artifact(output/'design-review.json', 'Design rationale, alternatives and unverified checks')
        artifact(output/'planning/submission.json', 'Astra planning rationale')
        artifact(output/'model-request.json', 'Recorded model request')
        package = brief.parent/'package-v1'
        command = [str(ROOT/'cad/freecad/run.sh'), str(recipe), '--output-dir', str(output/'cad'), '--design', str(design)]
        if args.cad_first:command += ['--demo-display']
        if package.is_dir():
            snapshot = output/'hardware-evidence'
            shutil.copytree(package, snapshot)
            metadata['hardware_evidence_sha256'] = {
                str(path.relative_to(snapshot)): hashlib.sha256(path.read_bytes()).hexdigest()
                for path in sorted(snapshot.rglob('*')) if path.is_file()
            }
            command += ['--package', str(snapshot)]
            readiness=json.loads((snapshot/'requirements.json').read_text())
            metadata['hardware_ready']=readiness.get('hardware_ready') is True
            if not metadata['hardware_ready']:
                message('The hardware package is not qualified. This run builds a provisional layout and keeps the complete hardware-freeze requirement open.')
            for path in sorted(snapshot.iterdir()):
                if path.is_file() and path.suffix in {'.json', '.csv', '.md'}:
                    artifact(path, 'Hardware evidence: '+path.name)
        metadata['recipe_sha256'] = hashlib.sha256(recipe.read_bytes()).hexdigest()
        message('Building editable geometry. Hardware readiness and analysis results are reported independently.')
        with timing.span('freecad_build') as build_span:
            returncode = build_with_progress(command, output, message,timing=timing,on_render=publish_render,on_ready=None if args.cad_first else start_audit,on_tick=publish_audit)
            build_span.update(return_code=returncode,status='succeeded' if returncode==0 else 'failed')
        artifact(output/'freecad.log', 'FreeCAD execution log')
        if returncode:
            raise RuntimeError(f'FreeCAD exited {returncode}; see freecad.log')
        cad = output/'cad'
        native = cad/'assembly.FCStd'
        if not native.is_file():
            raise RuntimeError('Recipe completed without assembly.FCStd')
        for path in sorted(cad.iterdir()):
            if path.is_file() and path.suffix.lower() in {'.fcstd','.step','.json','.csv','.png','.stl','.fcmacro','.py','.md'}:
                if path.suffix.lower()=='.png':publish_render(path)
                else:artifact(path,path.name)
        if args.cad_first:
            ready = cad_ready(cad, timing.stamp()['elapsed_s'])
            (output/'cad-ready.json').write_text(json.dumps(ready, indent=2))
            artifact(output/'cad-ready.json', 'CAD readiness milestone; engineering verification pending')
            message(f"Editable CAD and all six native views ready in {ready['elapsed_s']:.2f}s. Full engineering audit is pending and continues now.")
            timing.event('cad_ready', ready_elapsed_s=ready['elapsed_s'], native_sha256=ready['native_sha256'])
            start_audit(cad/'artifact-manifest.json')
        message('Checking actual native geometry against the selected inverted-V layout, winglets and rear pusher.')
        layout_report = cad/'independent-layout.json'
        layout_command = [str(ROOT/'cad/freecad/run.sh'), str(ROOT/'evals/cad/vtol_demonstrator/layout_acceptance.py'), str(native), str(cad/'artifact-manifest.json'), str(layout_report)]
        with timing.span('layout_check') as layout_span:
            with (output/'layout-check.log').open('w') as log:
                checked = subprocess.run(layout_command, cwd=ROOT, stdout=log, stderr=subprocess.STDOUT, timeout=300)
            layout_span.update(return_code=checked.returncode,status='succeeded' if checked.returncode==0 else 'failed')
        artifact(output/'layout-check.log', 'Native layout acceptance log')
        if layout_report.is_file(): artifact(layout_report, 'Independent native layout acceptance')
        if checked.returncode or not layout_report.is_file():
            raise RuntimeError('Native model does not pass the selected layout contract; inspect layout-check.log.')
        layout = json.loads(layout_report.read_text())
        if layout.get('passed') is not True or layout.get('source_unchanged') is not True or layout.get('native_sha256') != hashlib.sha256(native.read_bytes()).hexdigest():
            raise RuntimeError('Native layout acceptance failed or identifies another revision')
        metadata['layout_acceptance']='passed'
        message('Native inverted-V layout and retained winglets/pusher passed focused acceptance. Aircraft qualification remains separate.')
        if sys.platform == 'darwin':
            app = Path.home()/'Applications/FreeCAD.app'
            if not app.exists(): app = Path('/Applications/FreeCAD.app')
            if app.exists():
                launcher = cad/'open.FCMacro'
                launch_args = ['open','-n','-a',str(app),'--args',str(launcher)] if launcher.is_file() else ['open','-a',str(app),str(native)]
                with timing.span('gui_launch'):
                    subprocess.run(launch_args, check=True, timeout=15)
                message('Requested FreeCAD to open the saved native assembly.')
        message('Running independent checks against this saved assembly revision. Missing hardware and engineering evidence remain open.')
        audit_command = [sys.executable, str(ROOT/'evals/cad/vtol_demonstrator/run.py'), str(native), str(cad/'artifact-manifest.json'), str(output/'audit')]
        if background_audit is None:
            start_audit(cad/'artifact-manifest.json')
        audit_process = audit_with_progress(audit_command,output,message,prestarted=background_audit,publisher=publish_audit)
        timing.end(audit_timing,'succeeded' if audit_process.returncode==0 else 'failed',return_code=audit_process.returncode)
        audit_timing=None
        artifact(output/'audit.log', 'Independent audit log')
        report_path = output/'audit/result.json'
        if audit_process.returncode or not report_path.is_file():
            raise RuntimeError('Independent audit did not complete; inspect audit.log. Native CAD is retained.')
        report = json.loads(report_path.read_text())
        if report.get('native_sha256') != hashlib.sha256(native.read_bytes()).hexdigest():
            raise RuntimeError('Audit does not identify the current native assembly')
        artifact(report_path, 'Current-revision engineering checks')
        for name, gate in report['gates'].items():
            message(f"Check {name}: {gate['status']}. {gate.get('reason') or gate['requirement']}")
        metadata['audit_statuses'] = {name: gate['status'] for name, gate in report['gates'].items()}
        for name in ('gui-open.json', 'native-viewport.png'):
            path=cad/name
            if path.is_file():
                if path.suffix=='.png':publish_render(path,'FreeCAD viewport')
                else:artifact(path,name)
        outcomes = check_outcomes(report)
        passed = sum(gate['status']=='passed' for gate in report['gates'].values())
        emit('checks', passed=passed, failed=len(outcomes)-passed, outcomes=outcomes)
        metadata['status']='geometry_built_verification_pending'
        emit('run_finished', passed=False, objective=0, objective_label='Native assembly built; engineering qualification pending')
    except SystemExit:
        metadata.update(status='cancelled')
        raise
    except Exception as exc:
        metadata.update(status='failed', error=str(exc))
        timing.event('run_error',error_class=type(exc).__name__)
        if 'planner_submission_sha256' not in metadata and (output/'pdf-interpretation.json').is_file():
            artifact(output/'pdf-interpretation.json','Rejected PDF interpretation and requirements')
        message(str(exc))
        emit('run_finished', passed=False, objective=0, objective_label='Project execution failed')
        raise
    finally:
        signal.signal(signal.SIGTERM,signal.SIG_IGN)
        if background_audit is not None:
            if background_audit.poll() is None:
                if os.name=='posix':
                    try:os.killpg(background_audit.pid,signal.SIGKILL)
                    except ProcessLookupError:pass
                else:background_audit.kill()
            background_audit.wait()
        signal.signal(signal.SIGTERM,previous_term)
        metadata['finished_at']=time.time()
        (output/'request.json').write_text(json.dumps(metadata, indent=2))
        artifact(output/'request.json','Execution provenance')
        timing.finish(metadata['status'])
        artifact(timing.path,'Observed action timing trace')
        artifact(timing.summary,'Observed action timing summary')

if __name__ == '__main__':
    main()
