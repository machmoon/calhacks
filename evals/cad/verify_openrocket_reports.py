"""Verify the baseline/negative-control evidence without claiming full CAD coverage."""
import json,hashlib
from pathlib import Path
root=Path(__file__).resolve().parents[2]
paths=[root/'out/freecad-openrocket-verified/result.json',root/'out/freecad-openrocket-negative-control/result.json']
baseline,negative=[json.loads(p.read_text()) for p in paths]
assert baseline['evaluator_sha256']==negative['evaluator_sha256']
assert baseline['workbench_patch_sha256']==negative['workbench_patch_sha256']
assert baseline['workbench_python_sha256']==negative['workbench_python_sha256']
assert baseline['exchange_sha256']==negative['exchange_sha256']
assert not baseline['full_cad_benchmark_passed'] and not negative['full_cad_benchmark_passed']
checks=0
for b,n in zip(baseline['imports'],negative['imports']):
    assert b['source']==n['source']
    if b['import_passed']:
        assert b['independent_checks_count']>0 and b['independent_checks_passed'] is True
        assert b['exchange']['passed'] is True
        assert not b['component_coverage_passed'] and any(e['status']=='ignored' for e in b['component_coverage'])
        assert n['negative_control']['after_mm']-n['negative_control']['before_mm']==1
        assert n['independent_checks_passed'] is False
        assert any(c['check']=='analytic tube material volume' and not c['passed'] for e in n['component_coverage'] for c in e.get('independent_checks',[]))
        checks+=b['independent_checks_count']
    else:
        assert b['independent_checks_count']==0 and b['independent_checks_passed'] is None
        assert all(e['status']=='not_visited' for e in b['component_coverage'])
spacing_path=root/'out/freecad-openrocket-negative-spacing/result.json'
spacing=json.loads(spacing_path.read_text())
assert spacing['evaluator_sha256']==baseline['evaluator_sha256']
assert spacing['workbench_python_sha256']==baseline['workbench_python_sha256']
spacing_detected=0
for b,n in zip(baseline['imports'],spacing['imports']):
    assert b['source']==n['source'] and n['import_passed']==b['import_passed']
    if 'negative_control' in n:
        assert n['negative_control']['property']=='InstanceSeparation'
        assert n['independent_checks_passed'] is False
        assert any(c['check']=='rail button axial pitch' and not c['passed'] for e in n['component_coverage'] for c in e.get('independent_checks',[]))
        spacing_detected+=1
    else:
        assert not any(e['tag']=='railbutton' for e in n['component_coverage'])
        assert n['independent_checks_passed'] is True
paths.append(spacing_path)
report=dict(evidence_verified=True,numerical_checks=checks,negative_controls_detected=sum(i['import_passed'] for i in baseline['imports'])+spacing_detected,source_reports={str(p.relative_to(root)):hashlib.sha256(p.read_bytes()).hexdigest() for p in paths},full_cad_benchmark_passed=False)
(root/'out/freecad-openrocket-verified/evidence.json').write_text(json.dumps(report,indent=2))
print(json.dumps(report,indent=2))
