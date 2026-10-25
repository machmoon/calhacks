"""Reopen-only completion after correcting a stale expected cavity-section count.

Does not rebuild or alter CAD, or rerun already-passed all-part/sweep checks.
"""
import hashlib,json
from pathlib import Path
import FreeCAD as App
import worker
ROOT=Path(__file__).resolve().parents[2];OUT=ROOT/'out/tsung-vtol-smooth-aircraft-final'
report=json.loads((OUT/'result.json').read_text())
assert report['fit']['passed'] and report['cavity']['true_minimum_wall_mm']>=.8
assert report['servo_static_fit']['actual_wall_gap_mm']>=.5
assert len(report['rotor_vs_changed_wing'])==80
source=ROOT/'out/tsung-vtol-smooth-wing-actual-sections/assembly.FCStd'
assert hashlib.sha256(source.read_bytes()).hexdigest()==report['source_sha256'][str(source)]
original=App.openDocument(str(source));expected={n:len(original.getObject(n).Sections) for n in ['RightOuterContour','RightCavity']};App.closeDocument(original.Name)
path=OUT/'assembly.FCStd';digest=hashlib.sha256(path.read_bytes()).hexdigest()
worker.open_document(str(path));doc=worker.active()
actual={n:len(doc.getObject(n).Sections) for n in expected};assert actual==expected
assert {n:bool(doc.getObject(n).Ruled) for n in expected}==report['native_loft_ruled']
assert doc.RightSkin.Base==doc.RightOuterContour and doc.RightSkin.Tool==doc.RightCavity
for i in range(6):
 assert doc.getObject('RightRib'+str(i)).Base.Base==doc.RightCavity
 assert doc.getObject('LeftRib'+str(i)).Source==doc.getObject('RightRib'+str(i))
assert ('HalfSpan','ReferenceDimensions.HalfSpan') in list(doc.WingControls.ExpressionEngine)
assert doc.getObject('WingRight') is None and doc.getObject('WingLeft') is None
assert all(hashlib.sha256(Path(p).read_bytes()).hexdigest()==h for p,h in report['source_sha256'].items())
assert hashlib.sha256(path.read_bytes()).hexdigest()==digest
correction=dict(reason='Generator expected8cavity sections from obsolete prototype; actual verified source has16',
    expected_source_section_counts=expected,actual_reopened_section_counts=actual,
    repeated_geometry_checks=False,native_unchanged_sha256=digest)
report.update(source_section_counts=expected,reopened_native_dependencies_passed=True,status='complete',reopen_assertion_correction=correction)
wing_evidence=source.parent/'result.json'
report['wing_edit_evidence']=dict(path=str(wing_evidence),sha256=hashlib.sha256(wing_evidence.read_bytes()).hexdigest())
(OUT/'reopen-completion.json').write_text(json.dumps(correction,indent=2))
(OUT/'result.json').write_text(json.dumps(report,indent=2))
print('COMPLETE: saved native dependency graph and source-derived section counts verified',flush=True)
