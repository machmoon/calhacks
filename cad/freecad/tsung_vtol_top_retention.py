"""Proposed top-stabilizer retention: local pads, four M3 screws, side-entry nuts.
Engineering revision only; not unpublished author hardware or strength approval.
"""
import base64,hashlib,json,math
from pathlib import Path
import FreeCAD as App
import Part
import native,worker,booleans
ROOT=Path(__file__).resolve().parents[2];OUT=ROOT/'out/tsung-vtol-top-retention';OUT.mkdir(parents=True,exist_ok=True)
SOURCE=ROOT/'out/tsung-vtol-tail-assembly/assembly.FCStd';source_hash=hashlib.sha256(SOURCE.read_bytes()).hexdigest()
previous=json.loads((SOURCE.parent/'result.json').read_text());joint=json.loads((ROOT/'out/tsung-vtol-tail-joint-v2/result.json').read_text())
worker.open_document(str(SOURCE));doc=worker.active()
old_fin=doc.getObject(previous['fin_physical_names']['Right']).Shape.copy();old_top=doc.TopStabilizer.Shape.copy()
bindings=[]
def bind(obj,y,z):
 for prop,source,offset in [('Placement.Base.y','TailTopHalfSpacing',y-175),('Placement.Base.z','TailHeight',z-140)]:
  expression=f'ReferenceDimensions.{source} + ({offset!r} mm)';obj.setExpression(prop,expression)
  bindings.append(dict(object=obj.Name,property=prop,expression=expression))
def part(name,kind,params,xyz):
 native.make_part(doc,dict(name=name,kind=kind,params=params));o=doc.getObject(name);o.Placement.Base=App.Vector(*xyz)
 bind(o,xyz[1],xyz[2]);doc.recompute();return o
def cylinder(name,r,h,x,z):return part(name,'cylinder',dict(radius=r,height=h),(x,175,z))
def box(name,x,y,z,sx,sy,sz):return part(name,'box',dict(x=sx,y=sy,z=sz),(x,y,z))
def hex_prism(name,x,y,z,af,height):
 radius=af/math.sqrt(3)
 native.make_part(doc,dict(name=name,kind='loft',params=dict(rings=[[[x+radius*math.cos(t*math.tau/6),y+radius*math.sin(t*math.tau/6),zz] for t in range(6)] for zz in [z,z+height]])))
 o=doc.getObject(name)
 for section,zz in zip(o.Sections,[z,z+height]):bind(section,y,zz)
 doc.recompute();return o
def boolean(name,kind,base,tool):
 o=doc.addObject('Part::'+kind,name);o.Base=base;o.Tool=tool;doc.recompute()
 assert o.Shape.isValid() and len(o.Shape.Solids)==1,name
 return o
def mirror(name,source):
 worker.METHODS['mirror'](name=name,source=source.Name,normal=[0,1,0]);return doc.getObject(name)

stations=[700,745];pads=[];drills=[];bosses=[];screws=[];nuts=[];keys=[]
for i,x in enumerate(stations):
 pad=cylinder(f'TopPadRight{i}',4,5,x,129);pads.extend([pad,mirror(f'TopPadLeft{i}',pad)])
 drill=cylinder(f'TopDrillRight{i}',1.7,40,x,110);drills.extend([drill,mirror(f'TopDrillLeft{i}',drill)])
 bosses.append(cylinder(f'FinNutBoss{i}',5,15,x,115))
 shaft=cylinder(f'TopScrewShank{i}',1.5,16,x,118);head=cylinder(f'TopScrewHead{i}',2.75,3,x,134)
 screw=boolean(f'TopScrewBlank{i}','Fuse',shaft,head)
 socket=hex_prism(f'TopSocketTool{i}',x,175,135.7,2.5,1.4)
 screw=boolean(f'TopScrewRight{i}','Cut',screw,socket);screws.extend([screw,mirror(f'TopScrewLeft{i}',screw)])
 nut=hex_prism(f'TopNutBlank{i}',x,175,119,5.5,2.4)
 hole=cylinder(f'TopNutThreadTool{i}',1.5,4.4,x,118)
 nut=boolean(f'TopNutRight{i}','Cut',nut,hole);nuts.extend([nut,mirror(f'TopNutLeft{i}',nut)])
 key=hex_prism(f'TopHexKeyRight{i}',x,175,135.7,2.4,41.3);keys.extend([key,mirror(f'TopHexKeyLeft{i}',key)])
stab=doc.TopStabilizer
for i,pad in enumerate(pads):stab=boolean(f'TopPadFuse{i}','Fuse',stab,pad)
for i,drill in enumerate(drills):stab=boolean('TopStabilizerRetained' if i==len(drills)-1 else f'TopPadDrill{i}','Cut',stab,drill)
fin=doc.getObject(previous['fin_physical_names']['Right'])
for i,boss in enumerate(bosses):fin=boolean(f'FinBossFuse{i}','Fuse',fin,boss)
# Cut against the actual retained stabilizer, not an exempted intersecting mate.
booleans.create(doc,'FinTopSeatRetained','cut',fin.Name,stab.Name,True);fin=doc.FinTopSeatRetained
for i,x in enumerate(stations):
 fin=boolean(f'FinTopBore{i}','Cut',fin,drills[2*i])
 pocket=hex_prism(f'TopNutPocketTool{i}',x,175,118.8,5.7,2.6)
 fin=boolean(f'FinTopPocket{i}','Cut',fin,pocket)
 slot=box(f'TopNutEntryTool{i}',x-3.3,164,118.8,6.6,11,2.6)
 fin=boolean('FinTopRetentionRight' if i==len(stations)-1 else f'FinTopEntry{i}','Cut',fin,slot)
fin.Label='Proposed right fin: root clamp and top stabilizer fastener bosses'
left=mirror('FinTopRetentionLeft',fin);left.Label='Proposed left fin: mirrored root clamp and top stabilizer fastener bosses'
stab.Label='Top stabilizer with four local screw head-bearing pads'
replace={previous['fin_physical_names']['Right']:fin.Name,previous['fin_physical_names']['Left']:left.Name,'TopStabilizer':stab.Name}
physical=[replace.get(n,n) for n in previous['physical']]+[o.Name for o in screws+nuts]
contacts=[[replace.get(p['a'],p['a']),replace.get(p['b'],p['b'])] for p in previous['fit']['pairs'] if p['required_contact']]
for i,(screw,nut) in enumerate(zip(screws,nuts)):
 current_fin=fin if i%2==0 else left
 contacts.extend([[screw.Name,stab.Name],[nut.Name,current_fin.Name],[screw.Name,nut.Name]])
print('Top-retention geometry built; checking all physical pairs',flush=True)
fit=worker.check(physical,contacts,tolerance_mm=.001,require_connected=True)
assert fit['overlap_count']==0 and fit['unmet_contact_count']==0,[(p['a'],p['b'],p['overlap_mm3'],p['distance_mm']) for p in fit['pairs'] if p['clash'] or p['floating']]
# Measure retained fastener clearances and adversarial bearing/rotation stops.
fastener_checks=[]
for i,(screw,nut) in enumerate(zip(screws,nuts)):
 current_fin=fin if i%2==0 else left;sign=1 if i%2==0 else -1;x=stations[i//2]
 shank=doc.getObject(f'TopScrewShank{i//2}').Shape.copy()
 if sign<0:
  # Only this measurement copy is transformed; the actual assembly uses typed mirrors.
  shank=doc.getObject(f'TopScrewLeft{i//2}').Shape.common(Part.makeCylinder(1.5,16,App.Vector(x,-175,118)))
 gap=shank.distToShape(stab.Shape)[0];assert abs(gap-.2)<1e-6
 moved=screw.Shape.copy();moved.translate(App.Vector(0,0,-.1));head_stop=moved.common(stab.Shape).Volume;assert head_stop>1e-4
 moved=nut.Shape.copy();moved.translate(App.Vector(0,0,.1));ceiling_stop=moved.common(current_fin.Shape).Volume;assert ceiling_stop>1e-4
 moved=nut.Shape.copy();moved.translate(App.Vector(0,0,-.3));floor_stop=moved.common(current_fin.Shape).Volume;assert floor_stop>1e-4
 moved=nut.Shape.copy();moved.rotate(App.Vector(x,sign*175,119),App.Vector(0,0,1),6);rotation_stop=moved.common(current_fin.Shape).Volume;assert rotation_stop>1e-4
 fastener_checks.append(dict(screw=screw.Name,nut=nut.Name,shank_stabilizer_clearance_mm=gap,nominal_tip_protrusion_mm=1,head_down_0_1_mm_interference_mm3=head_stop,nut_up_0_1_mm_interference_mm3=ceiling_stop,nut_down_0_3_mm_interference_mm3=floor_stop,nut_rotation_6deg_interference_mm3=rotation_stop))
key_checks=[]
for key in keys:
 for name in physical:
  volume=key.Shape.common(doc.getObject(name).Shape).Volume;assert volume<1e-6,(key.Name,name,volume)
  key_checks.append(dict(tool=key.Name,part=name,overlap_mm3=volume))
insertion=[]
for i,nut in enumerate(nuts):
 current_fin=fin if i%2==0 else left;sign=1 if i%2==0 else -1
 for offset in [-10,-8,-6,-4,-2,0]:
  moved=nut.Shape.copy();moved.translate(App.Vector(0,sign*offset,0))
  overlap=moved.common(current_fin.Shape).Volume+moved.common(stab.Shape).Volume
  assert overlap<1e-6,(nut.Name,offset,overlap)
  insertion.append(dict(nut=nut.Name,inboard_offset_mm=offset,overlap_mm3=overlap))
# Prove all shape changes remain within explicit local interface windows.
rois=[Part.makeBox(16,24,34,App.Vector(x-8,sign*175-12,108)) for x in stations for sign in [1,-1]]
def outside(shape,tools):
 for tool in tools:shape=shape.cut(tool)
 return shape
fin_old=outside(old_fin,rois);fin_new=outside(fin.Shape,rois)
stab_old=outside(old_top,rois);stab_new=outside(stab.Shape,rois)
contour=dict(fin=[fin_old.cut(fin_new).Volume,fin_new.cut(fin_old).Volume],stabilizer=[stab_old.cut(stab_new).Volume,stab_new.cut(stab_old).Volume])
assert max(contour['fin']+contour['stabilizer'])<1e-5,contour
right_name,left_name,stab_name=fin.Name,left.Name,stab.Name
new_names=[o.Name for o in screws+nuts];tail_selection=[n for n in physical if n not in ['WingRight','WingLeft','Pod','PusherMotor']]
nominal_centers={n:doc.getObject(n).Shape.Solids[0].CenterOfMass for n in new_names}
print('Bearing, insertion and local exterior checks pass; testing bilateral spacing dependency',flush=True)
native.set_parameter(doc,'ReferenceDimensions','BoomHalfSpacing',230)
edited_fit=worker.check(tail_selection,contacts,tolerance_mm=.001,require_connected=True);assert edited_fit['passed'],edited_fit
for n,center in nominal_centers.items():assert (doc.getObject(n).Shape.Solids[0].CenterOfMass-center).Length<1e-6
for name,lower,boom in [(right_name,'LowerDrillV2_1','BoomRight'),(left_name,'LowerSaddleLeft','BoomLeft')]:
 assert abs(doc.getObject(name).Shape.distToShape(doc.getObject(lower).Shape)[0]-.5)<1e-6
 assert doc.getObject(name).Shape.distToShape(doc.getObject(boom).Shape)[0]<1e-6
native.set_parameter(doc,'ReferenceDimensions','BoomHalfSpacing',220)
fit=worker.check(physical,contacts,tolerance_mm=.001,require_connected=True)
assert fit['overlap_count']==0 and fit['unmet_contact_count']==0
exports=worker.save(str(OUT),physical)
image=worker.render(physical)
(OUT/'aircraft.png').write_bytes(base64.b64decode(image.pop('content_blocks')[0]['source']['data']))
(OUT/'render.json').write_text(json.dumps(image,indent=2))
report=dict(scope='Proposed top-stabilizer retention engineering on reconstructed full aircraft; not original-author hardware',source_sha256=source_hash,physical=physical,fin_physical_names=dict(Right=right_name,Left=left_name),stabilizer_physical_name=stab_name,bindings=bindings,fit=fit,fastener_checks=fastener_checks,hex_key_checks=key_checks,sampled_nut_insertion=insertion,exterior_difference_outside_interface_mm3=contour,edited_230mm_tail_fit=edited_fit,typed_boom_spacing_dependency_passed=True,exports=exports,fastener_sources=joint['fastener_sources'],provisional=dict(fastener_x_mm=stations,head_pad_radius_mm=4,head_pad_top_z_mm=134,nut_boss_radius_mm=5,nut_boss_bottom_z_mm=115,nut_pocket_af_mm=5.7,nut_pocket_z_mm=[118.8,121.4],nut_entry_slot_xyz_mm=[6.6,11,2.6],hole_diameter_mm=3.4,local_interface_windows=dict(x_half_width_mm=8,y_half_width_mm=12,z_mm=[108,142])),load_path=['Stabilizer head-bearing pads react against screw heads','Screw tension reacts through purchased nuts against fin-pocket ceilings','The existing conformal fin seat carries compression; four spaced screws prevent nominal lift-off','Shear/friction, preload, local boss/fin stresses and fatigue are not established by geometric contact'],assembly_order=['Insert nuts from inboard through side slots before screws; pocket floors support them','Seat stabilizer on conformal fin interfaces','Insert four screws from above and engage nuts; hex walls resist nut rotation','Use top-access hex keys; establish torque/preload only after material/tolerance and load validation'],limitations=['All hardware is nominal envelope geometry; thread helix/chamfers and real engagement tolerances are omitted','Side-entry nuts are supported axially by pocket floors/ceilings but can leave laterally when screws are removed','Sampled nut insertion and static hex-key envelopes are not a proof of complete serviceability','Boss strength, fin internal structure, preload, friction, fatigue, polymer creep and fabrication tolerances are unverified','Original wings/pod/boom group disconnections remain; motor contacts still lack engineered retention'],original_author_joint=False,full_aircraft_complete=False)
(OUT/'result.json').write_text(json.dumps(report,indent=2))
saved=Path(exports['native']);saved_hash=hashlib.sha256(saved.read_bytes()).hexdigest()
App.closeDocument(doc.Name);worker._doc=None;worker.restore(str(saved));doc=worker.active()
native.set_parameter(doc,'ReferenceDimensions','BoomHalfSpacing',230)
reopened=worker.check(tail_selection,contacts,tolerance_mm=.001,require_connected=True);assert reopened['passed']
assert hashlib.sha256(saved.read_bytes()).hexdigest()==saved_hash and hashlib.sha256(SOURCE.read_bytes()).hexdigest()==source_hash
report['saved_reopen_bilateral_edit_passed']=True;report['source_preserved']=True
(OUT/'result.json').write_text(json.dumps(report,indent=2))
print(json.dumps(dict(parts=len(physical),pairs=fit['pairs_total'],clashes=fit['overlap_count'],contacts_unmet=fit['unmet_contact_count'],groups=len(fit['components']),reopened_edit=True)))
