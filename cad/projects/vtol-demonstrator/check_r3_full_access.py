"""Source-preserving straight-driver access audit of saved r3 candidate."""
import FreeCAD as A,Part,json,hashlib,sys
from pathlib import Path
out=Path(__file__).resolve().parents[3]/'out/vtol-demonstrator-r3';sys.path.insert(0,str(out));import vtol_features_r3
source=out/'assembly.FCStd';sha=hashlib.sha256(source.read_bytes()).hexdigest();d=A.openDocument(str(source));p=d.DesignParameters
# Model outputs, not construction-tool shapes. Source package remains pending.
names=[o.Name for o in d.Objects if 'Fidelity' in o.PropertiesList and ('Original provisional' in o.Fidelity or 'Simplified' in o.Fidelity or 'Exact' in o.Fidelity)]
names+=['BoomRight','BoomLeft','Stabilizer','Elevator','FinRight','FinLeft','RudderRight','RudderLeft']
rows=[]
for side,sign in [('Right',1),('Left',-1)]:
 for end in ['Front','Rear']:
  for offset in [-25,25]:
   x=getattr(p,'Motor'+end+'X').Value;y=sign*p.BoomHalfSpacing.Value+offset;tool=Part.makeCylinder(3,200,A.Vector(x,y,56));checks=[]
   for name in sorted(set(names)):
    s=d.getObject(name).Shape
    if not tool.BoundBox.intersect(s.BoundBox):continue
    volume=tool.common(s).Volume;checks.append(dict(object=name,common_mm3=volume))
   rows.append(dict(side=side,end=end,offset_mm=offset,candidates=checks,pass_=all(r['common_mm3']<1e-6 for r in checks)));print(rows[-1],flush=True)
r=dict(revision='layout-r3-tool-access',source_sha256=sha,source_unchanged=sha==hashlib.sha256(source.read_bytes()).hexdigest(),checks=rows,passed=all(row['pass_'] for row in rows),scope='6mm diameter straight driver shaft200mm long in saved stationary assembly orientation. Not a rotating-prop clearance or a complete purchased-tool envelope. Propellers must be stopped and secured or removed for service.')
(out/'full-driver-access.json').write_text(json.dumps(r,indent=2))
