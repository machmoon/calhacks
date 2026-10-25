"""Diagnostic phase timings; does not change production action deadlines."""
import json,sys,time,hashlib
from pathlib import Path
import FreeCAD as App
import Part,Import
import step_import,native
source=Path(sys.argv[1]).resolve();out=Path(sys.argv[2]).resolve();out.mkdir(parents=True,exist_ok=True)
start=time.monotonic();trace=(out/'phases.jsonl').open('w');counts={}
def event(phase,state,**fields):
    trace.write(json.dumps(dict(phase=phase,event=state,elapsed_seconds=time.monotonic()-start,**fields))+'\n');trace.flush()
def wrap(module,name,phase):
    original=getattr(module,name)
    def measured(*args,**kwargs):
        counts[phase]=counts.get(phase,0)+1;seq=counts[phase];t=time.monotonic();event(phase,'start',sequence=seq)
        try:
            value=original(*args,**kwargs);event(phase,'end',sequence=seq,duration_seconds=time.monotonic()-t);return value
        except Exception as error:event(phase,'error',sequence=seq,error=str(error));raise
    setattr(module,name,measured)
for module,name,phase in [(step_import,'declared_units','units'),(Part,'read','native_read'),(Import,'insert','label_reimport'),(step_import,'_labels','labels'),(step_import,'_signature','signature'),(native,'validate_document','document_validation'),(native,'describe','describe')]:wrap(module,name,phase)
doc=App.newDocument('DiagnosticImport');doc.UndoMode=1;digest=hashlib.sha256(source.read_bytes()).hexdigest();event('import','start')
try:
    result=step_import.import_step(doc,str(source),'JetsonDiagnostic',preserve_reference_geometry=True,max_source_mib=80,max_entities=2048,summary=True)
    elapsed=time.monotonic()-start;event('import','end',duration_seconds=elapsed)
    doc.saveAs(str(out/'diagnostic.FCStd'))
    (out/'result.json').write_text(json.dumps(dict(status='diagnostic_complete',within_production_deadline=elapsed<300,elapsed_seconds=elapsed,source_unchanged=hashlib.sha256(source.read_bytes()).hexdigest()==digest,result=result),indent=2))
except Exception as error:
    event('import','error',error=str(error));raise
finally:trace.close()
