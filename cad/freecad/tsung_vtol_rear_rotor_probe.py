"""Measure a proposed rear rotor shift against actual saved fin BReps."""
import hashlib,json,math
from pathlib import Path
import FreeCAD as App
import native,worker
ROOT=Path(__file__).resolve().parents[2];OUT=ROOT/'out/tsung-vtol-rear-rotor-layout';OUT.mkdir(parents=True,exist_ok=True)
SOURCE=ROOT/'out/tsung-vtol-lift-mount/assembly.FCStd';digest=hashlib.sha256(SOURCE.read_bytes()).hexdigest();worker.open_document(str(SOURCE));doc=worker.active()
previous=json.loads((SOURCE.parent/'result.json').read_text())
inspection=[]
for code in ['RR','RL']:
 for role,name in previous['instances'][code].items():
  o=doc.getObject(name);inspection.append(dict(object=name,type=o.TypeId,source=o.Source.Name,normal=list(o.Normal),origin=list(o.Base),expressions=o.ExpressionEngine))
def clearance(shift):
 values=[]
 for code,fin in [('RR','FinTopRetentionRight'),('RL','FinTopRetentionLeft')]:
  shape=doc.getObject('Sweep'+code).Shape.copy();shape.translate(App.Vector(-shift,0,0));values.append(shape.distToShape(doc.getObject(fin).Shape)[0])
 return min(values)
results=[]
for spacing in [220,230]:
 native.set_parameter(doc,'ReferenceDimensions','BoomHalfSpacing',spacing)
 lo,hi=0.,80.;assert clearance(hi)>5
 initial=clearance(0)
 for _ in range(16):
  mid=(lo+hi)/2
  if clearance(mid)>=5:hi=mid
  else:lo=mid
 results.append(dict(boom_half_spacing_mm=spacing,initial_clearance_mm=initial,failing_shift_mm=lo,passing_shift_mm=hi,passing_clearance_mm=clearance(hi),interval_width_mm=hi-lo))
shift=math.ceil(max(x['passing_shift_mm'] for x in results)*100)/100
native.set_parameter(doc,'ReferenceDimensions','BoomHalfSpacing',220)
assert hashlib.sha256(SOURCE.read_bytes()).hexdigest()==digest
report=dict(scope='Proposed layout correction; 5 mm design clearance is provisional, not original-author intent',source_sha256=digest,mirror_inspection=inspection,target_fin_clearance_mm=5,search_interval_mm=[0,80],results=results,selected_forward_shift_mm=shift,selected_rear_motor_x_mm=585-shift,selection='Round maximum measured requirement at boom half-spacing220 and230 upward to0.01mm',source_preserved=True)
(OUT/'shift-search.json').write_text(json.dumps(report,indent=2));print(json.dumps(report['results']),flush=True);print('Selected rear motor X',585-shift,flush=True)
