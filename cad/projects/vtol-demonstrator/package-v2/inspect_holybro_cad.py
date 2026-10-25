import FreeCAD as A,Part,json,hashlib
from pathlib import Path
p=Path(__file__).resolve().parent
out=[]
for f in sorted((p/'sources/holybro-cad').glob('*.stp')):
 s=Part.Shape();s.read(str(f));b=s.optimalBoundingBox(False,False);c=[]
 for i,face in enumerate(s.Faces):
  u=face.Surface
  if isinstance(u,Part.Cylinder):
   bb=face.optimalBoundingBox(False,False);c.append({'face':i+1,'radius_mm':u.Radius,'axis':list(u.Axis),'center_mm':list(u.Center),'bounds_mm':[bb.XMin,bb.YMin,bb.ZMin,bb.XMax,bb.YMax,bb.ZMax]})
 r={'id':f.stem,'path':str(f.relative_to(p)),'sha256':hashlib.sha256(f.read_bytes()).hexdigest(),'valid':s.isValid(),'solids':len(s.Solids),'shells':len(s.Shells),'faces':len(s.Faces),'bounds_mm':[b.XMin,b.YMin,b.ZMin,b.XMax,b.YMax,b.ZMax],'size_mm':[b.XLength,b.YLength,b.ZLength],'cylinders':c,'solid_validity':[x.isValid() for x in s.Solids]};out.append(r);print(json.dumps({k:v for k,v in r.items() if k not in ['cylinders','solid_validity']}),flush=True)
(p/'holybro-geometry-inspection.json').write_text(json.dumps({'method':'FreeCAD Part.Shape.read;optimalBoundingBox(False,False);unmodified source','parts':out},indent=2)+'\n')
