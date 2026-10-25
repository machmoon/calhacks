"""Separate expensive native cut-material audit after mechanical checks."""
import hashlib,json,os,time
from pathlib import Path
import FreeCAD as App
import native
ROOT=Path(__file__).resolve().parents[2];OUT=ROOT/os.environ.get('WRIGHT_JOINT_OUTPUT','out/tsung-vtol-wing-pod-joint-native')
p=OUT/'assembly.FCStd';report=json.loads((OUT/'result.json').read_text())
assert report['status']=='mechanical checks complete; exact cut audit pending'
digest=hashlib.sha256(p.read_bytes()).hexdigest();start=time.time();doc=App.openDocument(str(p))
try:
 evidence=native.validate_document(doc)
 assert hashlib.sha256(p.read_bytes()).hexdigest()==digest
 report.update(whole_document_audit='passed',status='complete',exact_cut_audit_elapsed_seconds=time.time()-start)
 (OUT/'exact-audit.json').write_text(json.dumps(dict(passed=True,native_sha256=digest,objects=len(evidence),elapsed_seconds=time.time()-start),indent=2))
except Exception as error:
 report.update(whole_document_audit='failed',status='exact cut audit failed',exact_cut_audit_error=str(error))
 (OUT/'result.json').write_text(json.dumps(report,indent=2));raise
(OUT/'result.json').write_text(json.dumps(report,indent=2));print('EXACT CUT AUDIT COMPLETE',flush=True)
