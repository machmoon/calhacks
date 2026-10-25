"""Audit candidate carrier mounting holes in the pinned official Jetson STEP."""
import json,hashlib,math,time
from pathlib import Path
import FreeCAD as App
import Part
root=Path(__file__).resolve().parents[2];path=root/'cad/components/vendor/nvidia/P3766-P3768SKU4-P3767ENVELOPE.stp';start=time.perf_counter();expected='02dafef626d5ac6ee029d1ead431d6096c27c7c51fef6b49d5189e1765fb9d2f';assert hashlib.sha256(path.read_bytes()).hexdigest()==expected;shape=Part.read(str(path));candidates=[]
for i,s in enumerate(shape.Solids):
 b=s.optimalBoundingBox(False,False)
 if not (95<b.XLength<105 and 74<b.YLength<84 and 1<b.ZLength<2.5):continue
 circles=[]
 for face in s.Faces:
  surface=face.Surface
  if not isinstance(surface,Part.Cylinder) or abs(abs(surface.Axis.z)-1)>1e-8:continue
  radius=surface.Radius;c=surface.Center
  if not .5<radius<4:continue
  box=face.optimalBoundingBox(False,False);z=(b.ZMin+b.ZMax)/2
  if box.ZMin>b.ZMin+1e-5 or box.ZMax<b.ZMax-1e-5:continue
  center=App.Vector(c.x,c.y,z)
  empty=not s.isInside(center,1e-7,True)
  wall=all(s.isInside(App.Vector(c.x+(radius+.2)*math.cos(a),c.y+(radius+.2)*math.sin(a),z),1e-7,True) for a in [k*math.pi/4 for k in range(8)])
  circles.append(dict(center_mm=[c.x,c.y,z],radius_mm=radius,full_board_depth=True,center_is_void=empty,eight_surrounding_material_probes=wall))
 unique={tuple(round(v,6) for v in row['center_mm']+[row['radius_mm']]):row for row in circles}
 candidates.append(dict(source_solid_index=i+1,bounds_mm=[b.XMin,b.YMin,b.ZMin,b.XMax,b.YMax,b.ZMax],volume_mm3=s.Volume,cylindrical_through_features=list(unique.values())))
report=dict(source=str(path),source_sha256=hashlib.sha256(path.read_bytes()).hexdigest(),board_candidates=candidates,elapsed_seconds=time.perf_counter()-start,scope='Geometric through-hole candidates from official STEP. Mount purpose, screw specification and actual user kit configuration not yet confirmed.')
assert hashlib.sha256(path.read_bytes()).hexdigest()==expected
out=root/'out/vendor-component-inventory/jetson-mount-probe.json';out.write_text(json.dumps(report,indent=2));print(json.dumps(report,indent=2))
