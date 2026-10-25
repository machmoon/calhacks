"""Integrate proposed native spar-bearing/boom clamps into the smooth aircraft.

Boom clamps are fastener-retained; closed spar collars provide radial bearing,
not independently verified axial spar retention or structural sizing.
"""
import base64,hashlib,json,math,os,sys,time
from pathlib import Path
import FreeCAD as App
import Part
import native,worker
ROOT=Path(__file__).resolve().parents[2];SOURCE=ROOT/os.environ.get('WRIGHT_HARDPOINT_SOURCE','out/tsung-vtol-smooth-aircraft-final/assembly.FCStd');OUT=ROOT/os.environ.get('WRIGHT_HARDPOINT_OUT','out/tsung-vtol-wing-boom-assembly');OUT.mkdir(parents=True,exist_ok=True)
prior=json.loads((SOURCE.parent/'result.json').read_text());digest=hashlib.sha256(SOURCE.read_bytes()).hexdigest();old_evidence=json.loads((ROOT/'out/tsung-vtol-wing-hardpoint-linked/result.json').read_text());started=time.time();report=dict(scope='Proposed spar-bearing and fastener-retained boom hardpoints on smooth aircraft',source=str(SOURCE),source_sha256=digest,prior_hardpoint_evidence=str(ROOT/'out/tsung-vtol-wing-hardpoint-linked/result.json'),status='starting')
def phase(text):
 report['status']=text;report['elapsed_seconds']=time.time()-started;(OUT/'progress.json').write_text(json.dumps(report,indent=2));print(text,flush=True)
def failure(kind,error,trace):
 report['status']='failed';report['failure']=dict(type=kind.__name__,message=str(error));(OUT/'failure.json').write_text(json.dumps(report,indent=2));sys.__excepthook__(kind,error,trace)
sys.excepthook=failure
phase('Loading final smooth source and checking actual datums')
worker.open_document(str(SOURCE));doc=worker.active();call=worker.METHODS
right_skin_name=os.environ.get('WRIGHT_HARDPOINT_RIGHT_SKIN','RightSkin');left_skin_name=os.environ.get('WRIGHT_HARDPOINT_LEFT_SKIN','LeftSkin')
for n in ['RightSpar_Outer','SparBoreTool','BoomRight_Outer','BoomLeft_Outer','LiftConformalBore','RightOuterContour',right_skin_name,left_skin_name]:assert doc.getObject(n) is not None,n
assert abs(doc.RightSpar_Outer.Radius.Value-2)<1e-8 and abs(doc.BoomRight_Outer.Radius.Value-5)<1e-8
before_skin=doc.getObject(right_skin_name).Shape.copy();outer_fingerprint=hashlib.sha256(doc.RightOuterContour.Shape.exportBrepToString().encode()).hexdigest()
call['make_parameters'](name='WingBoomMating',parameters=dict(SparRadius=dict(value=2,min=1.9,max=2.2),BoomRadius=dict(value=5,min=4.8,max=5.2),SplitGap=dict(value=.5,min=.2,max=.8)))
bindings=[]
def bind(o,p,source,parameter,scale=1,offset=0):
 r=call['bind_parameter'](object_name=o.Name,parameter=p,source_object=source,source_parameter=parameter,scale=scale,offset_mm=offset);bindings.append(r)
for name,source in [('RightSpar_Outer','SparRadius'),('SparBoreTool','SparRadius'),('BoomRight_Outer','BoomRadius'),('BoomLeft_Outer','BoomRadius'),('LiftConformalBore','BoomRadius')]:bind(doc.getObject(name),'Radius','WingBoomMating',source)
phase('Creating typed native right hardpoint and linked mating bores')
def make(n,k,p,xyz=(0,0,0),axis=None,follow=True):
 call['make_part'](spec=dict(name=n,kind=k,params=p));o=doc.getObject(n)
 rotation=dict(axis=[0,0,1],angle_degrees=0)
 if axis=='x':rotation=dict(axis=[0,1,0],angle_degrees=90)
 elif axis=='y':rotation=dict(axis=[1,0,0],angle_degrees=-90)
 if xyz!=(0,0,0) or axis:call['place'](object_name=n,position=list(xyz),**rotation)
 if follow:bind(o,'Placement.Base.y','ReferenceDimensions','BoomHalfSpacing',offset=xyz[1]-220)
 return o
def op(n,kind,a,b,retain=False):
 call['boolean'](name=n,operation=kind,base=a.Name,tool=b.Name,retain_tool_as_part=retain);return doc.getObject(n)
def hexagon(n,y,z,af,h):
 radius=af/math.sqrt(3)
 o=make(n,'loft',dict(rings=[[[400+radius*math.cos(t*math.tau/6),y+radius*math.sin(t*math.tau/6),zz] for t in range(6)] for zz in [z,z+h]]),follow=False)
 for section in o.Sections:bind(section,'Placement.Base.y','ReferenceDimensions','BoomHalfSpacing',offset=section.Placement.Base.y-220)
 return o
collar=make('WBBoomCollar','cylinder',dict(radius=9,height=20),(390,220,-10),'x');bind(collar,'Radius','WingBoomMating','BoomRadius',offset=4)
ears=make('WBBoomEars','box',dict(x=20,y=40,z=12),(390,200,-16));blank=op('WBBoomBlank','fuse',collar,ears)
up=make('WBUpperSpace','box',dict(x=30,y=50,z=60),(385,195,-9.75));bind(up,'Placement.Base.z','WingBoomMating','SplitGap',scale=.5,offset=-10)
down=make('WBLowerSpace','box',dict(x=30,y=50,z=11.75),(385,195,-22));bind(down,'Height','WingBoomMating','SplitGap',scale=-.5,offset=12)
upper=op('WBUpperBlank','common',blank,up);lower=op('WBLowerBlank','common',blank,down)
web=make('WBSparWeb','box',dict(x=8,y=16,z=29.5),(396,212,-5));upper=op('WBWebAndSaddle','fuse',upper,web)
ring=make('WBSparCollar','cylinder',dict(radius=4.5,height=16),(400,212,24.5),'y');bind(ring,'Radius','WingBoomMating','SparRadius',offset=2.5);upper=op('WBHardpointBlank','fuse',upper,ring)
bore=make('WBBoomBore','cylinder',dict(radius=5,height=800),(0,220,-10),'x');bind(bore,'Radius','WingBoomMating','BoomRadius')
upper=op('WBBoomSeat','cut',upper,bore);lower=op('WBLowerSeat','cut',lower,bore)
sparbore=make('WBSparBore','cylinder',dict(radius=2,height=40),(400,200,24.5),'y');bind(sparbore,'Radius','WingBoomMating','SparRadius');upper=op('WBSparSeat','cut',upper,sparbore)
screws=[];nuts=[];keys=[]
for i,y in enumerate([206,234]):
 hole=make(f'WBDrill{i}','cylinder',dict(radius=1.7,height=30),(400,y,-25));upper=op(f'WBUpperDrill{i}','cut',upper,hole);lower=op(f'WBLowerDrill{i}','cut',lower,hole)
 pocket=hexagon(f'WBNutPocket{i}',y,-6.4,5.7,2.5);upper=op('WBUpperFinal' if i==1 else 'WBNutSeat0','cut',upper,pocket)
 nut=hexagon(f'WBNutBlank{i}',y,-6.4,5.5,2.4);nutbore=make(f'WBNutBore{i}','cylinder',dict(radius=1.5,height=4.4),(400,y,-7.4));nuts.append(op(f'WBNut{i}','cut',nut,nutbore))
 shaft=make(f'WBShank{i}','cylinder',dict(radius=1.5,height=16),(400,y,-16));head=make(f'WBHead{i}','cylinder',dict(radius=2.75,height=3),(400,y,-19));screw=op(f'WBScrewBlank{i}','fuse',shaft,head);screws.append(op(f'WBScrew{i}','cut',screw,hexagon(f'WBSocket{i}',y,-19.1,2.5,1.4)));keys.append(hexagon(f'WBHexKey{i}',y,-59,2.4,41.3))
phase('Creating native left mirrors and actual retained skin passages')
right=[upper,lower]+screws+nuts;left=[]
for o in right:
 call['mirror'](name=o.Name+'Left',source=o.Name,normal=[0,1,0]);left.append(doc.getObject(o.Name+'Left'))
new_skin=op('WingBoomRightSkin','cut',doc.getObject(right_skin_name),upper,True)
call['mirror'](name='WingBoomLeftSkin',source=new_skin.Name,normal=[0,1,0])
new_names=[o.Name for o in right+left];replace={right_skin_name:new_skin.Name,left_skin_name:'WingBoomLeftSkin'}
physical=[replace.get(n,n) for n in prior['physical']]+new_names;contacts=[[replace.get(a,a),replace.get(b,b)] for a,b in prior['contacts']]
for side,parts in [('Right',right),('Left',left)]:
 u,l,s0,s1,n0,n1=parts;contacts.extend([['Boom'+side,u.Name],['Boom'+side,l.Name],[side+'Spar',u.Name]])
 for s,n in [(s0,n0),(s1,n1)]:contacts.extend([[s.Name,l.Name],[n.Name,u.Name],[s.Name,n.Name]])
assert len(set(physical))==len(physical)
report.update(physical=physical,contacts=contacts,added_physical=new_names,skin_outputs=dict(Right=new_skin.Name,Left='WingBoomLeftSkin'),bindings=bindings,spanwise_collar_length_mm=16,prior_collar_length_mm=20)
phase('Checking nominal full aircraft and connected wing-to-boom load path')
fit=worker.check(physical,contacts,tolerance_mm=.001,require_connected=True);report['fit']=fit
assert fit['overlap_count']==0 and fit['unmet_contact_count']==0,[(p['a'],p['b'],p['overlap_mm3'],p['distance_mm']) for p in fit['pairs'] if p['clash'] or p['floating']]
wing_component=next(c for c in fit['components'] if 'RightSpar' in c);assert all(n in wing_component for n in ['LeftSpar','BoomRight','BoomLeft','FinTopRetentionRight','FinTopRetentionLeft'])
phase('Verifying explicit skin passage and local clamp travel/access')
intersection=before_skin.common(upper.Shape).Volume;assert intersection>1e-6
preserved=dict(added_mm3=new_skin.Shape.cut(before_skin).Volume,unaccounted_removed_mm3=before_skin.cut(upper.Shape).cut(new_skin.Shape).Volume)
assert max(preserved.values())<1e-5
assert hashlib.sha256(doc.RightOuterContour.Shape.exportBrepToString().encode()).hexdigest()==outer_fingerprint
report.update(skin_hardpoint_intersection_mm3=intersection,skin_change_outside_hardpoint=preserved,outer_contour_unchanged=True)
def overlap(a,b):
 aa=a.BoundBox;bb=b.BoundBox
 if any(getattr(aa,k+'Max')<getattr(bb,k+'Min') or getattr(bb,k+'Max')<getattr(aa,k+'Min') for k in ['X','Y','Z']):return 0.
 return a.common(b).Volume
def access():
 rows=[]
 for side in ['Right','Left']:
  m=App.Matrix()
  if side=='Left':m.A22=-1
  for key in keys:
   shape=key.Shape.transformGeometry(m)
   for name in physical:
    v=overlap(shape,doc.getObject(name).Shape);assert v<1e-6,(side,key.Name,name,v);rows.append(dict(side=side,key=key.Name,part=name,overlap_mm3=v))
 return rows
report['tool_access']=access();closure=[]
for side,parts in [('Right',right),('Left',left)]:
 u,l=parts[:2];boom=doc.getObject('Boom'+side).Shape;gap=u.Shape.distToShape(l.Shape)[0];assert abs(gap-.5)<1e-6
 moved=l.Shape.copy();moved.translate(App.Vector(0,0,-.4));opening=moved.distToShape(boom)[0];assert opening>0
 moved=l.Shape.copy();moved.translate(App.Vector(0,0,.05));stop=moved.common(boom).Volume;assert stop>1e-4
 closure.append(dict(side=side,split_mm=gap,opening_0_4_clearance_mm=opening,closure_0_05_interference_mm3=stop))
report['clamp_travel']=closure
# Static keepout is distinct from physical assembly: no invented servo bracket.
report['servo_keepout_to_new_hardware']=[dict(part=n,overlap_mm3=overlap(doc.MG90SStaticEnvelope.Shape,doc.getObject(n).Shape)) for n in new_names]
assert max(p['overlap_mm3'] for p in report['servo_keepout_to_new_hardware'])<1e-6
phase('Checking all rotor envelopes against the integrated physical assembly')
def rotor_checks():
 rows=[];sweeps=['SweepFR','SweepFL','SweepRR','SweepRL','PusherSweep'];own={'SweepFR':'AOS2807Threaded3','SweepFL':'LiftFLMotor','SweepRR':'LiftRRMotor','SweepRL':'LiftRLMotor','PusherSweep':'PusherMotor'}
 for name in sweeps:
  a=doc.getObject(name).Shape
  for part in physical:
   if part==own[name]:continue
   b=doc.getObject(part).Shape;v=overlap(a,b);distance=a.distToShape(b)[0];rows.append(dict(sweep=name,part=part,overlap_mm3=v,distance_mm=distance));assert v<1e-6 and distance>1e-6,(name,part,v,distance)
 between=[]
 for i,name in enumerate(sweeps):
  for other in sweeps[i+1:]:
   a=doc.getObject(name).Shape;b=doc.getObject(other).Shape;v=overlap(a,b);distance=a.distToShape(b)[0];assert v<1e-6 and distance>1e-6
   between.append(dict(a=name,b=other,overlap_mm3=v,distance_mm=distance))
 return dict(sweep_to_physical=rows,rotor_to_rotor=between,own_motor_exclusions=own)
report['rotors']=rotor_checks()
phase('Testing linked boom spacing and mating-radius edits')
for obj,p,v in [('ReferenceDimensions','BoomHalfSpacing',230),('WingBoomMating','SparRadius',2.1),('WingBoomMating','BoomRadius',5.1)]:call['set_parameter'](object_name=obj,parameter=p,value=v,summary=True)
for n in ['RightSpar_Outer','SparBoreTool','WBSparBore']:assert abs(doc.getObject(n).Radius.Value-2.1)<1e-7
for n in ['BoomRight_Outer','BoomLeft_Outer','LiftConformalBore','ConformalBoomSeat','WBBoomBore']:assert abs(doc.getObject(n).Radius.Value-5.1)<1e-7
edited=worker.check(physical,contacts,tolerance_mm=.001,require_connected=True);assert edited['overlap_count']==0 and edited['unmet_contact_count']==0
assert any(all(n in c for n in ['RightSpar','LeftSpar','BoomRight','BoomLeft']) for c in edited['components'])
report['edited_fit']=edited;report['edited_tool_access']=access();report['edited_rotors']=rotor_checks();report['edited_rib2_hardpoint_clearance_mm']=upper.Shape.distToShape(doc.RightRib2.Shape)[0];assert report['edited_rib2_hardpoint_clearance_mm']>0
for obj,p,v in [('WingBoomMating','SparRadius',2),('WingBoomMating','BoomRadius',5),('ReferenceDimensions','BoomHalfSpacing',220)]:call['set_parameter'](object_name=obj,parameter=p,value=v,summary=True)
report['fit']=worker.check(physical,contacts,tolerance_mm=.001,require_connected=True)
report.update(limitations=['Boom clamps are fastener-retained; closed spar collars provide radial bearing only. Spar axial retention/bonding remains unresolved.','OD4/ID3 spar, printed web and reduced16mm collar span are not structurally sized.','Zero-clearance skin passage needs seal/adhesive tolerance and local reinforcement design.','Hardware envelopes omit thread helices, preload and manufacturing tolerance.','Wing-to-pod attachment remains a separate integration task; geometric graph connectivity does not establish complete kinematic retention or strength.','Motor engagement depth remains provisional; rotor margin is geometric only, not dynamic blade-deflection allowance.'],manufacturing_complete=False,flight_validated=False,fastener_sources=json.loads((ROOT/'out/tsung-vtol-tail-joint-v2/result.json').read_text())['fastener_sources'])
(OUT/'checks-before-export.json').write_text(json.dumps(report,indent=2));phase('Saving native aircraft, STEP and GLB')
report['exports']=worker.save(str(OUT),physical)
try:
 image=worker.render(physical,deflection_mm=10,max_triangles=100000);(OUT/'aircraft.png').write_bytes(base64.b64decode(image.pop('content_blocks')[0]['source']['data']));(OUT/'render.json').write_text(json.dumps(image,indent=2))
except ValueError as e:report['render_failure']=str(e)
(OUT/'result.json').write_text(json.dumps(report,indent=2));saved=Path(report['exports']['native']);saved_hash=hashlib.sha256(saved.read_bytes()).hexdigest();App.closeDocument(doc.Name);worker._doc=None;worker.restore(str(saved));doc=worker.active()
phase('Verifying reopened typed dependencies')
for obj,p,v in [('ReferenceDimensions','BoomHalfSpacing',230),('WingBoomMating','SparRadius',2.1),('WingBoomMating','BoomRadius',5.1)]:call['set_parameter'](object_name=obj,parameter=p,value=v,summary=True)
reopened=worker.check(physical,contacts,tolerance_mm=.001,require_connected=True);assert reopened['overlap_count']==0 and reopened['unmet_contact_count']==0
assert any(all(n in c for n in ['RightSpar','LeftSpar','BoomRight','BoomLeft']) for c in reopened['components'])
assert hashlib.sha256(saved.read_bytes()).hexdigest()==saved_hash and hashlib.sha256(SOURCE.read_bytes()).hexdigest()==digest
report.update(reopened_parameter_edits_passed=True,source_preserved=True,status='complete',elapsed_seconds=time.time()-started);(OUT/'result.json').write_text(json.dumps(report,indent=2));print('COMPLETE',flush=True)
