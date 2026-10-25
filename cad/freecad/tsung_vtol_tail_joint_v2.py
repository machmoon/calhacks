"""Proposed split-clamp engineering revision, not unpublished author geometry.
Conformal nominal bore seats with remaining split clearance, underside screw
heads and upper hex-nut bearing pockets. Thread envelopes are simplified.
"""
import base64,hashlib,json,math,os,shutil
from pathlib import Path
import FreeCAD as App
import Part
import native,worker,layout,booleans
ROOT=Path(__file__).resolve().parents[2]
OUT=ROOT/'out/tsung-vtol-tail-joint-v2';OUT.mkdir(parents=True,exist_ok=True)
SOURCE=ROOT/'out/tsung-vtol-reference/assembly.FCStd'
source_hash=hashlib.sha256(SOURCE.read_bytes()).hexdigest()
worker.open_document(str(SOURCE));doc=worker.active()
original_fin=doc.FinRight.Shape.copy()
layout.create(doc,'ClampDimensions',{'SplitGap':{'value':.5,'min':.2,'max':1}})
features=[]
def part(name,kind,params,position=(0,0,0),axis=None):
 native.make_part(doc,dict(name=name,kind=kind,params=params));o=doc.getObject(name);o.Placement.Base=App.Vector(*position)
 if axis:o.Placement.Rotation=App.Rotation(App.Vector(0,0,1),App.Vector(*axis))
 doc.recompute();features.append(name);return o

def boolean(name,kind,base,tool):
 o=doc.addObject('Part::'+kind,name);o.Base=base;o.Tool=tool;doc.recompute()
 assert o.Shape.isValid() and len(o.Shape.Solids)==1,name
 features.append(name);return o

def hex_prism(name,x,y,z,af,height):
 radius=af/math.sqrt(3)
 return part(name,'loft',dict(rings=[[[x+radius*math.cos(t*math.tau/6),y+radius*math.sin(t*math.tau/6),zz] for t in range(6)] for zz in [z,z+height]]))

collar=part('ClampCollar','cylinder',dict(radius=9,height=20),(665,220,-10),(1,0,0))
ears=part('ClampEars','box',dict(x=20,y=40,z=12),(665,200,-16))
blank=boolean('ClampBlank','Fuse',collar,ears)
upper_space=part('ClampUpperSpace','box',dict(x=25,y=45,z=20),(662.5,197.5,-9.75))
upper_space.setExpression('Placement.Base.z','-10 mm + ClampDimensions.SplitGap / 2')
lower_space=part('ClampLowerSpace','box',dict(x=25,y=45,z=11.75),(662.5,197.5,-22))
lower_space.setExpression('Height','12 mm - ClampDimensions.SplitGap / 2');doc.recompute()
upper_blank=boolean('ClampUpperBlank','Common',blank,upper_space)
lower_blank=boolean('ClampLowerBlank','Common',blank,lower_space)
booleans.create(doc,'FinTopSeatV2','cut','FinRight','TopStabilizer',True)
upper=boolean('FinClampBlankV2','Fuse',doc.FinTopSeatV2,upper_blank)
# A solid bore cutter removes the tube interior as well as wall; cutting with
# the physical tube alone would accidentally leave a plug in the bore.
bore=part('ConformalBoomSeat','cylinder',dict(radius=5,height=800),(0,220,-10),(1,0,0))
bore.setExpression('Radius','BoomRight_Outer.Radius');doc.recompute()
upper=boolean('FinConformalSeat','Cut',upper,bore)
lower=boolean('LowerConformalSeat','Cut',lower_blank,bore)
bolts=[];nuts=[];keys=[]
for i,y in enumerate([206,234]):
 hole=part(f'ClearanceDrill{i}','cylinder',dict(radius=1.7,height=30),(675,y,-25))
 upper=boolean(f'FinDrillV2_{i}','Cut',upper,hole)
 lower=boolean(f'LowerDrillV2_{i}','Cut',lower,hole)
 pocket=hex_prism(f'NutPocketTool{i}',675,y,-6.4,5.7,2.5)
 upper=boolean(f'FinNutPocket{i}','Cut',upper,pocket)
 nut_outer=hex_prism(f'NutBlank{i}',675,y,-6.4,5.5,2.4)
 nut_bore=part(f'NutThreadEnvelopeTool{i}','cylinder',dict(radius=1.5,height=4.4),(675,y,-7.4))
 nut=boolean(f'Nut{i}','Cut',nut_outer,nut_bore);nuts.append(nut)
 shaft=part(f'ScrewShank{i}','cylinder',dict(radius=1.5,height=16),(675,y,-16))
 head=part(f'ScrewHead{i}','cylinder',dict(radius=2.75,height=3),(675,y,-19))
 bolt=boolean(f'ScrewBlank{i}','Fuse',shaft,head)
 socket=hex_prism(f'HexSocketTool{i}',675,y,-19.1,2.5,1.4)
 bolt=boolean(f'Screw{i}','Cut',bolt,socket);bolts.append(bolt)
 key=hex_prism(f'AssemblyHexKeyEnvelope{i}',675,y,-59,2.4,41.3);keys.append(key)
upper.Label='Proposed right fin clamp: conformal seat and upper nut pockets'
lower.Label='Proposed lower saddle: residual split gap and underside screw heads'
for o in nuts:o.Label='M3 nut nominal thread envelope; no helix/preload model'
for o in bolts:o.Label='M3 x 16 socket screw envelope; no helix/preload model'
selected=['BoomRight',upper.Name,lower.Name,'TopStabilizer']+[o.Name for o in bolts+nuts]
contacts=[[upper.Name,'BoomRight'],[lower.Name,'BoomRight'],[upper.Name,'TopStabilizer']]
for bolt,nut in zip(bolts,nuts):contacts.extend([[bolt.Name,lower.Name],[nut.Name,upper.Name],[bolt.Name,nut.Name]])
fit=worker.check(selected,contacts,tolerance_mm=.001,require_connected=True)
assert fit['passed'],[(p['a'],p['b'],p['overlap_mm3'],p['distance_mm']) for p in fit['pairs'] if p['clash'] or p['floating']]
radial_gaps={o.Name:o.Shape.distToShape(doc.BoomRight.Shape)[0] for o in [upper,lower]}
assert all(v<1e-6 for v in radial_gaps.values())
split=upper.Shape.distToShape(lower.Shape)[0]
assert abs(split-.5)<1e-6,split
print('V2 assembled fit passes; checking closure, retention and source contour',flush=True)
# Opening the lower saddle and its seated screw heads by 0.4 mm gives positive
# radial clearance; closing returns to bearing before the split faces bottom.
closure=[]
for offset in [-.4,-.3,-.2,-.1,0,.05]:
 moved=lower.Shape.copy();moved.translate(App.Vector(0,0,offset))
 overlap=moved.common(doc.BoomRight.Shape).Volume
 clearance=moved.distToShape(doc.BoomRight.Shape)[0]
 split_gap=moved.distToShape(upper.Shape)[0]
 closure.append(dict(lower_z_offset_mm=offset,boom_overlap_mm3=overlap,boom_clearance_mm=clearance,upper_gap_mm=split_gap))
 if offset<=0:assert overlap<1e-6
 else:assert overlap>1e-4 and split_gap>.4
assert closure[0]['boom_clearance_mm']>.02
# Local rigid rotation proves the oversize hex pocket has an anti-rotation stop.
rotation=[]
for i,nut in enumerate(nuts):
 rotated=nut.Shape.copy();rotated.rotate(App.Vector(675,[206,234][i],-6.4),App.Vector(0,0,1),6)
 overlap=rotated.common(upper.Shape).Volume;assert overlap>1e-4
 rotation.append(dict(nut=nut.Name,rotation_degrees=6,interference_mm3=overlap))
# Moving a screw upward pushes its head into the lower bearing face, and
# pulling its nut downward pushes it into the upper pocket floor.
retention=[]
for bolt,nut in zip(bolts,nuts):
 b=bolt.Shape.copy();b.translate(App.Vector(0,0,.1));n=nut.Shape.copy();n.translate(App.Vector(0,0,-.1))
 overlaps=[b.common(lower.Shape).Volume,n.common(upper.Shape).Volume]
 assert min(overlaps)>1e-4
 retention.append(dict(screw=bolt.Name,nut=nut.Name,head_up_0_1_mm_interference_mm3=overlaps[0],nut_down_0_1_mm_interference_mm3=overlaps[1]))
# Straight hex-key volume below each head is clear against this selected joint.
# This does not certify tool access in the full aircraft.
key_checks=[]
for key in keys:
 for name in selected:
  volume=key.Shape.common(doc.getObject(name).Shape).Volume
  assert volume<1e-6,(key.Name,name,volume)
  key_checks.append(dict(tool=key.Name,part=name,overlap_mm3=volume))
# Preserve the source fin everywhere outside explicit root and top seat regions.
root_roi=Part.makeBox(160,70,30,App.Vector(620,185,-22))
top_roi=Part.makeBox(160,80,15,App.Vector(620,135,120))
old=original_fin.cut(root_roi).cut(top_roi);new=upper.Shape.cut(root_roi).cut(top_roi)
contour_errors=[old.cut(new).Volume,new.cut(old).Volume]
assert max(contour_errors)<1e-5,contour_errors
# The full lower-plus-screw downward extraction remains conservatively screened.
removal=worker.METHODS['check_translation'](moving=[lower.Name]+[o.Name for o in bolts],obstacles=[upper.Name,'BoomRight','TopStabilizer']+[o.Name for o in nuts],translation=[0,0,-30],samples=11)
fit=worker.check(selected,contacts,tolerance_mm=.001,require_connected=True)
exports=worker.save(str(OUT),selected)
image=worker.render(selected,width=640,height=480,azimuth_degrees=-52,elevation_degrees=12)
(OUT/'joint.png').write_bytes(base64.b64decode(image.pop('content_blocks')[0]['source']['data']))
(OUT/'render.json').write_text(json.dumps(image,indent=2))
# Close-up without tall fin/stabilizer; physical lower, boom and fasteners reveal seats.
closeup=worker.render([lower.Name]+[o.Name for o in bolts+nuts],width=640,height=480,azimuth_degrees=-52,elevation_degrees=-25)
(OUT/'lower-fasteners.png').write_bytes(base64.b64decode(closeup.pop('content_blocks')[0]['source']['data']))
report=dict(scope='Proposed split-clamp engineering revision on actual reconstructed fin/boom; not author internals',source_sha256=source_hash,original_author_joint=False,physical=selected,fit=fit,nominal_boom_radius_mm=5,assembled_radial_gaps_mm=radial_gaps,remaining_split_gap_mm=split,closure=closure,nut_rotation_stops=rotation,axial_retention_stops=retention,local_hex_key_checks=key_checks,fin_contour_difference_outside_interface_mm3=contour_errors,removal=removal,exports=exports,provisional=dict(split_gap_mm=.5,opening_travel_mm=.4,collar_outer_radius_mm=9,collar_length_mm=20,ear_xyz_mm=[20,40,12],bolt_hole_diameter_mm=3.4,nut_pocket_af_mm=5.7,nut_pocket_floor_z_mm=-6.4,fastener_centers_xy_mm=[[675,206],[675,234]]),fastener_sources=[dict(url='https://www.accu.co.uk/metric-cap-head-screws/250420-SSCF-M3-16-A2-R360',dimensions=dict(thread='M3',nominal_diameter_mm=3,length_mm=16,pitch_mm=.5,head_diameter_mm=5.5,head_height_mm=3,socket_af_mm=2.5,socket_depth_mm=1.3),note='Dimensional reference only; coating/material/stock not procurement approval'),dict(url='https://media.distrelec.com/Web/Downloads/_t/ds/1241613_eng_tds.pdf',author='Bossard BN629 technical sheet',dimensions=dict(thread='M3',af_mm=5.5,max_height_mm=2.4),note='Ideal sharp hex envelope, chamfers and thread helix omitted')],load_path=['Fin into fused upper collar and ears','Screw tension bridges lower head-bearing face and upper nut-pocket floor','Upper and lower conformal saddle surfaces bear radially on boom before split faces touch','Axial and torsional retention on round boom rely on friction/preload, not proven by geometric contact'],assembly_order=['Place nuts in upward-open hex pockets while their floor supports them','Place upper fin saddle on nominal boom surface','Bring lower saddle and screw heads upward from below','Insert screws upward and engage purchased M3 nuts; alternate closure to nominal bearing condition','Verify remaining split gap on both sides and actual boom fit before any preload specification'],limitations=['Rigid nominal geometry does not determine clamp preload, friction or strength','Boom diameter tolerance, surface finish, CFRP crushing and polymer creep require measured allowances and analysis','Thread helix and elastic deformation omitted; nominal thread-envelope contact is not thread engagement validation','Nut pockets resist rotation when assembled but are open above and not loose-nut capture features','Top stabilizer seat retention hardware remains missing','Fin and stabilizer remain exterior envelopes rather than manufactured structures','Local straight hex-key clearance and sampled removal do not prove full-aircraft serviceability'])
(OUT/'result.json').write_text(json.dumps(report,indent=2))
# Verify the shared split-gap edit on reopened native features, preserving file bytes.
upper_name=upper.Name;lower_name=lower.Name
saved=Path(exports['native']);saved_hash=hashlib.sha256(saved.read_bytes()).hexdigest()
App.closeDocument(doc.Name);worker._doc=None;worker.restore(str(saved));doc=worker.active()
native.set_parameter(doc,'ClampDimensions','SplitGap',.6)
newgap=doc.getObject(upper_name).Shape.distToShape(doc.getObject(lower_name).Shape)[0]
assert abs(newgap-.6)<1e-6
assert hashlib.sha256(saved.read_bytes()).hexdigest()==saved_hash and hashlib.sha256(SOURCE.read_bytes()).hexdigest()==source_hash
report['saved_reopen_split_edit_mm']=newgap;report['source_preserved']=True
(OUT/'result.json').write_text(json.dumps(report,indent=2))
print(json.dumps(dict(passed=fit['passed'],parts=len(selected),pairs=fit['pairs_total'],split_gap_mm=split,radial_gaps=radial_gaps,closure=closure,removal=removal['status'])))
