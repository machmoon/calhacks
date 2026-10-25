"""Resume display/reopen verification after a bounded render-budget rejection."""
import base64,hashlib,json
from pathlib import Path
import FreeCAD as App
import native,worker
ROOT=Path(__file__).resolve().parents[2];out=ROOT/'out/tsung-vtol-lift-mount';r=json.loads((out/'checks-before-export.json').read_text());path=out/'assembly.FCStd';digest=hashlib.sha256(path.read_bytes()).hexdigest()
worker.open_document(str(path));doc=worker.active()
image=worker.render(r['physical'],deflection_mm=10.0,max_triangles=100000)
(out/'aircraft.png').write_bytes(base64.b64decode(image.pop('content_blocks')[0]['source']['data']));(out/'render.json').write_text(json.dumps(image,indent=2))
r['exports']={k:str(out/v) for k,v in [('native','assembly.FCStd'),('step','assembly.step'),('glb','assembly.glb')]}
assert all(Path(p).is_file() for p in r['exports'].values())
r['step_validation']=json.loads((out/'assembly.step.validation.json').read_text());r['render_retry']=dict(initial_failure='default 50k and explicit 1mm/100k triangle budgets exceeded',successful_request=dict(deflection_mm=10,max_triangles=100000,width=640,height=480))
local=[n for n in r['physical'] if n not in ['WingRight','WingLeft','Pod','PusherMotor']]
native.set_parameter(doc,'ReferenceDimensions','BoomHalfSpacing',230)
fit=worker.check(local,r['contacts'],tolerance_mm=.001,require_connected=True);assert fit['passed']
assert hashlib.sha256(path.read_bytes()).hexdigest()==digest
assert hashlib.sha256((ROOT/'out/tsung-vtol-top-retention/assembly.FCStd').read_bytes()).hexdigest()==r['source_sha256']
r.update(saved_reopen_spacing_edit_passed=True,source_preserved=True)
(out/'result.json').write_text(json.dumps(r,indent=2));print('COMPLETE',flush=True)
