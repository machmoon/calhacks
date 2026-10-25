"""Actual native cavity packaging trial; no inferred box dimensions disguised as hardware CAD."""
import hashlib,json
from pathlib import Path
import FreeCAD as App
import Part
import native
ROOT=Path(__file__).resolve().parents[2]
SOURCE=ROOT/'out/tsung-vtol-wing-spline-structure/verified/unverified-build.FCStd'
OUT=ROOT/'out/tsung-vtol-servo-fit';OUT.mkdir(exist_ok=True)
hash_before=hashlib.sha256(SOURCE.read_bytes()).hexdigest()
spec=json.loads((ROOT/'cad/projects/tsung_vtol/references/mg90s-dimensions.json').read_text())
doc=App.openDocument(str(SOURCE));cavity=doc.RightCavity.Shape
# Servo laid flat: tab-span along X, shaft-height envelope along Y, case width along Z.
size=[32.1,32.5,12.4];clearance=.5;y=370-size[1]/2
candidates=[]
for x in range(310,376,5):
 for z in range(12,27,2):
  p=App.Vector(x-.5,y-.5,z-.5);extent=[v+1 for v in size]
  corners=[p+App.Vector(a*extent[0],b*extent[1],c*extent[2]) for a in (0,1) for b in (0,1) for c in (0,1)]
  if all(cavity.isInside(v,1e-7,True) for v in corners):candidates.append((x,z))
print('Corner-screen candidates',len(candidates),flush=True)
trials=[];chosen=None
for x,z in candidates:
 grown=Part.makeBox(*(v+1 for v in size),App.Vector(x-.5,y-.5,z-.5))
 extra=grown.cut(cavity);outside=0 if extra.isNull() else extra.Volume
 trial=dict(x=x,z=z,grown_envelope_outside_cavity_mm3=outside);trials.append(trial)
 print('Exact envelope trial',trial,flush=True)
 if outside>1e-6:continue
 obstacles={n:grown.common(doc.getObject(n).Shape).Volume for n in ['RightSpar']+[f'RightRib{i}' for i in range(6)]}
 trial['grown_envelope_obstacle_overlaps_mm3']=obstacles
 if max(obstacles.values())<=1e-6:chosen=(x,z);break
report=dict(source=str(SOURCE),source_sha256=hash_before,source_wing_audit_status='unverified checkpoint at time of test',hardware_evidence=spec,orientation='Flat: tab span along X, total height including shaft along Y, body width along Z',static_size_xyz_mm=size,required_clearance_mm=clearance,corner_screen_candidate_count=len(candidates),exact_trials=trials,static_packaging_passed=chosen is not None,limitations=['Conservative static bounding box, not detailed servo CAD','Horn rotation, pushrod, mounting brackets, cable bend and hatch access not modeled','Actual author servo variant unknown','Wing source remains an independently audited prototype; no structural or manufacturing approval'])
if chosen:
 x,z=chosen
 native.make_part(doc,dict(name='MG90SStaticEnvelope',kind='box',params=dict(x=size[0],y=size[1],z=size[2])))
 servo=doc.MG90SStaticEnvelope;servo.Placement.Base=App.Vector(x,y,z)
 servo.setExpression('Placement.Base.y','WingControls.HalfSpan * (37 / 60) - 16.25 mm')
 doc.recompute();native.validate_document(doc)
 gap=servo.Shape.distToShape(cavity.Shells[0])[0];assert gap>=clearance-1e-6
 report['nominal']=dict(origin_mm=[x,y,z],native_minimum_cavity_wall_distance_mm=gap)
 dest=OUT/'assembly.FCStd';doc.saveAs(str(dest));savedhash=hashlib.sha256(dest.read_bytes()).hexdigest();App.closeDocument(doc.Name)
 doc=App.openDocument(str(dest));native.set_parameter(doc,'WingControls','HalfSpan',610)
 servo=doc.MG90SStaticEnvelope
 grown=Part.makeBox(*(v+1 for v in size),servo.Placement.Base-App.Vector(.5,.5,.5))
 outside=grown.cut(doc.RightCavity.Shape).Volume
 overlap={n:grown.common(doc.getObject(n).Shape).Volume for n in ['RightSpar']+[f'RightRib{i}' for i in range(6)]}
 gap=servo.Shape.distToShape(doc.RightCavity.Shape.Shells[0])[0]
 assert outside<=1e-6 and max(overlap.values())<=1e-6 and gap>=clearance-1e-6
 report['reopened_610mm_span_edit']=dict(grown_envelope_outside_cavity_mm3=outside,obstacle_overlaps_mm3=overlap,native_minimum_cavity_wall_distance_mm=gap,origin_mm=list(servo.Placement.Base))
 assert hashlib.sha256(dest.read_bytes()).hexdigest()==savedhash
 report['saved_native']=str(dest);report['saved_native_sha256']=savedhash
App.closeDocument(doc.Name)
assert hashlib.sha256(SOURCE.read_bytes()).hexdigest()==hash_before
report['source_preserved']=True
(OUT/'result.json').write_text(json.dumps(report,indent=2))
print('Finished static packaging:',chosen,flush=True)
