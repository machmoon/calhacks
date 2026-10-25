import FreeCAD as A,Part,json,hashlib
from pathlib import Path
p=Path(__file__).resolve().parent;f=p/'sources/servo-cad/HS-65MG_221121.stp';s=Part.Shape();s.read(str(f));b=s.optimalBoundingBox(False,False)
c=[]
for i,face in enumerate(s.Faces):
 surf=face.Surface
 if isinstance(surf,Part.Cylinder):
  bb=face.optimalBoundingBox(False,False)
  c.append({'face':i+1,'radius':surf.Radius,'center':list(surf.Center),'axis':list(surf.Axis),'bounds':[bb.XMin,bb.YMin,bb.ZMin,bb.XMax,bb.YMax,bb.ZMax]})
r={'source':str(f),'sha256':hashlib.sha256(f.read_bytes()).hexdigest(),'valid':s.isValid(),'solids':len(s.Solids),'shells':len(s.Shells),'faces':len(s.Faces),'bounds':[b.XMin,b.YMin,b.ZMin,b.XMax,b.YMax,b.ZMax],'size':[b.XLength,b.YLength,b.ZLength],'cylinders':c,'scope':'Unmodified manufacturer STEP; geometry measurement not load qualification'}
(p/'servo-geometry-inspection.json').write_text(json.dumps(r,indent=2)+'\n');print(json.dumps({k:v for k,v in r.items() if k!='cylinders'}));print('cylinders',len(c));print(json.dumps([x for x in c if abs(x['radius']-1)<.01],indent=2))
