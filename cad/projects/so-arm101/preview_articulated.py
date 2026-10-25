"""Two actual FreeCAD mesh poses and native edit/reopen verification."""
import hashlib,json,math
from pathlib import Path
import FreeCAD as App
import render
ROOT=Path(__file__).resolve().parents[3];OUT=ROOT/'out/so-arm101-candidate';SOURCE=OUT/'assembly.FCStd';before=hashlib.sha256(SOURCE.read_bytes()).hexdigest();doc=App.openDocument(str(SOURCE));manifest=json.loads((OUT/'manifest.json').read_text())

def frames(document):
 return {name:list(document.getObject(obj).getGlobalPlacement().toMatrix().A) for name,obj in manifest['links'].items()}

def pose(document,angles):
 for name,value in angles.items():
  bound=manifest['controls'][name];assert bound['lower_degrees']<=value<=bound['upper_degrees'];setattr(document.JointParameters,name,value)
 document.recompute()

def preview(document,path):
 triangles=[];colors=[];rows=[]
 for record in manifest['visual_instances']:
  obj=document.getObject(record['object_name']);mesh=obj.Mesh.copy();mesh.Placement=obj.getGlobalPlacement().multiply(obj.Placement.inverse()).multiply(mesh.Placement);vertices,facets=mesh.Topology
  filename=record['source_asset'];color=(68,78,88) if 'sts3215' in filename else (64,139,157)
  if 'moving_jaw' in filename:color=(215,153,59)
  if 'base_so101' in filename:color=(53,102,127)
  for f in facets:triangles.append([[vertices[j].x,vertices[j].y,vertices[j].z] for j in f]);colors.append(color)
  rows.append(dict(name=obj.Name,facets=len(facets)))
 assert len(triangles)<=1000000
 rgb,stats=render.rasterize(triangles,colors,800,600,38,24);path.write_bytes(render.png_rgb(rgb));return dict(image=str(path),facets=len(triangles),stats=stats,visual_instances=rows,geometry_kind='official mesh visuals in actual native FreeCAD global placements',illustrative_colors=True)

zero=frames(doc);zero_preview=preview(doc,OUT/'zero-pose.png')
angles=dict(shoulder_pan=25,shoulder_lift=-20,elbow_flex=35,wrist_flex=-25,wrist_roll=30,gripper=35)
pose(doc,angles);edited=frames(doc);assert any(abs(a-b)>1 for a,b in zip(zero['gripper_frame_link'],edited['gripper_frame_link']))
posed_path=OUT/'demonstration-pose.FCStd';doc.saveAs(str(posed_path));edited_preview=preview(doc,OUT/'demonstration-pose.png');name=doc.Name;App.closeDocument(name);doc=App.openDocument(str(posed_path));doc.recompute()
assert all(abs(getattr(doc.JointParameters,n).Value-v)<1e-8 for n,v in angles.items())
reopened=frames(doc);error=max(abs(a-b) for n in edited for a,b in zip(edited[n],reopened[n]));assert error<1e-8
pose(doc,{n:0 for n in angles});restored=frames(doc);zero_error=max(abs(a-b) for n in zero for a,b in zip(zero[n],restored[n]));assert zero_error<1e-8
assert hashlib.sha256(SOURCE.read_bytes()).hexdigest()==before
report=dict(source_native_sha256=before,master_unchanged=True,zero_pose_preview=zero_preview,demonstration_angles_degrees=angles,demonstration_native=str(posed_path),demonstration_preview=edited_preview,reopen_max_matrix_error=error,revert_max_matrix_error=zero_error,native_expressions_persisted=True,custom_proxy_required=False,zero_frames=zero,posed_frames=edited,scope='Native articulation/edit persistence and visual demonstration; independent URDF correctness checked separately, no collision or strength claim')
(OUT/'reopen-preview-report.json').write_text(json.dumps(report,indent=2));print('Two native poses rendered; edit/reopen/revert passed',flush=True)
