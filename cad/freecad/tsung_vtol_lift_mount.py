"""Proposed lift motor retention, manufacturer pattern and provisional engagement."""
import base64,hashlib,json,math
from pathlib import Path
import FreeCAD as App
import Part
import native,worker,layout
ROOT=Path(__file__).resolve().parents[2];OUT=ROOT/'out/tsung-vtol-lift-mount';OUT.mkdir(exist_ok=True,parents=True)
SOURCE=ROOT/'out/tsung-vtol-top-retention/assembly.FCStd';source_hash=hashlib.sha256(SOURCE.read_bytes()).hexdigest()
previous=json.loads((SOURCE.parent/'result.json').read_text());worker.open_document(str(SOURCE));doc=worker.active()
layout.create(doc,'LiftMountDimensions',dict(MotorBaseZ=dict(value=6,min=6,max=8),HoleRadius=dict(value=9.5,min=9.5,max=9.5),BlindDepth=dict(value=2.5,min=2.5,max=3),SplitGap=dict(value=.5,min=.2,max=1)))
bindings=[]
def bind(o,y):
 e=f'ReferenceDimensions.BoomHalfSpacing + ({y-220!r} mm)';o.setExpression('Placement.Base.y',e);bindings.append(dict(object=o.Name,property='Placement.Base.y',expression=e))
def make(n,k,p,xyz):
 native.make_part(doc,dict(name=n,kind=k,params=p));o=doc.getObject(n);o.Placement.Base=App.Vector(*xyz);bind(o,xyz[1]);doc.recompute();return o
def cyl(n,r,h,x,y,z,axis=False):
 o=make(n,'cylinder',dict(radius=r,height=h),(x,y,z))
 if axis:o.Placement.Rotation=App.Rotation(App.Vector(0,0,1),App.Vector(1,0,0));doc.recompute()
 return o
def box(n,x,y,z,a,b,c):return make(n,'box',dict(x=a,y=b,z=c),(x,y,z))
def hexagon(n,x,y,z,af,h):
 r=af/math.sqrt(3);native.make_part(doc,dict(name=n,kind='loft',params=dict(rings=[[[x+r*math.cos(t*math.tau/6),y+r*math.sin(t*math.tau/6),zz] for t in range(6)] for zz in [z,z+h]])))
 o=doc.getObject(n)
 for section in o.Sections:bind(section,y)
 doc.recompute();return o
def op(n,k,a,b):
 o=doc.addObject('Part::'+k,n);o.Base=a;o.Tool=b;doc.recompute();assert o.Shape.isValid() and len(o.Shape.Solids)==1,n;return o
def mirror(n,o,normal,origin):
 worker.METHODS['mirror'](name=n,source=o.Name,normal=normal,origin=origin);return doc.getObject(n)
def screw(n,x,y,z,length):
 shaft=cyl(n+'Shaft',1.5,length,x,y,z);head=cyl(n+'Head',2.75,3,x,y,z-3)
 blank=op(n+'Blank','Fuse',shaft,head);socket=hexagon(n+'Socket',x,y,z-3,2.5,1.3)
 return op(n,'Cut',blank,socket)
# Front-right master. All critical assumptions are explicit in report.
collar=cyl('LiftCollar',9,26,92,220,-10,True)
ears=box('LiftEars',92,200,-16,26,40,16)
blank=op('LiftClampBlank','Fuse',collar,ears)
plate=cyl('LiftPlate',19,6,105,220,0);plate.setExpression('Height','LiftMountDimensions.MotorBaseZ');doc.recompute()
blank=op('LiftClampPlate','Fuse',blank,plate)
bore=cyl('LiftConformalBore',5,40,85,220,-10,True)
blank=op('LiftClampBored','Cut',blank,bore)
upspace=box('LiftUpperSpace',80,190,-9.75,50,60,30);upspace.setExpression('Placement.Base.z','-10 mm + LiftMountDimensions.SplitGap / 2')
lowspace=box('LiftLowerSpace',80,190,-30,50,60,19.75);lowspace.setExpression('Height','20 mm - LiftMountDimensions.SplitGap / 2');doc.recompute()
upper=op('LiftUpperSplit','Common',blank,upspace);lower=op('LiftLowerSplit','Common',blank,lowspace)
body=cyl('AOS2807BodyEnvelope',16.45,20.8,105,220,6);body.setExpression('Placement.Base.z','LiftMountDimensions.MotorBaseZ')
shaft=cyl('AOS2807ShaftEnvelope',2.5,14.5,105,220,26.8);shaft.setExpression('Placement.Base.z','LiftMountDimensions.MotorBaseZ + 20.8 mm');doc.recompute()
motor=op('AOS2807Envelope','Fuse',body,shaft)
parts=[];keys=[];contacts=[];bearing=[];hole_offsets=[]
for i in range(4):
 t=math.pi/4+i*math.pi/2;dx=9.5*math.cos(t);dy=9.5*math.sin(t);x=105+dx;y=220+dy;hole_offsets.append([dx,dy])
 hole=cyl(f'LiftMotorClearance{i}',1.7,40,x,y,-20)
 upper=op(f'LiftUpperDrill{i}','Cut',upper,hole);lower=op(f'LiftLowerDrill{i}','Cut',lower,hole)
 access=cyl(f'LiftHeadAccess{i}',3,30,x,y,-30)
 upper=op(f'LiftUpperAccess{i}','Cut',upper,access);lower=op(f'LiftLowerAccess{i}','Cut',lower,access)
 thread=cyl(f'ProvisionalMotorThread{i}',1.5,2.5,x,y,6);thread.setExpression('Placement.Base.z','LiftMountDimensions.MotorBaseZ');thread.setExpression('Height','LiftMountDimensions.BlindDepth');doc.recompute()
 motor=op(f'AOS2807Threaded{i}','Cut',motor,thread)
 bolt=screw(f'LiftMotorScrew{i}',x,y,0,8);parts.append(bolt)
 keys.append(hexagon(f'LiftMotorKey{i}',x,y,-43,2.4,41.3))
for i,dy in enumerate([-16,16]):
 x=105;y=220+dy
 drill=cyl(f'LiftClampDrill{i}',1.7,30,x,y,-20)
 upper=op(f'LiftUpperBolt{i}','Cut',upper,drill);lower=op(f'LiftLowerBolt{i}','Cut',lower,drill)
 pocket=hexagon(f'LiftClampPocket{i}',x,y,-3.4,5.7,10)
 upper=op(f'LiftUpperPocket{i}','Cut',upper,pocket)
 nut=hexagon(f'LiftClampNutBlank{i}',x,y,-3.4,5.5,2.4)
 thread=cyl(f'LiftClampNutThread{i}',1.5,4,x,y,-4)
 nut=op(f'LiftClampNut{i}','Cut',nut,thread);bolt=screw(f'LiftClampScrew{i}',x,y,-16,16)
 parts.extend([bolt,nut]);keys.append(hexagon(f'LiftClampKey{i}',x,y,-59,2.4,41.3))
master=[upper,lower,motor]+parts
roles=['Upper','Lower','Motor']+[f'MotorScrew{i}' for i in range(4)]+['ClampScrew0','ClampNut0','ClampScrew1','ClampNut1']
instances={};key_instances={}
for code in ['FR','RR','FL','RL']:
 instances[code]={};key_instances[code]=[]
 for role,o in zip(roles,master):
  q=o
  if code=='RR':q=mirror('LiftRR'+role,o,[1,0,0],[345,0,0])
  elif code=='FL':q=mirror('LiftFL'+role,o,[0,1,0],[0,0,0])
  elif code=='RL':q=mirror('LiftRL'+role,doc.getObject('LiftRR'+role),[0,1,0],[0,0,0])
  instances[code][role]=q.Name
 # Tool envelopes are native mirrors too; only direct local keys require access tests, symmetric peers follow same transforms.
 print('Built lift mount '+code,flush=True)
physical=[n for n in previous['physical'] if not n.startswith('LiftMotor')]+[n for d in instances.values() for n in d.values()]
for p in previous['fit']['pairs']:
 if p['required_contact'] and p['a'] in physical and p['b'] in physical:contacts.append([p['a'],p['b']])
for code,d in instances.items():
 boom='Boom'+('Right' if code.endswith('R') else 'Left')
 contacts.extend([[d['Upper'],boom],[d['Lower'],boom],[d['Motor'],d['Upper']]])
 for i in range(4):contacts.extend([[d[f'MotorScrew{i}'],d['Upper']],[d[f'MotorScrew{i}'],d['Motor']]])
 for i in range(2):contacts.extend([[d[f'ClampScrew{i}'],d['Lower']],[d[f'ClampNut{i}'],d['Upper']],[d[f'ClampScrew{i}'],d[f'ClampNut{i}']]])
 # Moving motor up 11 mm creates physical plate room; sweep plane follows this explicit provisional layout change.
 sw=doc.getObject('Sweep'+code);sw.setExpression('Placement.Base.z','LiftMountDimensions.MotorBaseZ + 23.8 mm')
doc.recompute()
print('Checking full physical assembly',flush=True)
fit=worker.check(physical,contacts,tolerance_mm=.001,require_connected=True)
assert fit['overlap_count']==0 and fit['unmet_contact_count']==0,[(p['a'],p['b'],p['overlap_mm3'],p['distance_mm']) for p in fit['pairs'] if p['clash'] or p['floating']]
print("Physical pairs passed; checking bearing and tool access",flush=True)
checks=[]
for i in range(4):
 bolt=doc.getObject(instances['FR'][f'MotorScrew{i}']);m=bolt.Shape.copy();m.translate(App.Vector(0,0,.1));v=m.common(upper.Shape).Volume;assert v>1e-4
 checks.append(dict(kind='motor_head_up_0_1_stop',index=i,overlap_mm3=v))
for i in range(2):
 bolt=doc.getObject(instances['FR'][f'ClampScrew{i}']);m=bolt.Shape.copy();m.translate(App.Vector(0,0,.1));v=m.common(lower.Shape).Volume;assert v>1e-4
 checks.append(dict(kind='clamp_head_up_0_1_stop',index=i,overlap_mm3=v))
key_checks=[]
for key in keys:
 for n in physical:
  v=key.Shape.common(doc.getObject(n).Shape).Volume;assert v<1e-6,(key.Name,n,v)
  key_checks.append(dict(tool=key.Name,part=n,overlap_mm3=v))
print("Master tool access passed; checking rotor envelopes",flush=True)
sweep_checks=[]
for code,d in instances.items():
 sweep=doc.getObject('Sweep'+code)
 for n in physical:
  if n==d['Motor']:continue # own shaft crosses its propeller plane intentionally
  dist=sweep.Shape.distToShape(doc.getObject(n).Shape)[0];v=sweep.Shape.common(doc.getObject(n).Shape).Volume
  sweep_checks.append(dict(sweep=sweep.Name,part=n,distance_mm=dist,overlap_mm3=v))
# Do not conceal aircraft-level prop/wing collisions if the reference layout has them.
local=[n for n in physical if n not in ['WingRight','WingLeft','Pod','PusherMotor']]
print('Rotor envelope checks recorded; testing spacing edit',flush=True)
native.set_parameter(doc,'ReferenceDimensions','BoomHalfSpacing',230)
edit=worker.check(local,contacts,tolerance_mm=.001,require_connected=True);assert edit['passed']
for code,d in instances.items():assert abs(doc.getObject(d['Motor']).Shape.Solids[0].CenterOfMass.y-(230 if code.endswith('R') else -230))<1e-6
native.set_parameter(doc,'ReferenceDimensions','BoomHalfSpacing',220)
fit=worker.check(physical,contacts,tolerance_mm=.001,require_connected=True)
report=dict(scope='Proposed split boom clamp with AOS2807 manufacturer mounting pattern; engagement depth provisional',source_sha256=source_hash,physical=physical,instances=instances,fit=fit,contacts=contacts,bindings=bindings,bearing_checks=checks,key_checks=key_checks,sweep_checks=sweep_checks,spacing_edit_fit=edit,manufacturer_verified=dict(model='Original AOS Supernova 2807-1400KV',body_diameter_mm=32.9,body_length_mm=20.8,shaft_extension_mm=14.5,shaft_outer_diameter_mm=5,mount_thread='4 x M3',opposite_hole_span_mm=19,hole_offsets_xy_mm=hole_offsets),provisional=dict(motor_base_z_mm=6,plate_thickness_mm=6,thread_engagement_mm=2,blind_depth_mm=2.5,split_gap_mm=.5,prop_plane_above_motor_base_mm=23.8),missing_manufacturer_data=['Permitted screw engagement/max insertion depth','Threaded base exact solid/void topology','Wire exit and moving bell detailed clearances'],limitations=['Motor is a dimensional envelope with provisional thread sockets, not manufacturer CAD','Clamping friction, preload, CFRP crushing, strength, vibration and fatigue unverified','No claim of original-author mount design or flight readiness','Own motor shaft intentionally intersects its own prop sweep plane; other sweep intersections are reported'],sources=dict(motor_page='https://www.aos-rc.com/old-designs/aos-supernova-2807-1400kv',motor_drawing='https://e57211f4-bc64-4c8a-825a-4812477417e7.usrfiles.com/ugd/e57211_c97fdf9a27dd46d393dbd561417428ba.pdf',motor_screws='https://www.accu.co.uk/metric-cap-head-screws/250416-SSCF-M3-8-A2-R360',clamp_hardware=previous['fastener_sources']))
(OUT/'checks-before-export.json').write_text(json.dumps(report,indent=2))
print('Native checks complete; exporting',flush=True)
report['exports']=worker.save(str(OUT),physical)
try:
 r=worker.render(physical,deflection_mm=10.0,max_triangles=100000);(OUT/'aircraft.png').write_bytes(base64.b64decode(r.pop('content_blocks')[0]['source']['data']));(OUT/'render.json').write_text(json.dumps(r,indent=2))
except ValueError as error:
 report['full_render_failure']=str(error)
 (OUT/'render-failure.json').write_text(json.dumps(dict(error=str(error),deflection_mm=10,max_triangles=100000),indent=2))
(OUT/'result.json').write_text(json.dumps(report,indent=2))
saved=Path(report['exports']['native']);saved_hash=hashlib.sha256(saved.read_bytes()).hexdigest();App.closeDocument(doc.Name);worker._doc=None;worker.restore(str(saved));doc=worker.active()
native.set_parameter(doc,'ReferenceDimensions','BoomHalfSpacing',230);reopened=worker.check(local,contacts,tolerance_mm=.001,require_connected=True);assert reopened['passed']
assert hashlib.sha256(saved.read_bytes()).hexdigest()==saved_hash and hashlib.sha256(SOURCE.read_bytes()).hexdigest()==source_hash
report.update(saved_reopen_spacing_edit_passed=True,source_preserved=True);(OUT/'result.json').write_text(json.dumps(report,indent=2));print('COMPLETE',flush=True)
