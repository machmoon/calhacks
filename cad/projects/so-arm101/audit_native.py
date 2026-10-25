"""Read-only saved FreeCAD assembly audit; preserves source frames and hierarchy."""
import hashlib,json
from pathlib import Path
import FreeCAD as App
ROOT=Path(__file__).resolve().parents[3];OUT=ROOT/'out/so-arm101-native';path=OUT/'official-assembly.FCStd';before=hashlib.sha256(path.read_bytes()).hexdigest();doc=App.openDocument(str(path))
leaves=[]
for obj in doc.Objects:
 if obj.TypeId!='Part::Feature' or obj.Shape.isNull() or not obj.Shape.Solids:continue
 chain=[];parent=obj.getParentGeoFeatureGroup()
 while parent is not None:
  chain.append(dict(name=parent.Name,label=parent.Label));parent=parent.getParentGeoFeatureGroup()
 shape=obj.Shape.copy();shape.Placement=obj.getGlobalPlacement().multiply(obj.Placement.inverse()).multiply(shape.Placement)
 bounds=shape.optimalBoundingBox(False,False)
 leaves.append(dict(name=obj.Name,label=obj.Label,ancestors=chain,solids=len(shape.Solids),faces=len(shape.Faces),valid=shape.isValid(),volume_mm3=shape.Volume,global_placement=dict(position=list(obj.getGlobalPlacement().Base),quaternion=list(obj.getGlobalPlacement().Rotation.Q)),world_bounds_mm=[bounds.XMin,bounds.YMin,bounds.ZMin,bounds.XMax,bounds.YMax,bounds.ZMax]))
assert leaves
invalid=[dict(name=r['name'],label=r['label']) for r in leaves if not r['valid']]
bounds=[min(r['world_bounds_mm'][i] for r in leaves) for i in range(3)]+[max(r['world_bounds_mm'][i] for r in leaves) for i in range(3,6)]
servos=[dict(name=o.Name,label=o.Label,children=[x.Name for x in o.Group]) for o in doc.Objects if o.TypeId=='App::Part' and 'ST3215 Servo' in o.Label]
assert hashlib.sha256(path.read_bytes()).hexdigest()==before
report=dict(native=str(path),native_sha256=before,source_frame='Official STEP assembly frame; no URDF registration applied',reopen_passed=True,native_unchanged=True,physical_leaf_count=len(leaves),all_physical_leaves_valid=not invalid,invalid_physical_leaves=invalid,physical_solid_count=sum(r['solids'] for r in leaves),world_bounds_mm=bounds,servo_instances=servos,leaves=leaves,fit_checked=False,motion_checked=False,kinematic_constraints_added=False,reference_surfaces=[dict(name=o.Name,label=o.Label,faces=len(o.Shape.Faces)) for o in doc.Objects if o.TypeId=='Part::Feature' and not o.Shape.isNull() and not o.Shape.Solids])
(OUT/'native-audit.json').write_text(json.dumps(report,indent=2));print(json.dumps({k:v for k,v in report.items() if k not in ['leaves','servo_instances']}),flush=True)
