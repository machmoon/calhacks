"""Proposed root wing-to-pod joint built through bounded native worker actions.

Measured source datums, nominal manufacturer fasteners, conformal seats and
explicit access paths. Not original-author hardware or a sized flight structure.
"""
import hashlib,json,math,os,sys,time
from pathlib import Path
import FreeCAD as App
import Part
import native,worker,exchange,surface_preview,layout
ROOT=Path(__file__).resolve().parents[2];OUT=ROOT/os.environ.get('WRIGHT_JOINT_OUTPUT','out/tsung-vtol-wing-pod-joint');OUT.mkdir(parents=True,exist_ok=True)
SOURCE=ROOT/'out/tsung-vtol-smooth-aircraft-final/assembly.FCStd';prior=json.loads((SOURCE.parent/'result.json').read_text())
probe=json.loads((OUT/'source-probe.json').read_text());digest=hashlib.sha256(SOURCE.read_bytes()).hexdigest();assert digest==probe['sha256']
native_build=os.environ.get('WRIGHT_JOINT_NATIVE_BUILD')=='1'
started=time.time();trace=[];pending_bindings=[];batch_bindings=os.environ.get('WRIGHT_JOINT_BATCH_BINDINGS')=='1';report=dict(source=str(SOURCE),source_sha256=digest,status='building',original_author_joint=False,structurally_sized=False)
def phase(s):
 report['status']=s;(OUT/'progress.json').write_text(json.dumps(report,indent=2));print(s,flush=True)
def failed(kind,error,tb):
 report.update(status='failed',failure=str(error));(OUT/'failure.json').write_text(json.dumps(report,indent=2));sys.__excepthook__(kind,error,tb)
sys.excepthook=failed

def native_action(action,args):
 # Ordinary native authoring checks the newly created feature. Expensive exact
 # whole-document and assembly checks are explicitly deferred to the audit.
 if action=='open_document':
  worker._doc=App.openDocument(args['path']);return dict(path=args['path'],whole_document_audit='pending')
 d=worker.active()
 if action=='make_part':return native.make_part(d,args['spec'])
 if action=='make_parameters':return layout.create(d,args['name'],args['parameters'])
 if action=='place':
  o=d.getObject(args['object_name']);o.Placement.Base=App.Vector(*args['position']);d.recompute();return native.describe(o)
 if action=='bind_parameters':
  prepared=[layout._binding(d,**item) for item in args['bindings']]
  for o,p,e in prepared:o.setExpression(p,e)
  d.recompute();layout.validate(d.WingPodMating)
  return dict(binding_count=len(prepared),native_expressions=True,whole_document_audit='pending')
 if action in ('boolean','mirror'):
  if d.getObject(args['name']):raise ValueError('duplicate native feature name')
  if action=='boolean':
   o=d.addObject({'cut':'Part::Cut','fuse':'Part::Fuse','common':'Part::Common'}[args['operation']],args['name'])
   o.Base=d.getObject(args['base']);o.Tool=d.getObject(args['tool'])
  else:
   o=d.addObject('Part::Mirroring',args['name']);o.Source=d.getObject(args['source']);o.Normal=App.Vector(*args['normal']);o.Base=App.Vector(*args['origin'])
  d.recompute();evidence=native.describe(o)
  if evidence['solids']!=1:raise ValueError('new native feature is not one valid solid: '+o.Name)
  return evidence
 if action=='inspect':return [native.describe(d.getObject(n)) for n in args['objects']]
 raise ValueError('unknown native construction operation: '+action)

def call(action,**args):
 row=dict(action=action,args=args);action_started=time.time()
 try:result=native_action(action,args) if native_build else worker.METHODS[action](**args);row['result']=result
 except Exception as error:row['error']=str(error);raise
 finally:
  row['elapsed_seconds']=time.time()-action_started
  trace.append(row)
  with (OUT/('native-actions.jsonl' if native_build else 'typed-actions.jsonl')).open('a') as stream:stream.write(json.dumps(row,default=str)+'\n')
 return result
phase('Opening preserved smooth source')
call('open_document',path=str(SOURCE));doc=worker.active()
original={n:doc.getObject(n).Shape.copy() for n in prior['physical']}
outer_hash=hashlib.sha256(doc.RightOuterContour.Shape.cleaned().exportBrepToString().encode()).hexdigest()
rows=[next(r for r in probe['vertical_intersections_mm'] if r['x']==x and r['y']==30) for x in [285,370]]
datums=[r['RightOuterContour'][0] for r in rows];roofs=[r['PodClosedShell_Outer'][-1] for r in rows]
parameters=dict(HalfSpacing=dict(value=30,min=27,max=33),FrameThickness=dict(value=3,min=2.5,max=3.5))
for key,value in [('FrontDatum',datums[0]),('RearDatum',datums[1]),('BoltLength',20),('BoltDiameter',3),('HeadDiameter',5.5),('HeadHeight',3),('HoleRadius',1.7),('HeadWellRadius',3.1)]:parameters[key]=dict(value=value,min=value,max=value)
call('make_parameters',name='WingPodMating',parameters=parameters)

def bind(obj,prop,key,offset=0,scale=1):
 item=dict(object_name=obj.Name,parameter=prop,source_object='WingPodMating',source_parameter=key,offset_mm=offset,scale=scale)
 if batch_bindings:pending_bindings.append(item);return
 return call('bind_parameter',**item)
def flush_bindings():
 if not pending_bindings:return
 assert len(pending_bindings)<=128
 assert len({(b['object_name'],b['parameter']) for b in pending_bindings})==len(pending_bindings)
 call('bind_parameters',bindings=list(pending_bindings));pending_bindings.clear()
def make(name,kind,params,position=None):
 call('make_part',spec=dict(name=name,kind=kind,params=params));obj=doc.getObject(name)
 if position is not None:call('place',object_name=name,position=position)
 return obj
def cyl(name,r,h,x,y,z,index=None):
 obj=make(name,'cylinder',dict(radius=r,height=h),[x,y,z]);bind(obj,'Placement.Base.y','HalfSpacing',y-30)
 if index is not None:bind(obj,'Placement.Base.z',['FrontDatum','RearDatum'][index],z-datums[index])
 return obj
def box(name,x,y,z,sx,sy,sz,index=None,bind_y=False):
 obj=make(name,'box',dict(x=sx,y=sy,z=sz),[x,y,z])
 if bind_y:bind(obj,'Placement.Base.y','HalfSpacing',y-30)
 if index is not None:bind(obj,'Placement.Base.z',['FrontDatum','RearDatum'][index],z-datums[index])
 return obj
def hexagon(name,x,y,z,af,height,index):
 radius=af/math.sqrt(3);rings=[[[x+radius*math.cos(k*math.tau/6),y+radius*math.sin(k*math.tau/6),zz] for k in range(6)] for zz in [z,z+height]]
 obj=make(name,'loft',dict(rings=rings))
 for section in obj.Sections:
  bind(section,'Placement.Base.y','HalfSpacing',float(section.Placement.Base.y)-30)
  bind(section,'Placement.Base.z',['FrontDatum','RearDatum'][index],float(section.Placement.Base.z)-datums[index])
 return obj
def op(name,operation,a,b,retain=False):
 call('boolean',name=name,operation=operation,base=a.Name,tool=b.Name,retain_tool_as_part=retain);return doc.getObject(name)
def mirror(name,obj):
 call('mirror',name=name,source=obj.Name,normal=[0,1,0],origin=[0,0,0]);return doc.getObject(name)

skin=doc.RightSkin;pod=doc.Pod;posts=[];frames=[];screws=[];nuts=[];keys=[];bolt_sweeps=[];nut_sweeps=[];details=[]
for i,(x,z,roof) in enumerate(zip([285,370],datums,roofs)):
 datum=['FrontDatum','RearDatum'][i];phase('Constructing joint station '+str(x)+' through native FreeCAD features' if native_build else 'Constructing joint station '+str(x)+' through typed actions')
 # Ring-shaped transverse reinforcement follows the actual pod envelope.
 slab=box(f'WJFrameSlab{i}',x-1.5,-100,-130,3,200,180)
 bind(slab,'Length','FrameThickness');bind(slab,'Placement.Base.x','FrameThickness',x,-.5)
 blank=op(f'WJFrameBlank{i}','common',doc.PodClosedShell_Outer,slab)
 # Explicit clear service window; unchanged pod cavity is not called empty.
 bounds=native.shape_bounds(blank.Shape)
 half_y=.55*(bounds.YMax-bounds.YMin)/2;half_z=.55*(bounds.ZMax-bounds.ZMin)/2
 window=box(f'WJFrameWindow{i}',x-5,-half_y,-45-half_z,10,2*half_y,2*half_z)
 frame=op(f'WJFrame{i}','cut',blank,window);frames.append(frame)
 pod=op(f'WJPodFrameFuse{i}','fuse',pod,frame)
 # Integral wing boss stays inside the actual outer contour.
 boss_raw=cyl(f'WJBossBlank{i}',7,12,x,30,z-3,i)
 boss=op(f'WJBossConformal{i}','common',boss_raw,doc.RightOuterContour)
 skin=op(f'WJBossFuse{i}','fuse',skin,boss)
 bore=cyl(f'WJWingBore{i}',1.7,14,x,30,z-3,i);bind(bore,'Radius','HoleRadius')
 skin=op(f'WJWingDrill{i}','cut',skin,bore)
 pocket=hexagon(f'WJNutPocket{i}',x,30,z+4.6,5.7,2.6,i)
 skin=op(f'WJWingPocket{i}','cut',skin,pocket)
 entry=box(f'WJNutEntry{i}',x-3.3,-5,z+4.6,6.6,35,2.6,i)
 bind(entry,'Width','HalfSpacing',5)
 skin=op(f'WJWingEntry{i}','cut',skin,entry)
 # A conformal pylon top carries compression, not an overlap exemption.
 raw=cyl(f'WJPylonBlank{i}',9,z-roof+18,x,30,roof-15)
 bind(raw,'Height',datum,18-roof)
 seat=op(f'WJPylonRight{i}','cut',raw,doc.RightOuterContour)
 left=mirror(f'WJPylonLeft{i}',seat);posts.extend([seat,left])
 pod=op(f'WJPodPylonRFuse{i}','fuse',pod,seat);pod=op(f'WJPodPylonLFuse{i}','fuse',pod,left)
 drill=cyl(f'WJPodBoreRight{i}',1.7,40,x,30,z-20,i);bind(drill,'Radius','HoleRadius')
 drill_left=mirror(f'WJPodBoreLeft{i}',drill)
 access=cyl(f'WJHeadWellRight{i}',3.1,z+118,x,30,-130);bind(access,'Radius','HeadWellRadius');bind(access,'Height',datum,118)
 access_left=mirror(f'WJHeadWellLeft{i}',access)
 for suffix,tool in [('R',drill),('L',drill_left),('AccessR',access),('AccessL',access_left)]:pod=op(f'WJPodCut{i}{suffix}','cut',pod,tool)
 # Standard M3x20 full-thread nominal envelope; top protrudes1mm past nut.
 shaft=cyl(f'WJScrewShaft{i}',1.5,20,x,30,z-12,i);bind(shaft,'Radius','BoltDiameter',0,.5);bind(shaft,'Height','BoltLength')
 head=cyl(f'WJScrewHead{i}',2.75,3,x,30,z-15,i);bind(head,'Radius','HeadDiameter',0,.5);bind(head,'Height','HeadHeight')
 blank=op(f'WJScrewBlank{i}','fuse',shaft,head)
 socket=hexagon(f'WJScrewSocket{i}',x,30,z-15.1,2.5,1.4,i)
 screw=op(f'WJScrewRight{i}','cut',blank,socket);screws.extend([screw,mirror(f'WJScrewLeft{i}',screw)])
 nut_blank=hexagon(f'WJNutBlank{i}',x,30,z+4.6,5.5,2.4,i)
 thread=cyl(f'WJNutThread{i}',1.5,4.4,x,30,z+3.6,i);bind(thread,'Radius','BoltDiameter',0,.5)
 nut=op(f'WJNutRight{i}','cut',nut_blank,thread);nuts.extend([nut,mirror(f'WJNutLeft{i}',nut)])
 # Conservative external bolt sweep along its assembly axis, not modeled threads.
 sh=cyl(f'WJBoltSweepHead{i}',2.75,z+118,x,30,-130);bind(sh,'Height',datum,118)
 ss=cyl(f'WJBoltSweepShaft{i}',1.5,z+138,x,30,-130);bind(ss,'Height',datum,138)
 swept=op(f'WJBoltSweepRight{i}','fuse',sh,ss);bolt_sweeps.extend([swept,mirror(f'WJBoltSweepLeft{i}',swept)])
 # Actual112mm tool reach; elbow-turning reservation includes3mm margins.
 tip=z-13.75;base=tip-112
 stem=cyl(f'WJKeyStem{i}',1.5,110.75,x,30,base,i)
 key_tip=hexagon(f'WJKeyTip{i}',x,30,z-15,2.5,1.25,i)
 key=op(f'WJKeyBlank{i}','fuse',stem,key_tip)
 elbow=cyl(f'WJKeyElbowSweep{i}',22,6,x,30,base,i)
 key=op(f'WJKeyRight{i}','fuse',key,elbow);keys.extend([key,mirror(f'WJKeyLeft{i}',key)])
 # Exact XY convex hull of nominal filled hex at both translation endpoints;
 # filling the threaded hole makes this a conservative nut insertion sweep.
 radius=5.5/math.sqrt(3)
 points=[(x+radius*math.cos(k*math.tau/6),cy+radius*math.sin(k*math.tau/6)) for cy in [-5,30] for k in range(6)]
 points=sorted(set(points))
 def cross(a,b,c):return (b[0]-a[0])*(c[1]-a[1])-(b[1]-a[1])*(c[0]-a[0])
 lower=[]
 for p in points:
  while len(lower)>1 and cross(lower[-2],lower[-1],p)<=0:lower.pop()
  lower.append(p)
 upper=[]
 for p in reversed(points):
  while len(upper)>1 and cross(upper[-2],upper[-1],p)<=0:upper.pop()
  upper.append(p)
 hull=lower[:-1]+upper[:-1]
 swept=make(f'WJNutInsertionRight{i}','loft',dict(rings=[[[xx,yy,zz] for xx,yy in hull] for zz in [z+4.6,z+7]]))
 # Insertion proof is rebuilt from current nut position during parameter audit.
 nut_sweeps.append(swept)
 details.append(dict(x=x,nominal_y=30,measured_wing_lower_z=z,measured_pod_roof_z=roof,
    frame_window_xyz_mm=[10,2*half_y,2*half_z],nominal_wing_pod_gap_mm=z-roof,screw_length_mm=20,head_bearing_z=z-12,nut_floor_z=z+4.6,nut_top_z=z+7,tip_z=z+8,tip_protrusion_mm=1))
 if batch_bindings:
  phase(('Applying native expression batch for station ' if native_build else 'Applying atomic typed binding batch for station ')+str(x))
  flush_bindings()

right_name=skin.Name;left=mirror('WingPodLeftSkin',skin);left_name=left.Name;pod_name=pod.Name
replacement={'RightSkin':right_name,'LeftSkin':left_name,'Pod':pod_name}
physical=[replacement.get(n,n) for n in prior['physical']]+[o.Name for o in screws+nuts]
contacts=[[replacement.get(a,a),replacement.get(b,b)] for a,b in prior['contacts']]
contacts.extend([[pod_name,right_name],[pod_name,left_name]])
for i,(screw,nut) in enumerate(zip(screws,nuts)):
 side=right_name if i%2==0 else left_name
 contacts.extend([[screw.Name,pod_name],[nut.Name,side],[screw.Name,nut.Name]])
for obj in keys+bolt_sweeps+nut_sweeps:
 obj.addProperty('App::PropertyBool','NonphysicalKeepout','Wright');obj.NonphysicalKeepout=True
report.update(authoring_backend='FreeCAD Python native features' if native_build else 'typed worker actions',whole_document_audit='pending',batch_bindings=batch_bindings,physical=physical,contacts=contacts,replacements=replacement,station_details=details,
    wing_right_result=right_name,wing_left_result=left_name,pod_result=pod_name,
    source_outer_unchanged=hashlib.sha256(doc.RightOuterContour.Shape.cleaned().exportBrepToString().encode()).hexdigest()==outer_hash,
    merge_contract='Apply separate boom-passage cut to this right-skin result, then mirror the combined result; no source outer changes',
    assumptions=dict(boss_radius_mm=7,pylon_radius_mm=9,frame_thickness_mm=3,frame_window_half_extent_fraction=.55,head_well_radius_mm=3.1,clearance_hole_diameter_mm=3.4,nut_pocket_af_mm=5.7,nut_pocket_height_mm=2.6,root_nut_entry_width_mm=6.6),
    tool_source=dict(url='https://www.wera.de/en/tools/950-pkl-l-key-metric-chrome-plated',part='05022054001',hex_af_mm=2.5,long_length_mm=112,short_length_mm=18,representation='Conservative1.5mmradius shaft, nominalhex tip,22mmradius x6mm elbow turning reservation; no exact Hex-Plus/ball/bend CAD'),
    fastener_sources=[dict(url='https://www.accu.co.uk/metric-cap-head-screws/2802-SSC-M3-20-A2',model='M3x20 full-thread DIN912/ISO4762',head_diameter_mm=5.5,head_height_mm=3,socket_af_mm=2.5,socket_depth_mm=1.3,pitch_mm=.5),dict(url='https://media.distrelec.com/Web/Downloads/_t/ds/1241613_eng_tds.pdf',model='BossardBN629M3',af_mm=5.5,height_mm=2.4)],
    limitations=['Nominal thread cylinders omit helix/chamfers; real thread engagement/preload relies on selected matching hardware and requires validation','Tool check reserves nominal straight-axis reach and elbow turning space, not operator hand/torque ergonomics','Wing-root nuts install from root faces before wing halves are seated; no claim of in-place nut replacement','Internal spar axial retention remains unresolved; root bolts retain skin/boss assembly, not the loose spar','Materials, shell/frame stresses, fatigue, manufacturing tolerances and load capacity remain unsized','Pod underside ports are unsealed; environmental protection is unresolved'])
phase('Native joint built; saving unverified checkpoint')
call('inspect',objects=[right_name,left_name,pod_name]);worker.checkpoint(str(OUT/'unverified'))
(OUT/'construction.json').write_text(json.dumps(report,indent=2))
# Verification continues in a separately resumable script; construction itself
# is not labeled validated and no collision exemption is granted.
print('BUILD COMPLETE',flush=True)
