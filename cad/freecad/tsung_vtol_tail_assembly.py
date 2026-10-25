"""Integrate both proposed V2 tail clamps into the full reference reconstruction.
No author-internals or full assembly-completeness claim. V2 remains untouched.
"""
import base64,hashlib,json
from pathlib import Path
import FreeCAD as App
import Part
import native,worker
ROOT=Path(__file__).resolve().parents[2];OUT=ROOT/'out/tsung-vtol-tail-assembly';OUT.mkdir(parents=True,exist_ok=True)
SOURCE=ROOT/'out/tsung-vtol-tail-joint-v2/assembly.FCStd'
source_hash=hashlib.sha256(SOURCE.read_bytes()).hexdigest()
reference=json.loads((ROOT/'out/tsung-vtol-reference/result.json').read_text())
joint=json.loads((ROOT/'out/tsung-vtol-tail-joint-v2/result.json').read_text())
worker.open_document(str(SOURCE));doc=worker.active()
original_names=reference['physical_representation_names']
unchanged_names=[n for n in original_names if n not in ['FinRight','FinLeft']]
original={n:doc.getObject(n).Shape.copy() for n in unchanged_names}
# Bind primitive centers and profile origins, never placements of their derived
# booleans. The source fin and top-stabilizer expressions are already native.
targets=['ClampCollar','ClampEars','ClampUpperSpace','ClampLowerSpace','ConformalBoomSeat']
for i in range(2):
 targets.extend(f'{prefix}{i}' for prefix in ['ClearanceDrill','NutThreadEnvelopeTool','ScrewShank','ScrewHead'])
 targets.extend(f'{prefix}{i}_Section{j}' for prefix in ['NutPocketTool','NutBlank','HexSocketTool','AssemblyHexKeyEnvelope'] for j in range(2))
bindings=[]
doc.openTransaction('Bind complete right clamp to boom spacing')
for name in targets:
 obj=doc.getObject(name);assert obj is not None,name
 offset=obj.Placement.Base.y-220
 assert -30<=offset<=30,(name,offset)
 expression=f'ReferenceDimensions.BoomHalfSpacing + ({offset!r} mm)'
 obj.setExpression('Placement.Base.y',expression)
 bindings.append(dict(object=name,property='Placement.Base.y',expression=expression,offset_mm=offset))
doc.recompute();native.validate_document(doc);doc.commitTransaction()
right=[joint['physical'][1],joint['physical'][2],'Screw0','Screw1','Nut0','Nut1']
left=['FinClampLeft','LowerSaddleLeft','ScrewLeft0','ScrewLeft1','NutLeft0','NutLeft1']
for source,name in zip(right,left):
 mirror=doc.addObject('Part::Mirroring',name);mirror.Source=doc.getObject(source)
 mirror.Base=App.Vector(0,0,0);mirror.Normal=App.Vector(0,1,0)
 mirror.Label='Left '+doc.getObject(source).Label
 doc.recompute();assert native.describe(mirror)['solids']==1
assert all(n not in left for n in targets)
physical=unchanged_names+right+left
contacts=list(joint['fit']['pairs'])
contacts=[[p['a'],p['b']] for p in contacts if p['required_contact']]
map_left=dict(zip(right,left));map_left['BoomRight']='BoomLeft';map_left['TopStabilizer']='TopStabilizer'
contacts+=[[map_left[a],map_left[b]] for a,b in contacts.copy()]
# 11 original non-fin physical parts remain exactly unchanged at nominal layout.
unchanged=[]
for name,shape in original.items():
 now=doc.getObject(name).Shape
 exact=shape.cleaned().exportBrepToString()==now.cleaned().exportBrepToString()
 differences=[0.0,0.0] if exact else [shape.cut(now).Volume,now.cut(shape).Volume]
 assert max(differences)<1e-5 and abs(shape.Volume-now.Volume)<1e-5,name
 unchanged.append(dict(name=name,identical_clean_brep=exact,difference_mm3=differences))
# New left mirror must preserve the original left fin away from joint interfaces.
left_root=Part.makeBox(160,70,30,App.Vector(620,-255,-22))
left_top=Part.makeBox(160,80,15,App.Vector(620,-215,120))
a=doc.FinLeft.Shape.cut(left_root).cut(left_top)
b=doc.FinClampLeft.Shape.cut(left_root).cut(left_top)
left_contour=[a.cut(b).Volume,b.cut(a).Volume];assert max(left_contour)<1e-5,left_contour
print('Both clamps integrated; verifying coupled layout edits',flush=True)
def joint_measurements():
 measurements={}
 for side,names,boom in [('Right',right,'BoomRight'),('Left',left,'BoomLeft')]:
  upper,lower=map(doc.getObject,names[:2]);tube=doc.getObject(boom)
  split=upper.Shape.distToShape(lower.Shape)[0]
  radial=[upper.Shape.distToShape(tube.Shape)[0],lower.Shape.distToShape(tube.Shape)[0]]
  assert abs(split-.5)<1e-6 and max(radial)<1e-6,(side,split,radial)
  measurements[side]=dict(split_gap_mm=split,radial_gaps_mm=radial,screw_y_mm=[doc.getObject(n).Shape.Solids[0].CenterOfMass.y for n in names[2:4]])
 return measurements
def same_measurements(a,b):
 for side in a:
  for key in a[side]:
   av=a[side][key] if isinstance(a[side][key],list) else [a[side][key]]
   bv=b[side][key] if isinstance(b[side][key],list) else [b[side][key]]
   if any(abs(x-y)>1e-6 for x,y in zip(av,bv)):return False
 return True
nominal=joint_measurements()
static=['WingRight','WingLeft','Pod','PusherMotor','TopStabilizer']
static_before=worker.geometry_fingerprint([doc.getObject(n) for n in static])
native.set_parameter(doc,'ReferenceDimensions','BoomHalfSpacing',230)
edited=joint_measurements()
joint_selection=['BoomRight','BoomLeft','TopStabilizer']+right+left
joint_contacts=worker.check(joint_selection,contacts,tolerance_mm=.001,require_connected=True)
assert joint_contacts['passed'],joint_contacts
static_edit_equivalence=[]
for name in static:
 old=original[name];now=doc.getObject(name).Shape
 same_brep=old.cleaned().exportBrepToString()==now.cleaned().exportBrepToString()
 differences=[0.0,0.0] if same_brep else [old.cut(now).Volume,now.cut(old).Volume]
 assert max(differences)<1e-5,name
 static_edit_equivalence.append(dict(name=name,identical_clean_brep=same_brep,difference_mm3=differences))
for side,sign in [('Right',1),('Left',-1)]:
 assert abs(doc.getObject('Boom'+side).Shape.Solids[0].CenterOfMass.y-sign*230)<1e-6
 for suffix in ['F','R']:
  obj=doc.getObject('LiftMotor'+suffix+('R' if sign==1 else 'L'))
  assert abs(obj.Shape.Solids[0].CenterOfMass.y-sign*230)<1e-6
assert all(abs(a-b-10)<1e-6 for a,b in zip(edited['Right']['screw_y_mm'],nominal['Right']['screw_y_mm']))
assert all(abs(a-b+10)<1e-6 for a,b in zip(edited['Left']['screw_y_mm'],nominal['Left']['screw_y_mm']))
native.set_parameter(doc,'ReferenceDimensions','BoomHalfSpacing',220)
assert same_measurements(joint_measurements(),nominal)
print('Typed 220 -> 230 -> 220 spacing edit passes; full aircraft checks',flush=True)
fit=worker.check(physical,contacts,tolerance_mm=.001,require_connected=True)
assert fit['overlap_count']==0 and fit['unmet_contact_count']==0,fit
bounds=Part.makeCompound([doc.getObject(n).Shape for n in physical]).BoundBox
assert abs(bounds.XLength-770)<1e-5 and abs(bounds.YLength-1200)<1e-5
exports=worker.save(str(OUT),physical)
render=worker.render(physical)
(OUT/'aircraft.png').write_bytes(base64.b64decode(render.pop('content_blocks')[0]['source']['data']))
(OUT/'render.json').write_text(json.dumps(render,indent=2))
report=dict(scope='Full reference reconstruction with bilateral proposed V2 tail joints; remaining interfaces incomplete',source_sha256=source_hash,source_native=str(SOURCE),physical=physical,fin_physical_names=dict(Right=right[0],Left=left[0]),right_joint_outputs=right,left_joint_outputs=left,bindings=bindings,unchanged_reference_parts=unchanged,left_fin_contour_difference_outside_interfaces_mm3=left_contour,nominal_joint_measurements=nominal,edited_230mm_joint_measurements=edited,edited_joint_fit=joint_contacts,static_geometry_unchanged_during_edit=True,static_edit_equivalence=static_edit_equivalence,typed_boom_spacing_edit_passed=True,fit=fit,bounds_mm=[[bounds.XMin,bounds.YMin,bounds.ZMin],[bounds.XMax,bounds.YMax,bounds.ZMax]],exports=exports,full_aircraft_complete=False,original_author_joint=False,limitations=joint['limitations']+['The full fit report identifies disconnected groups; nominal motor/boom or pod/motor contact does not establish mounting hardware or retention','Full reference wing and tail surfaces are still exterior envelopes; internal structure incomplete'])
(OUT/'result.json').write_text(json.dumps(report,indent=2))
# The saved full artifact must retain the bilateral dependency after reopening.
saved=Path(exports['native']);saved_hash=hashlib.sha256(saved.read_bytes()).hexdigest()
App.closeDocument(doc.Name);worker._doc=None;worker.restore(str(saved));doc=worker.active()
native.set_parameter(doc,'ReferenceDimensions','BoomHalfSpacing',230)
assert same_measurements(joint_measurements(),edited)
report['saved_reopen_bilateral_edit_passed']=True
assert hashlib.sha256(saved.read_bytes()).hexdigest()==saved_hash and hashlib.sha256(SOURCE.read_bytes()).hexdigest()==source_hash
report['source_preserved']=True
(OUT/'result.json').write_text(json.dumps(report,indent=2))
print(json.dumps(dict(physical_parts=len(physical),pairs=fit['pairs_total'],clashes=fit['overlap_count'],unmet_contacts=fit['unmet_contact_count'],groups=len(fit['components']),full_fit_passed=fit['passed'],reopen_edit=True)))
