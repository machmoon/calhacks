"""Bounded read-only native-wing analysis through the fixed open-source runner."""
import hashlib,json,math,os,re,subprocess,uuid
from pathlib import Path
ROOT=Path(__file__).resolve().parents[2]
RUNNER=ROOT/'crates/wright-cfd/py/native_wing_analysis.py'
CFD_PYTHON=Path('/private/tmp/claude-501/-Users-patliu/e9f305f9-7d6c-4ecb-a4fc-7e5345001431/scratchpad/venv312/bin/python')
TIMEOUT_SECONDS=240


def options(object_name,velocity_m_s=20,alphas_deg=None,altitude_m=0,samples_per_branch=65,symmetric=True):
    if not isinstance(object_name,str) or not re.fullmatch(r'[A-Za-z][A-Za-z0-9_]{0,95}',object_name):raise ValueError('invalid wing object name')
    def number(value,low,high,label):
        if isinstance(value,bool) or not isinstance(value,(int,float)) or not math.isfinite(value) or not low<=value<=high:raise ValueError(f'{label} must be finite in {low}..{high}')
        return float(value)
    velocity=number(velocity_m_s,1,100,'velocity_m_s');altitude=number(altitude_m,0,20000,'altitude_m')
    alphas=[0,4,8] if alphas_deg is None else alphas_deg
    if not isinstance(alphas,list) or not 1<=len(alphas)<=5:raise ValueError('alphas_deg must contain 1..5 values')
    alphas=[number(a,-10,12,'alpha') for a in alphas]
    if len(set(alphas))!=len(alphas):raise ValueError('alphas must be unique')
    if type(samples_per_branch) is not int or not 33<=samples_per_branch<=129:raise ValueError('samples_per_branch must be an integer in 33..129')
    if not isinstance(symmetric,bool):raise ValueError('symmetric must be boolean')
    return dict(object_name=object_name,velocity_m_s=velocity,alphas_deg=alphas,altitude_m=altitude,samples_per_branch=samples_per_branch,symmetric=symmetric)


def python():
    persistent=Path.home()/'.wright/venv312/bin/python'
    fallback=persistent if persistent.is_file() and os.access(persistent,os.X_OK) else CFD_PYTHON
    selected=Path(os.environ.get('WRIGHT_ANALYSIS_PYTHON') or os.environ.get('WRIGHT_PYTHON') or fallback).expanduser()
    if not selected.is_file() or not os.access(selected,os.X_OK):raise ValueError('analysis Python unavailable; configure WRIGHT_ANALYSIS_PYTHON with AeroSandbox, OptVL and NeuralFoil installed')
    return selected.absolute()


def analyze(doc,directory,object_name,velocity_m_s=20,alphas_deg=None,altitude_m=0,samples_per_branch=65,symmetric=True):
    values=options(object_name,velocity_m_s,alphas_deg,altitude_m,samples_per_branch,symmetric)
    import aero_sections
    geometry=aero_sections.extract(doc,object_name,values['samples_per_branch'],values['symmetric'])
    payload=dict(geometry=geometry,velocity_m_s=values['velocity_m_s'],alphas_deg=values['alphas_deg'],altitude_m=values['altitude_m'])
    encoded=json.dumps(payload,allow_nan=False).encode()
    if len(encoded)>1024*1024:raise ValueError('extracted wing input exceeds 1 MiB')
    executable=python()
    if not RUNNER.is_file():raise ValueError('fixed native wing analysis runner is unavailable')
    out=Path(directory).resolve()/('wing-analysis-'+uuid.uuid4().hex[:12]);out.mkdir(parents=True,exist_ok=False)
    input_path=out/'input.json';input_path.write_bytes(encoded)
    provenance=dict(source_geometry_sha256=geometry['source_geometry_sha256'],input_sha256=hashlib.sha256(encoded).hexdigest(),runner_sha256=hashlib.sha256(RUNNER.read_bytes()).hexdigest(),python=str(executable),timeout_seconds=TIMEOUT_SECONDS,options=values)
    (out/'provenance.json').write_text(json.dumps(provenance,indent=2))
    environment=dict(os.environ,OMP_NUM_THREADS='1',OPENBLAS_NUM_THREADS='1',MKL_NUM_THREADS='1')
    log=out/'solver.log'
    def unchanged():return aero_sections.extract(doc,object_name,values['samples_per_branch'],values['symmetric'])==geometry
    try:
        with log.open('wb') as stream:
            completed=subprocess.run([str(executable),'-I',str(RUNNER),str(input_path),str(out)],cwd=out,env=environment,stdout=stream,stderr=subprocess.STDOUT,timeout=TIMEOUT_SECONDS,check=False)
        if not unchanged():raise ValueError('native wing geometry changed during analysis')
        if completed.returncode!=0:raise ValueError(f'open-source wing solver exited {completed.returncode}')
        result_path=out/'analysis.json'
        if not result_path.is_file() or not 1<=result_path.stat().st_size<=2*1024*1024:raise ValueError('analysis JSON missing or exceeds 2 MiB')
        def reject_constant(value):raise ValueError('nonfinite solver JSON constant '+value)
        result=json.loads(result_path.read_text(),parse_constant=reject_constant)
        if result.get('status')!='analysis_complete':raise ValueError('solver did not complete analysis')
        if any(not isinstance(result.get('solvers',{}).get(name),str) for name in ['aerosandbox','optvl','neuralfoil']):raise ValueError('required open-source solver evidence missing')
        if result.get('runner_sha256')!=provenance['runner_sha256']:raise ValueError('solver script provenance mismatch')
        if result.get('geometry',{}).get('source_geometry_sha256')!=geometry['source_geometry_sha256']:raise ValueError('solver geometry provenance mismatch')
        artifacts=[]
        for name,label,limit in [('analysis.json','Native wing analysis (JSON)',2*1024*1024),('wing-analysis.png','Wing analysis plot',10*1024*1024),('panel-loads.csv','Wing panel loads (CSV)',5*1024*1024)]:
            path=out/name
            if not path.is_file() or not 1<=path.stat().st_size<=limit:raise ValueError('missing or oversized analysis artifact '+name)
            artifacts.append(dict(label=label,path=str(path)))
        summary={key:result[key] for key in ['status','solvers','operating_point','rows','mesh_convergence','cross_solver','section_polars','references','sampled_section_frame_error_mm','scope'] if key in result}
        summary.update(geometry={key:geometry[key] for key in ['object_name','source_geometry_sha256','native_loft_ruled','symmetric','axes','scope']},
            section_count=len(geometry['sections']),numerical_checks_passed=all(r.get('passed') is True for key in ['mesh_convergence','cross_solver'] for r in result.get(key,[])),artifacts=artifacts,provenance=provenance,
            native_document_changed=False,scope_limit='Wing-only reduced model; no full-aircraft stability, viscous stall, structural or flight approval')
        if len(json.dumps(summary,allow_nan=False).encode())>256*1024:raise ValueError('analysis summary exceeds 256 KiB')
        return summary
    except (ValueError,OSError,subprocess.TimeoutExpired) as error:
        try:changed=not unchanged()
        except Exception:changed=None
        failure=dict(status='analysis_failed',passed=False,error=str(error),provenance=provenance,native_document_changed=changed,
            scope='No successful wing-analysis claim; retained input/log for diagnosis',artifacts=[dict(label='Wing analysis input',path=str(input_path)),dict(label='Wing analysis solver log',path=str(log))])
        (out/'failure.json').write_text(json.dumps(failure,indent=2));return failure
