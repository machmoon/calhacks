"""Real native fixture -> fixed open-source runner -> managed artifacts, read-only."""
import hashlib,json
from pathlib import Path
import worker,wing_analysis
from unittest.mock import patch
from subprocess import TimeoutExpired
ROOT=Path(__file__).resolve().parents[2];source=ROOT/'out/freecad-aero-sections/fixture.FCStd';out=ROOT/'out/native-wing-tool-test'
digest=hashlib.sha256(source.read_bytes()).hexdigest();worker.document('WingTool');worker.open_document(str(source));doc=worker.active()
before=worker.geometry_fingerprint([o for o in doc.Objects if hasattr(o,'Shape')])
for args in [dict(velocity_m_s=101),dict(alphas_deg=[1,1]),dict(symmetric=1),dict(samples_per_branch=True)]:
    try:worker.METHODS['analyze_wing'](directory=str(out),object_name='Wing',**args)
    except ValueError:pass
    else:raise AssertionError('invalid analysis option accepted')
result=worker.METHODS['analyze_wing'](directory=str(out),object_name='Wing',alphas_deg=[0,4],samples_per_branch=33)
assert result['status']=='analysis_complete',result
assert result['native_document_changed'] is False and result['section_count']==3
assert all(name in result['solvers'] for name in ['aerosandbox','optvl','neuralfoil'])
assert len(result['rows'])==2 and len(result['mesh_convergence'])==2 and len(result['cross_solver'])==2
assert [Path(a['path']).name for a in result['artifacts']]==['analysis.json','wing-analysis.png','panel-loads.csv']
assert all(Path(a['path']).is_relative_to(out) and Path(a['path']).is_file() for a in result['artifacts'])
assert before==worker.geometry_fingerprint([o for o in doc.Objects if hasattr(o,'Shape')])
assert hashlib.sha256(source.read_bytes()).hexdigest()==digest
with patch.object(wing_analysis.subprocess,'run',side_effect=TimeoutExpired('fixed wing runner',240)) as process:
    failure=worker.METHODS['analyze_wing'](directory=str(out),object_name='Wing',alphas_deg=[4])
    assert process.call_args.kwargs['timeout']==240 and '-I' in process.call_args.args[0]
assert failure['status']=='analysis_failed' and failure['passed'] is False
assert failure['native_document_changed'] is False
assert all(Path(a['path']).is_file() for a in failure['artifacts'])
assert before==worker.geometry_fingerprint([o for o in doc.Objects if hasattr(o,'Shape')])
(out/'test-result.json').write_text(json.dumps(dict(passed=True,source_file_unchanged=True,native_geometry_unchanged=True,invalid_options_rejected=True,timeout_failure_retained=True,result=result),indent=2))
print('Typed native wing analysis actual solvers, bounded options, artifacts and read-only geometry pass')
