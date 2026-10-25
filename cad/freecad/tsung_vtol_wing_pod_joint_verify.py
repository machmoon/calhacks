"""Independent exact-geometry audit of the proposed typed root-joint build."""
import os,hashlib,json,math,sys,time
from pathlib import Path
import FreeCAD as App
import Part
import native,worker,exchange,surface_preview,layout
ROOT=Path(__file__).resolve().parents[2];OUT=ROOT/os.environ.get('WRIGHT_JOINT_OUTPUT','out/tsung-vtol-wing-pod-joint')
report=json.loads((OUT/'construction.json').read_text());physical=report['physical'];contacts=report['contacts'];started=time.time()
report['limitations'].append('This baseline has no installed battery, avionics, wiring or payload geometry; new pod frames and underside tool paths are not verified against that future equipment.')
report['limitations'].append('Side-loaded nut pockets provide bearing and antirotation with installed screws; loose nuts are not positively retained against escape through the insertion slots during disassembly.')
def write(): (OUT/'result.json').write_text(json.dumps(report,indent=2))
def phase(s): report['status']=s;write();print(s,flush=True)
def failed(kind,error,tb):
 report.update(status='failed',failure=str(error));write();sys.__excepthook__(kind,error,tb)
sys.excepthook=failed
doc=App.openDocument(str(OUT/'unverified/working.FCStd'));worker._doc=doc
report['whole_document_audit']='pending separate exact cut audit'
def shape(name):return doc.getObject(name).Shape

def overlap(a,b):
 ba=native.shape_bounds(a,conservative=True);bb=native.shape_bounds(b,conservative=True)
 if any(getattr(ba,k+'Max')<getattr(bb,k+'Min') or getattr(bb,k+'Max')<getattr(ba,k+'Min') for k in 'XYZ'):return 0.
 return a.common(b).Volume

def shifted(a,delta):
 result=a.copy();result.translate(App.Vector(*delta));return result

def hull_sweep(x,y,z):
 radius=5.5/math.sqrt(3);points=sorted(set((x+radius*math.cos(k*math.tau/6),cy+radius*math.sin(k*math.tau/6)) for cy in [-5,y] for k in range(6)))
 def cross(a,b,c):return (b[0]-a[0])*(c[1]-a[1])-(b[1]-a[1])*(c[0]-a[0])
 halves=[]
 for seq in [points,list(reversed(points))]:
  half=[]
  for p in seq:
   while len(half)>1 and cross(half[-2],half[-1],p)<=0:half.pop()
   half.append(p)
  halves.append(half[:-1])
 vertices=[App.Vector(xx,yy,z+4.6) for xx,yy in halves[0]+halves[1]]
 return Part.Face(Part.makePolygon(vertices+[vertices[0]])).extrude(App.Vector(0,0,2.4))

def local_audit(label):
 phase(label+': checking real bearing surfaces and access sweeps')
 rows=[];y=float(doc.WingPodMating.HalfSpacing)
 for i,station in enumerate(report['station_details']):
  x=station['x'];z=station['measured_wing_lower_z']
  for side,sign in [('Right',1),('Left',-1)]:
   wing=shape(report['wing_right_result'] if side=='Right' else report['wing_left_result']);pod=shape(report['pod_result'])
   screw_name=f'WJScrew{side}{i}';nut_name=f'WJNut{side}{i}';screw=shape(screw_name);nut=shape(nut_name)
   seat=shape(f'WJPylon{side}{i}');common=seat.common(wing)
   row=dict(station=i,side=side,seat_gap_mm=seat.distToShape(wing)[0],seat_overlap_mm3=common.Volume,seat_contact_area_mm2=common.Area)
   # Solid Common may omit lower-dimensional touching faces. A finite downward
   # perturbation supplies separate compression-bearing evidence, without
   # treating that volume/step ratio as certified contact surface area.
   row['seat_compression_step_mm']=.05
   row['seat_compression_interference_mm3']=overlap(shifted(wing,[0,0,-.05]),seat)
   row['seat_area_scope']='Raw solid Common area only; zero does not establish zero bearing area. Compression perturbation is reported separately.'
   assert row['seat_gap_mm']<=1e-6 and row['seat_overlap_mm3']<=1e-6 and row['seat_compression_interference_mm3']>5,row
   row['head_bearing_test_mm3']=overlap(shifted(screw,[0,0,.1]),pod)
   row['nut_floor_test_mm3']=overlap(shifted(nut,[0,0,-.1]),wing)
   rotated=nut.copy();rotated.rotate(App.Vector(x,sign*y,z),App.Vector(0,0,1),6)
   row['nut_antirotation_test_mm3']=overlap(rotated,wing)
   assert min(row[k] for k in ['head_bearing_test_mm3','nut_floor_test_mm3','nut_antirotation_test_mm3'])>1e-4,row
   row['lateral_bearing_tests_mm3']=[overlap(shifted(wing,[0,dy,0]),screw) for dy in [-.3,.3]]
   assert min(row['lateral_bearing_tests_mm3'])>1e-4,row
   row['tool_obstacles_mm3']={n:overlap(shape(f'WJKey{side}{i}'),shape(n)) for n in physical}
   assert max(row['tool_obstacles_mm3'].values())<=1e-6,row
   row['bolt_insertion_obstacles_mm3']={n:overlap(shape(f'WJBoltSweep{side}{i}'),shape(n)) for n in physical if n!=screw_name}
   assert max(row['bolt_insertion_obstacles_mm3'].values())<=1e-6,row
   sweep=hull_sweep(x,y,z)
   if side=='Left':
    matrix=App.Matrix();matrix.A22=-1;sweep=sweep.transformGeometry(matrix)
   obstacles=[report['wing_right_result'] if side=='Right' else report['wing_left_result'],side+'Spar']+[side+'Rib'+str(j) for j in range(6)]
   row['nut_preassembly_insertion_mm3']={n:overlap(sweep,shape(n)) for n in obstacles}
   assert max(row['nut_preassembly_insertion_mm3'].values())<=1e-6,row
   row['actual_tip_protrusion_mm']=native.shape_bounds(screw).ZMax-native.shape_bounds(nut).ZMax
   assert abs(row['actual_tip_protrusion_mm']-1)<1e-6,row
   rows.append(row)
 return rows

phase('Verifying native objects and preserved source')
report['physical_validity']=[native.describe(doc.getObject(n)) for n in physical]
assert all(r['valid'] and r['solids']==1 for r in report['physical_validity'])
source=App.openDocument(report['source']);old_result=json.loads((Path(report['source']).parent/'result.json').read_text())
unchanged={n:shape(n).exportBrepToString()==source.getObject(n).Shape.exportBrepToString() for n in old_result['physical'] if n not in report['replacements']}
assert all(unchanged.values()),[n for n,v in unchanged.items() if not v]
report['unchanged_other_parts']=unchanged
assert shape('RightOuterContour').exportBrepToString()==source.RightOuterContour.Shape.exportBrepToString()
phase('Verifying material modifications stay inside explicit local joint regions')
regions=[]
for row in report['station_details']:
 for sign in [-1,1]:regions.append(Part.makeBox(22,45,25,App.Vector(row['x']-11,-5 if sign==1 else -40,row['measured_wing_lower_z']-4)))
region=regions[0]
for other in regions[1:]:region=region.fuse(other)
wing=shape(report['wing_right_result']);old=source.RightSkin.Shape
outside=(wing.cut(old).fuse(old.cut(wing))).cut(region).Volume
extra=wing.cut(shape('RightOuterContour')).Volume
report['wing_locality']=dict(change_outside_joint_regions_mm3=outside,material_outside_outer_mm3=extra)
assert outside<=1e-6 and extra<=1e-6,report['wing_locality']
for i in range(2):assert shape(f'WJFrame{i}').cut(shape('PodClosedShell_Outer')).Volume<=1e-6
pod_regions=[Part.makeBox(22,220,200,App.Vector(row['x']-11,-110,-140)) for row in report['station_details']]
pod_region=pod_regions[0].fuse(pod_regions[1]);pod=shape(report['pod_result']);old_pod=source.Pod.Shape
report['pod_change_outside_joint_regions_mm3']=pod.cut(old_pod).fuse(old_pod.cut(pod)).cut(pod_region).Volume
assert report['pod_change_outside_joint_regions_mm3']<=1e-6
App.closeDocument(source.Name);App.setActiveDocument(doc.Name)
phase('Checking existing rotor sweep envelopes against every changed physical part')
changed=list(report['replacements'].values())+[n for n in physical if n.startswith(('WJScrew','WJNut'))]
rotor_rows=[]
for sweep_name in ['SweepFR','SweepFL','SweepRR','SweepRL','PusherSweep']:
 sweep=shape(sweep_name)
 for n in changed:
  target=shape(n);volume=overlap(sweep,target)
  ba=native.shape_bounds(sweep,conservative=True);bb=native.shape_bounds(target,conservative=True)
  lower=math.sqrt(sum(max(0,getattr(ba,k+'Min')-getattr(bb,k+'Max'),getattr(bb,k+'Min')-getattr(ba,k+'Max'))**2 for k in 'XYZ'))
  gap=lower if lower>1e-6 else sweep.distToShape(target)[0]
  rotor_rows.append(dict(sweep=sweep_name,part=n,overlap_mm3=volume,clearance_lower_bound_or_exact_mm=gap,broadphase=lower>1e-6))
  assert volume<=1e-6 and gap>1e-6,rotor_rows[-1]
report['rotor_vs_changed_parts']=rotor_rows
report['baseline_local']=local_audit('Baseline')
phase('Baseline: full physical assembly interference and required contacts')
report['baseline_fit']=worker.check(physical,contacts,tolerance_mm=.001)
write();assert report['baseline_fit']['passed'],[(p['a'],p['b'],p['overlap_mm3'],p['distance_mm']) for p in report['baseline_fit']['pairs'] if p['clash'] or p['floating']]
assert len(report['baseline_fit']['components'])==2,report['baseline_fit']['components']
phase('Testing native HalfSpacing edit 30 to 32 mm')
doc.WingPodMating.HalfSpacing=32;doc.recompute();layout.validate(doc.WingPodMating)
report['edited_local']=local_audit('HalfSpacing32')
report['edited_fit']=worker.check(physical,contacts,tolerance_mm=.001)
write();assert report['edited_fit']['passed']
doc.WingPodMating.HalfSpacing=30;doc.recompute();layout.validate(doc.WingPodMating)
phase('Saving native assembly and display GLB')
for obj in doc.Objects:
 if 'Visibility' in obj.PropertiesList:obj.Visibility=obj.Name in physical
doc.saveAs(str(OUT/'assembly.FCStd'));report['native']=str(OUT/'assembly.FCStd')
report['glb']=exchange.export_glb([doc.getObject(n) for n in physical],OUT/'assembly.glb',report['baseline_fit'])
report['step']='Not exported or checked for this candidate'
digest=hashlib.sha256((OUT/'assembly.FCStd').read_bytes()).hexdigest()
App.closeDocument(doc.Name);worker._doc=None;doc=App.openDocument(str(OUT/'assembly.FCStd'));worker._doc=doc
phase('Fresh reopen and linked parameter edit')
doc.WingPodMating.HalfSpacing=32;doc.recompute();layout.validate(doc.WingPodMating)
report['reopened_local']=local_audit('Reopened32')
assert abs(float(doc.WJScrewShaft0.Placement.Base.y)-32)<1e-6
assert abs(native.shape_bounds(shape('WJScrewLeft0')).Center.y+32)<1e-6
doc.WingPodMating.HalfSpacing=30;doc.recompute();layout.validate(doc.WingPodMating)
assert hashlib.sha256((OUT/'assembly.FCStd').read_bytes()).hexdigest()==digest
assert hashlib.sha256(Path(report['source']).read_bytes()).hexdigest()==report['source_sha256']
report.update(status='mechanical checks complete; exact cut audit pending',source_preserved=True,saved_native_sha256=digest,reopened_parameter_edit_passed=True,elapsed_seconds=time.time()-started)
write();print('MECHANICAL CHECKS COMPLETE; separate exact cut audit pending',flush=True)
