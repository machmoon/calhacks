"""Display-only views of the checked wing-root joint; no geometry mutation."""
import os,hashlib,json
from pathlib import Path
import FreeCAD as App
import worker,surface_preview
ROOT=Path(__file__).resolve().parents[2];OUT=ROOT/os.environ.get('WRIGHT_JOINT_OUTPUT','out/tsung-vtol-wing-pod-joint')
report=json.loads((OUT/'result.json').read_text());assert report['status'] in ('complete','mechanical checks complete; exact cut audit pending')
p=OUT/'assembly.FCStd';digest=hashlib.sha256(p.read_bytes()).hexdigest()
doc=App.openDocument(str(p));worker._doc=doc;objects=[doc.getObject(n) for n in report['physical']]
views={}
views['exterior']=surface_preview.selected(objects,OUT/'aircraft-joint.png',width=640,height=480)
# Display-only removal of wing skins exposes the final drilled pod and hardware.
selected=[doc.getObject(report['pod_result'])]
selected += [doc.getObject(n) for n in report['physical'] if n.startswith(('WJScrew','WJNut'))]
views['pod_and_hardware']=surface_preview.selected(selected,OUT/'pod-and-hardware.png',width=640,height=480,elevation=-25)
views['pod_and_hardware']['scope']='Final physical pod and hardware; wing hidden only for display. No construction stock or access envelopes exported.'
assert hashlib.sha256(p.read_bytes()).hexdigest()==digest
(OUT/'preview.json').write_text(json.dumps(dict(native_sha256=digest,native_unchanged=True,audit_status=report['status'],views=views),indent=2))
