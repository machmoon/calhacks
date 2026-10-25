"""Read-only timing of aggregate and leaf STEP validity; no import pass claim."""
import json,sys,time
from pathlib import Path
import FreeCAD
import Part
import step_import
source=Path(sys.argv[1]);out=Path(sys.argv[2]);out.mkdir(parents=True,exist_ok=True)
start=time.monotonic()
with (out/'validation-phases.jsonl').open('w') as log:
    def measure(phase,operation,**metadata):
        stamp=time.monotonic();log.write(json.dumps(dict(phase=phase,event='start',elapsed_seconds=stamp-start,**metadata))+'\n');log.flush()
        value=operation();log.write(json.dumps(dict(phase=phase,event='end',elapsed_seconds=time.monotonic()-start,duration_seconds=time.monotonic()-stamp,result=value,**metadata))+'\n');log.flush();return value
    holder={}
    measure('read',lambda:holder.update(shape=Part.read(str(source))))
    shape=holder['shape']
    measure('aggregate_valid',shape.isValid)
    entities=step_import._entities(shape,True)
    for index,(leaf,role) in enumerate(entities):
        measure('leaf_valid',leaf.isValid,index=index,role=role)
        measure('leaf_measure',lambda:leaf.Volume if role=='physical_solid' else leaf.Area,index=index,role=role)
