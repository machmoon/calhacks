"""Actual 11-section aircraft wing through the public typed analysis action."""
import hashlib,json
from pathlib import Path
import worker
ROOT=Path(__file__).resolve().parents[2];source=ROOT/'out/tsung-vtol-smooth-aircraft-final/assembly.FCStd';out=ROOT/'out/native-wing-tool-actual'
digest=hashlib.sha256(source.read_bytes()).hexdigest();worker.document('ActualWingAnalysis');worker.open_document(str(source));doc=worker.active()
wing=doc.getObject('RightOuterContour');assert wing is not None and len(wing.Sections)==11
before=worker.geometry_fingerprint([wing]+list(wing.Sections))
result=worker.METHODS['analyze_wing'](directory=str(out),object_name=wing.Name,velocity_m_s=20,alphas_deg=[0,4,8],samples_per_branch=65)
assert result['status']=='analysis_complete',result
assert result['section_count']==11 and result['native_document_changed'] is False
assert before==worker.geometry_fingerprint([wing]+list(wing.Sections))
assert hashlib.sha256(source.read_bytes()).hexdigest()==digest
(out/'test-result.json').write_text(json.dumps(dict(passed=True,source=str(source),source_file_sha256=digest,source_file_unchanged=True,native_wing_geometry_unchanged=True,result=result),indent=2))
print('Actual11section native wing typed analysis complete; CAD/file unchanged; inspect numerical criteria and scope independently')
