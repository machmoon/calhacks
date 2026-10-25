"""Native host invocation of an unchanged submitted build module; records real stages."""
import hashlib,importlib.util,json,sys,time,traceback
from pathlib import Path
import FreeCAD as App
candidate,source,out=map(lambda x:Path(x).resolve(),sys.argv[1:4]);out.mkdir(parents=True,exist_ok=True)
if (out/'host-execution-started.json').exists():raise ValueError('host execution already attempted here')
sha=lambda p:hashlib.sha256(p.read_bytes()).hexdigest()
identity={'candidate_sha256':sha(candidate),'source_sha256':sha(source),'execution':'headless native FreeCAD; not GUI capture'}
(out/'host-execution-started.json').write_text(json.dumps(identity,indent=2));start=time.monotonic()
def checkpoint(label):
 doc=App.ActiveDocument
 event={'label':label,'elapsed_seconds':time.monotonic()-start,'object_count':len(doc.Objects) if doc else None,'objects':[o.Name for o in doc.Objects] if doc else []}
 with (out/'native-stages.jsonl').open('a') as f:f.write(json.dumps(event)+'\n')
 print(label,flush=True)
status=dict(identity)
try:
 spec=importlib.util.spec_from_file_location('model_camera_mount',candidate);module=importlib.util.module_from_spec(spec);spec.loader.exec_module(module)
 status.update(returned=module.build(str(source),str(out),checkpoint),execution_passed=True)
except Exception as error:status.update(execution_passed=False,error=str(error),traceback=traceback.format_exc())
status.update(elapsed_seconds=time.monotonic()-start,candidate_unchanged=sha(candidate)==identity['candidate_sha256'],source_unchanged=sha(source)==identity['source_sha256'],independent_geometry_score_pending=True)
(out/'host-execution-result.json').write_text(json.dumps(status,indent=2));print(json.dumps(status,indent=2));sys.exit(0 if status['execution_passed'] else 1)
