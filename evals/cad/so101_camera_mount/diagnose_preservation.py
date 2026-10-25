"""Observe original model's failing snapshot; never patch its construction code."""
import sys,json,importlib.util,hashlib
from pathlib import Path
import FreeCAD as App
candidate=Path(sys.argv[1]).resolve();source=Path(sys.argv[2]).resolve();out=Path(sys.argv[3]).resolve();out.mkdir(parents=True,exist_ok=True)
identity=hashlib.sha256(candidate.read_bytes()).hexdigest();captured={}
def trace(frame,event,arg):
 if frame.f_code.co_filename==str(candidate) and frame.f_code.co_name=='build' and event=='exception':
  local=frame.f_locals
  if 'before' in local and 'snapshot' in local:
   deltas={}
   for obj in local.get('original',[]):
    before=local['before'][obj.Name];after=local['snapshot'](obj)
    changes={k:{'before':before.get(k),'after':after.get(k)} for k in set(before)|set(after) if before.get(k)!=after.get(k)}
    if changes:deltas[obj.Name]=changes
   captured.update(exception=str(arg[1]),deltas=deltas)
 return trace
spec=importlib.util.spec_from_file_location('unchanged_camera_candidate',candidate);module=importlib.util.module_from_spec(spec);spec.loader.exec_module(module)
sys.settrace(trace)
try:module.build(str(source),str(out/'unsubmitted-build'));captured['unexpected_success']=True
except Exception as error:captured['error']=str(error)
finally:sys.settrace(None)
captured['candidate_sha256']=identity;captured['candidate_unchanged']=hashlib.sha256(candidate.read_bytes()).hexdigest()==identity
(out/'snapshot-deltas.json').write_text(json.dumps(captured,indent=2));print(json.dumps({'error':captured.get('error'),'changed_objects':len(captured.get('deltas',{}))}))
