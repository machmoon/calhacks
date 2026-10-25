"""Read-only kernel diagnostics for invalid components in official STEP import."""
import hashlib,json
from pathlib import Path
import FreeCAD as App
ROOT=Path(__file__).resolve().parents[3];OUT=ROOT/'out/so-arm101-native';path=OUT/'official-assembly.FCStd';digest=hashlib.sha256(path.read_bytes()).hexdigest();doc=App.openDocument(str(path));rows=[]
for name in ['Part__Feature214','Part__Feature222','Part__Feature230','Part__Feature238','Part__Feature246','Part__Feature253']:
 o=doc.getObject(name);s=o.Shape;r=dict(name=name,label=o.Label,shape_type=s.ShapeType,solids=len(s.Solids),shells=len(s.Shells),faces=len(s.Faces),edges=len(s.Edges),valid=s.isValid(),closed=s.isClosed(),volume_mm3=s.Volume)
 try:r['kernel_check']=str(s.check(True))
 except Exception as e:r['kernel_check']=type(e).__name__+': '+str(e)
 r['invalid_faces']=[i+1 for i,f in enumerate(s.Faces) if not f.isValid()]
 rows.append(r);print(json.dumps(r),flush=True)
assert hashlib.sha256(path.read_bytes()).hexdigest()==digest
(OUT/'source-defects.json').write_text(json.dumps(dict(native_sha256=digest,source_preserved=True,repairs_applied=False,components=rows),indent=2))
