import FreeCAD as A,Part,json,hashlib
from pathlib import Path
root=Path(__file__).resolve().parents[3];out=root/'out/vtol-demonstrator';out.mkdir(exist_ok=True)
r={}
for name in ['mn2806evo','p12x4']:
 p=root/f'cad/components/vendor/tmotor/{name}.step';s=Part.read(str(p));b=s.BoundBox
 cylinders=[];planes=[]
 for i,f in enumerate(s.Faces):
  t=f.Surface
  if isinstance(t,Part.Cylinder):cylinders.append(dict(face=i+1,radius=t.Radius,center=list(t.Center),axis=list(t.Axis),bounds=[getattr(f.BoundBox,k) for k in ['XMin','YMin','ZMin','XMax','YMax','ZMax']]))
  if isinstance(t,Part.Plane) and f.Area>15:planes.append(dict(face=i+1,area=f.Area,center=list(f.CenterOfMass),normal=list(f.normalAt(0,0))))
 r[name]=dict(file=str(p),sha256=hashlib.sha256(p.read_bytes()).hexdigest(),valid=s.isValid(),solids=len(s.Solids),bounds=[getattr(b,k) for k in ['XMin','YMin','ZMin','XMax','YMax','ZMax']],cylinders=cylinders,planes=planes)
 print(name,len(s.Solids),len(s.Faces),flush=True)
(out/'hardware-geometry-inspection.json').write_text(json.dumps(r,indent=2))
