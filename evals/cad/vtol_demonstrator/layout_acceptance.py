"""Focused inverted-V appearance/layout acceptance, not aircraft qualification."""
import argparse,ast,hashlib,importlib.util,json,math,sys,traceback
from pathlib import Path
ROOT=Path(__file__).resolve().parents[3];sys.path.insert(0,str(ROOT/'cad/freecad'))
import FreeCAD as App
import Part,native
from assembly_frames import world_shape
sha=lambda p:hashlib.sha256(Path(p).read_bytes()).hexdigest()
TAIL=['TailSurfaceRight','TailSurfaceLeft','RuddervatorRight','RuddervatorLeft']
KEEP=['WingletRight','WingletLeft','CruiseMotor','CruiseProp']
OLD={'FinRight','FinLeft','RudderRight','RudderLeft','Stabilizer','Elevator'}
def section(shape,y):
 wires=shape.slice(App.Vector(0,1,0),y)
 if not wires:raise ValueError('Missing actual tail cross-section at y='+str(y))
 b=native.shape_bounds(Part.makeCompound(wires));return {'y_mm':y,'z_mid_mm':(b.ZMin+b.ZMax)/2,'x_min_mm':b.XMin,'x_max_mm':b.XMax,'z_min_mm':b.ZMin,'z_max_mm':b.ZMax}
def mirrored_difference(right,left):
 matrix=App.Matrix();matrix.A22=-1
 mirrored=right.transformGeometry(matrix)
 return {'right_reflected_minus_left_mm3':abs(mirrored.cut(left).Volume),'left_minus_right_reflected_mm3':abs(left.cut(mirrored).Volume)}
def evaluate(source,manifest,output):
 source,manifest,output=map(lambda p:Path(p).resolve(),[source,manifest,output]);before=sha(source)
 r={'native_sha256':before,'manifest_sha256':sha(manifest),'auditor_sha256':sha(__file__),'world_frame_helper_sha256':sha(ROOT/'cad/freecad/assembly_frames.py'),'passed':False,'scope':'Native inverted-V layout and retained component accounting only; not full collision, hinge, structure, control mixing, propulsion or flight qualification'}
 try:
  m=json.loads(manifest.read_text())
  if m['native_sha256']!=before:raise ValueError('Manifest/source hash mismatch')
  module=Path(m['proxy_module']);module=module if module.is_absolute() else manifest.parent/module
  pinned={v for k,v in m['generator_sources'].items() if Path(k).name==module.name}
  if pinned!={sha(module)}:raise ValueError('Unpinned or mismatched native feature dependency')
  spec=importlib.util.spec_from_file_location(module.stem,module);loaded=importlib.util.module_from_spec(spec);sys.modules[module.stem]=loaded;spec.loader.exec_module(loaded)
  r['dependency']={'path':str(module),'sha256':sha(module)}
  doc=App.openDocument(str(source));doc.recompute();physical=m['physical'];collision=m['collision_objects'];checks={}
  checks['old_tail_outputs_absent']=not OLD.intersection(physical)
  checks['four_tail_outputs_physical']=all(n in physical for n in TAIL)
  rows={};shapes={}
  for name in TAIL+KEEP:
   obj=doc.getObject(name)
   if obj is None:raise ValueError('Missing native object '+name)
   native.validate_live_feature(obj);shape=world_shape(obj);shapes[name]=shape;b=native.shape_bounds(shape)
   rows[name]={'valid':not shape.isNull() and shape.isValid(),'solids':len(shape.Solids),'volume_mm3':shape.Volume,'bounds_mm':[b.XMin,b.YMin,b.ZMin,b.XMax,b.YMax,b.ZMax],'collision_accounted':name in collision}
  r['objects']=rows
  checks['native_tail_singles']=all(rows[n]['valid'] and rows[n]['solids']==1 for n in TAIL)
  checks['retained_geometry']=all(rows[n]['valid'] and rows[n]['solids']>=1 for n in KEEP)
  checks['retained_collision_accounting']=all(rows[n]['collision_accounted'] for n in TAIL+KEEP)
  checks['winglets_physical']=all(n in physical for n in KEEP[:2])
  half=float(doc.DesignParameters.BoomHalfSpacing.Value);angle=float(doc.DesignParameters.TailDihedralAngle.Value)
  if not 20<=angle<=40 or half<=0:raise ValueError('Unsupported inverted-V angle/spacing')
  r['actual_sections']={};r['section_slopes_deg']={}
  for name in TAIL:
   sign=1 if name.endswith('Right') else -1
   samples=[section(shapes[name],sign*half*f) for f in [.9,.7,.5,.3,.15,.05,.01]]
   slopes=[math.degrees(math.atan2(b['z_mid_mm']-a['z_mid_mm'],abs(b['y_mm']-a['y_mm']))) for a,b in zip(samples,samples[1:])]
   r['actual_sections'][name]=samples;r['section_slopes_deg'][name]=slopes
   checks[name+'_rises_inward']=all(abs(s-angle)<.5 for s in slopes)
   checks[name+'_reaches_center']=abs(native.shape_bounds(shapes[name]).YMin if sign>0 else native.shape_bounds(shapes[name]).YMax)<.01
  r['mirror_differences']={}
  for right,left in [('TailSurfaceRight','TailSurfaceLeft'),('RuddervatorRight','RuddervatorLeft'),('WingletRight','WingletLeft')]:
   difference=mirrored_difference(shapes[right],shapes[left]);r['mirror_differences'][right]=difference;checks[right+'_mirror_consistency']=max(difference.values())<1e-4
  r['boom_attachment']={}
  for side in ['Right','Left']:
   tail=shapes['TailSurface'+side];boom=world_shape(doc.getObject('Boom'+side));distance=tail.distToShape(boom)[0];overlap=abs(tail.common(boom).Volume)
   r['boom_attachment'][side]={'distance_mm':distance,'overlap_mm3':overlap};checks[side+'_boom_bearing']=distance<.1 and overlap<1e-4
  roofs=[]
  for name in physical:
   shape=world_shape(doc.getObject(name));b=native.shape_bounds(shape)
   if b.XMax<850 or b.YMin>half*.6 or b.YMax<-half*.6:continue
   for index,face in enumerate(shape.Faces):
    b=native.shape_bounds(face)
    if b.XMin>850 and b.ZMin>80 and b.ZLength<1e-4 and b.YLength>half*.1 and b.YMin<half*.6 and b.YMax>-half*.6:roofs.append({'object':name,'face':index+1,'bounds_mm':[b.XMin,b.YMin,b.ZMin,b.XMax,b.YMax,b.ZMax]})
  r['horizontal_center_roof_faces']=roofs;checks['no_flat_center_roof']=not roofs
  macro=Path(m['open_macro']);macro=macro if macro.is_absolute() else manifest.parent/macro
  visible=None
  for node in ast.walk(ast.parse(macro.read_text())):
   if isinstance(node,ast.Assign) and any(isinstance(t,ast.Name) and t.id=='visible' for t in node.targets):visible=ast.literal_eval(node.value)
  checks['open_macro_configures_retained_visibility']=isinstance(visible,list) and all(n in visible for n in TAIL+KEEP)
  r['visibility_scope']='Shipped native GUI-opening macro explicitly enables these objects. Headless FCStd has no GuiDocument.xml; actual screen visibility/occlusion is not assessed here.'
  r['checks']=checks;r['passed']=all(checks.values());r['status']='complete';App.closeDocument(doc.Name)
 except Exception as error:
  r.update(status='failed',error=str(error),traceback=traceback.format_exc())
  if 'doc' in locals():App.closeDocument(doc.Name)
 r['source_unchanged']=sha(source)==before;r['passed']=r['passed'] and r['source_unchanged'];output.parent.mkdir(parents=True,exist_ok=True);output.write_text(json.dumps(r,indent=2,allow_nan=False));print(json.dumps({'passed':r['passed'],'status':r['status'],'output':str(output),'error':r.get('error')}));return r
if __name__=='__main__':
 p=argparse.ArgumentParser();p.add_argument('native');p.add_argument('manifest');p.add_argument('output');a=p.parse_args();sys.exit(0 if evaluate(a.native,a.manifest,a.output)['passed'] else 1)
