"""Render actual imported leaf BReps in the original assembly frame."""
import base64,hashlib,json
from pathlib import Path
from types import SimpleNamespace
import FreeCAD as App
import MeshPart
import render
ROOT=Path(__file__).resolve().parents[3];OUT=ROOT/'out/so-arm101-native';path=OUT/'official-assembly.FCStd';source_hash=hashlib.sha256(path.read_bytes()).hexdigest();doc=App.openDocument(str(path));objects=[]
for obj in doc.Objects:
 if obj.TypeId!='Part::Feature' or obj.Shape.isNull() or not obj.Shape.Faces:continue
 shape=obj.Shape.copy();shape.Placement=obj.getGlobalPlacement().multiply(obj.Placement.inverse()).multiply(shape.Placement)
 objects.append(SimpleNamespace(Name=obj.Name,Label=obj.Label,Shape=shape))
def fingerprint(items):
 h=hashlib.sha256()
 for o in sorted(items,key=lambda x:x.Name):h.update(o.Name.encode());h.update(o.Shape.cleaned().exportBrepToString().encode())
 return h.hexdigest()
triangles=[];colors=[];parts=[]
for obj in objects:
 mesh=MeshPart.meshFromShape(Shape=obj.Shape,LinearDeflection=1,AngularDeflection=1,Relative=False)
 vertices,indices=mesh.Topology
 for tri in indices:
  triangles.append([[vertices[j].x,vertices[j].y,vertices[j].z] for j in tri]);colors.append(render.part_color(obj.Name))
 parts.append(dict(name=obj.Name,label=obj.Label,triangles=len(indices)))
 if len(triangles)>500000:raise ValueError('Project preview exceeds explicit500000-triangle budget')
rgb,stats=render.rasterize(triangles,colors,800,600,35,24)
(OUT/'official-assembly.png').write_bytes(render.png_rgb(rgb))
result=dict(stats=stats,parts=parts,triangle_count=len(triangles),geometry_fingerprint=fingerprint(objects),display_mesh=dict(linear_deflection_mm=1,angular_deflection_radians=1,triangle_limit=500000))
result.update(source_native_sha256=source_hash,source_frame='Official STEP frame; global native placements applied to leaf shapes',illustrative_colors=True,reference_surface_objects=[o.Name for o in objects if not o.Shape.Solids],fit_checked=False,motion_checked=False)
assert hashlib.sha256(path.read_bytes()).hexdigest()==source_hash
(OUT/'preview-report.json').write_text(json.dumps(result,indent=2));print('Rendered actual native assembly',flush=True)
