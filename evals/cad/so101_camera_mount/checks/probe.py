"""Independent native camera-mount checks; edits only an evaluator-owned copy."""
import argparse,hashlib,json,math,struct,sys,time
from pathlib import Path
import FreeCAD as A
import Part
HERE=Path(__file__).resolve().parent;ROOT=HERE.parents[3]
p=argparse.ArgumentParser();p.add_argument('--candidate',required=True);p.add_argument('--output',required=True);p.add_argument('--stage',choices=['baseline','reopen'],default='baseline');p.add_argument('--save-copy');args=p.parse_args()
contract=json.loads((HERE/'contract.json').read_text());camera=json.loads((ROOT/contract['camera_spec']).read_text());interface=json.loads((ROOT/contract['wrist_measurements']).read_text());out=Path(args.output);out.parent.mkdir(parents=True,exist_ok=True);start=time.time();report={'status':'running','stage':args.stage,'states':[],'scope':'Local native camera mount, preserved input assembly and static registered wrist STEP only; no whole-arm motion/load/manufacturing certification'}
def write():out.write_text(json.dumps(report,indent=2))
def digest(p):return hashlib.sha256(Path(p).read_bytes()).hexdigest()
def mat(o):
 m=o.getGlobalPlacement().toMatrix();return [getattr(m,'A'+str(i)+str(j)) for i in range(1,5) for j in range(1,5)]
def raw_mesh_hash(o):
 m=o.Mesh.copy();m.Placement=A.Placement();v,f=m.Topology;h=hashlib.sha256()
 for p in v:h.update(struct.pack('<ddd',p.x,p.y,p.z))
 for t in f:h.update(struct.pack('<III',*t))
 return h.hexdigest()
def shape_in(o,frame=None):
 s=o.Shape.copy();global_place=o.getGlobalPlacement()*o.Placement.inverse()*s.Placement
 s.Placement=(frame.inverse()*global_place) if frame else global_place
 return s

def bounds(s):
 b=s.optimalBoundingBox(False,False);return [b.XMin,b.YMin,b.ZMin,b.XMax,b.YMax,b.ZMax]
def overlap(a,b):
 x=bounds(a);y=bounds(b)
 if any(x[i+3]<y[i]-1e-7 or y[i+3]<x[i]-1e-7 for i in range(3)):return 0.
 return max(0.,a.common(b).Volume)
def cyl(r,z0,z1,x,y):return Part.makeCylinder(r,z1-z0,A.Vector(x,y,z0))
def solid(s):return not s.isNull() and s.isValid() and len(s.Solids)==1 and s.Volume>1e-6

def bore_faces(s,x,y,r):
 found=[]
 for face in s.Faces:
  surf=face.Surface
  if not isinstance(surf,Part.Cylinder):continue
  if not r-.05<=surf.Radius<=r+.6 or abs(abs(surf.Axis.z)-1)>1e-6:continue
  center=surf.Center
  if math.hypot(center.x-x,center.y-y)>.03:continue
  found.append({'radius_mm':surf.Radius,'center_xy_mm':[center.x,center.y],'bounds':bounds(face)})
 return found

def snapshot(d):
 rows={}
 for o in d.Objects:
  row={'type':o.TypeId,'label':o.Label,'placement':mat(o) if hasattr(o,'Placement') else None,'expressions':sorted([list(v) for v in o.ExpressionEngine]),'children':[c.Name for c in getattr(o,'Group',[])]}
  if hasattr(o,'Mesh'):row['mesh_hash']=raw_mesh_hash(o)
  # Container Shape is a derived aggregate of children, including newly added mount.
  # Do not access it: FreeCAD may materialize a transient _Part_ShapeCache.
  container=o.TypeId in {'App::Part','App::Origin','App::DocumentObjectGroup','App::LinkGroup'}
  if not container and hasattr(o,'Shape') and not o.Shape.isNull():row['brep_hash']=hashlib.sha256(o.Shape.exportBrepToString().encode()).hexdigest()
  if o.Name=='JointParameters':row['angles']={n:float(getattr(o,n)) for n in o.PropertiesList if o.getTypeIdOfProperty(n)=='App::PropertyAngle'}
  rows[o.Name]=row
 return rows

def preserve(original,d):
 actual=snapshot(d);bad=[]
 for n,before in original.items():
  after=actual.get(n)
  if after is None:bad.append({'name':n,'reason':'missing'});continue
  for k in ['type','label','expressions','mesh_hash','brep_hash','angles']:
   if before.get(k)!=after.get(k):bad.append({'name':n,'field':k})
  if before['placement'] is not None and max(abs(x-y) for x,y in zip(before['placement'],after['placement']))>1e-6:bad.append({'name':n,'field':'placement'})
  if not set(before['children'])<=set(after['children']):bad.append({'name':n,'field':'original_children_removed'})
 return {'passed':not bad,'original_objects':len(original),'failures':bad}
try:
 source=ROOT/contract['source_model'];assert digest(source)==contract['source_sha256']
 original_doc=A.openDocument(str(source));original=snapshot(original_doc);A.closeDocument(original_doc.Name)
 doc=A.openDocument(str(Path(args.candidate).resolve()));params=doc.getObject(contract['parameters'])
 for key in contract['length_properties']:
  assert params is not None and params.getTypeIdOfProperty(key)=='App::PropertyLength','missing native length '+key
 initial_t=float(params.PlateThickness);initial_h=float(params.StandoffHeight)
 visual=doc.getObject(interface['visual_object']);f=interface['attachment_part_frame'];rotation=A.Matrix()
 for col,key in enumerate(['u','v','outward_normal'],1):
  for row,value in enumerate(f[key],1):setattr(rotation,'A'+str(row)+str(col),value)
 attachment=visual.getGlobalPlacement()*A.Placement(A.Vector(*f['origin_mm']),A.Rotation(rotation))
 wrist=Part.read(interface['source_part_step']);assert solid(wrist)
 wrist.Placement=visual.getGlobalPlacement()*wrist.Placement
 wrist_attachment=wrist.copy();wrist_attachment.Placement=attachment.inverse()*wrist.Placement
 queries=[('baseline',initial_t,initial_h)] if args.stage=='baseline' else [('reopened',initial_t,initial_h),('thickness_edit',initial_t+1,initial_h),('standoff_edit',initial_t+1,initial_h+2)]
 previous_shape=None
 for name,t,h in queries:
  params.PlateThickness=t;params.StandoffHeight=h;doc.recompute();state={'name':name,'T_mm':t,'H_mm':h,'gates':{}}
  state['source_preservation']=preserve(original,doc);state['gates']['source_preserved']=state['source_preservation']['passed']
  mount=doc.getObject('CameraMount');plate=doc.getObject('CameraPlate');pf=doc.getObject('CameraPlateFrame').getGlobalPlacement();cf=doc.getObject('CameraFrame').getGlobalPlacement()
  full=shape_in(mount);local=shape_in(mount,pf);foot=shape_in(mount,attachment);pl=shape_in(plate,pf)
  state['mount']={'valid_single_solid':solid(full),'volume_mm3':full.Volume,'type':mount.TypeId,'faces':len(full.Faces)};state['gates']['native_mount']=state['mount']['valid_single_solid'] and mount.TypeId!='Mesh::Feature'
  b=bounds(pl);state['plate']={'bounds':b,'thickness_error_mm':abs((b[5]-b[2])-t),'outside_final_mm3':pl.cut(local).Volume}
  state['gates']['plate_dimensions_and_containment']=solid(pl) and abs(b[2])<1e-5 and state['plate']['thickness_error_mm']<1e-5 and state['plate']['outside_final_mm3']<1e-6
  relation=pf.inverse()*cf;state['camera_frame']={'translation':list(relation.Base),'angle_rad':relation.Rotation.Angle};state['gates']['camera_frame']=relation.Base.sub(A.Vector(0,0,t+h)).Length<1e-5 and abs(relation.Rotation.Angle)<1e-6
  posts=[];holes=[]
  for i,(x,y) in enumerate(camera['mounting_holes_board_xy_mm']):
   s=shape_in(doc.getObject('CameraStandoff'+str(i)),pf);bb=bounds(s);post={'index':i,'valid_single_solid':solid(s),'bounds':bb,'outside_final_mm3':s.cut(local).Volume,'bores':bore_faces(s,x,y,1.1)}
   post['passed']=post['valid_single_solid'] and abs(bb[2]-t)<1e-5 and abs(bb[5]-(t+h))<1e-5 and post['outside_final_mm3']<1e-6 and bool(post['bores']);posts.append(post)
   hole={'center_xy_mm':[x,y],'cylindrical_faces':bore_faces(local,x,y,1.1),'shaft_overlap_mm3':overlap(local,cyl(1.,-.1,t+h+.1,x,y))};hole['passed']=bool(hole['cylindrical_faces']) and hole['shaft_overlap_mm3']<1e-6;holes.append(hole)
  state['standoffs']=posts;state['camera_holes']=holes;state['gates']['standoffs_and_pattern']=h>=5 and all(p['passed'] for p in posts) and all(p['passed'] for p in holes)
  state['wrist']={'overlap_mm3':overlap(full,wrist),'gap_mm':full.distToShape(wrist)[0]};state['gates']['wrist_fit']=state['wrist']['overlap_mm3']<1e-6 and state['wrist']['gap_mm']<.03
  fasteners=[]
  for x in [-interface['spacing_mm']/2,interface['spacing_mm']/2]:
   faces=bore_faces(foot,x,0,1.6);row={'x_mm':x,'faces':faces}
   if faces:
    top=max(f['bounds'][5] for f in faces);end=max(top+20,bounds(foot)[5]+10)
    shaft=cyl(1.5,-3.5,end,x,0);head=cyl(2.75,top,top+3,x,0);driver=cyl(1.5,top+3,end,x,0)
    row.update(shaft_mount_overlap_mm3=overlap(foot,shaft),shaft_wrist_overlap_mm3=overlap(wrist_attachment,shaft),head_mount_overlap_mm3=overlap(foot,head),driver_mount_overlap_mm3=overlap(foot,driver),head_seat_z_mm=top)
    row['passed']=max(row[k] for k in ['shaft_mount_overlap_mm3','shaft_wrist_overlap_mm3','head_mount_overlap_mm3','driver_mount_overlap_mm3'])<1e-6
   else:row['passed']=False
   fasteners.append(row)
  state['wrist_fastener_paths']=fasteners;state['gates']['wrist_bores_and_nominal_fastener_paths']=all(r['passed'] for r in fasteners)
  board=Part.makeBox(25,23.862,1.12);board.Placement=cf
  lens=Part.makeCylinder(2.875,7.43,A.Vector(12.5,14.4,1.12));lens.Placement=cf*lens.Placement
  ref=doc.getObject('CameraReferenceEnvelope');ribbon=doc.getObject('CameraRibbonClearance');ribbon_world=shape_in(ribbon);ribbon_local=shape_in(ribbon,cf);rb=bounds(ribbon_local)
  ref_bounds=bounds(shape_in(ref,cf))
  state['camera_clearance']={'reference_local_bounds':ref_bounds,'declared_reference_overlap_mm3':overlap(full,shape_in(ref)),'board_overlap_mm3':overlap(full,board),'lens_axis_envelope_overlap_mm3':overlap(full,lens),'reference_label':ref.Label,'reference_visible':bool(ref.Visibility) if 'Visibility' in ref.PropertiesList else None,'reference_contains_board_mm3':board.cut(shape_in(ref)).Volume}
  exits=[]
  if rb[1]<0 and rb[4]>=-.1 and rb[3]>=0 and rb[0]<=25:exits.append(('bottom',rb[3]-rb[0]))
  if rb[4]>23.862 and rb[1]<=23.962 and rb[3]>=0 and rb[0]<=25:exits.append(('top',rb[3]-rb[0]))
  if rb[0]<0 and rb[3]>=-.1 and rb[4]>=0 and rb[1]<=23.862:exits.append(('left',rb[4]-rb[1]))
  if rb[3]>25 and rb[0]<=25.1 and rb[4]>=0 and rb[1]<=23.862:exits.append(('right',rb[4]-rb[1]))
  state['ribbon_clearance']={'bounds_in_camera_frame':rb,'mount_overlap_mm3':overlap(full,ribbon_world),'label':ribbon.Label,'exit_edges_and_widths_mm':exits,'width_mm':max([width for _,width in exits],default=0),'thickness_mm':rb[5]-rb[2],'scope':'Candidate-declared prototype routing reservation, not manufacturer bend-radius certification'}
  state['gates']['camera_reference_and_space']=('reference' in ref.Label.lower()) and state['camera_clearance']['reference_visible'] is not False and ref_bounds[0]<=1e-5 and ref_bounds[1]<=1e-5 and ref_bounds[3]>=25-1e-5 and ref_bounds[4]>=23.862-1e-5 and ref_bounds[5]-ref_bounds[2]>=11.3-1e-5 and state['camera_clearance']['declared_reference_overlap_mm3']<1e-6 and state['camera_clearance']['board_overlap_mm3']<1e-6 and state['camera_clearance']['lens_axis_envelope_overlap_mm3']<1e-6
  state['gates']['ribbon_space']=('reference' in ribbon.Label.lower() or 'clearance' in ribbon.Label.lower()) and state['ribbon_clearance']['mount_overlap_mm3']<1e-6 and state['ribbon_clearance']['width_mm']>=18-1e-5 and state['ribbon_clearance']['thickness_mm']>=2-1e-5
  if previous_shape is not None:
   difference=full.cut(previous_shape).Volume+previous_shape.cut(full).Volume;state['final_solid_change_mm3']=difference;state['gates']['real_final_geometry_edit']=difference>1e-4
  previous_shape=full.copy();state['passed']=all(state['gates'].values());report['states'].append(state);write();print(name,state['gates'],flush=True)
 if args.save_copy:doc.saveAs(str(Path(args.save_copy).resolve()));report['saved_copy']=str(Path(args.save_copy).resolve())
 report.update(status='complete',passed=all(s['passed'] for s in report['states']),elapsed_seconds=time.time()-start,limitations=['Static local interface only; other arm parts and whole-arm sweeps are not collision-certified','Nominal M3 path envelopes do not validate thread engagement, nut installation with motor6 present, screw length or torque','Source STEP registration proves measured bore/plane correspondence, not whole-part equivalence to mesh','Reference camera/ribbon geometry is simplified; no exact manufacturer CAD or guaranteed cable bend radius'])
 write()
except Exception as error:
 report.update(status='failed',passed=False,error=str(error),elapsed_seconds=time.time()-start);write();raise
