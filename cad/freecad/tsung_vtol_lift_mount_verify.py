"""Independent saved-artifact access, closure and unchanged-source verification."""
import hashlib,json
from pathlib import Path
import FreeCAD as App
import native
ROOT=Path(__file__).resolve().parents[2];out=ROOT/'out/tsung-vtol-lift-mount';path=out/'assembly.FCStd';digest=hashlib.sha256(path.read_bytes()).hexdigest()
r=json.loads((out/('result.json' if (out/'result.json').exists() else 'checks-before-export.json')).read_text());doc=App.openDocument(str(path));source=App.openDocument(str(ROOT/'out/tsung-vtol-top-retention/assembly.FCStd'))
unchanged=[]
for name in json.loads((ROOT/'out/tsung-vtol-top-retention/result.json').read_text())['physical']:
 if name.startswith('LiftMotor'):continue
 a=source.getObject(name).Shape;b=doc.getObject(name).Shape
 if a.exportBrepToString()==b.exportBrepToString():method='identical_brep';diff=[0,0]
 else:method='bidirectional_difference';diff=[a.cut(b).Volume,b.cut(a).Volume]
 assert max(diff)<1e-5,(name,diff);unchanged.append(dict(object=name,method=method,difference_mm3=diff))
access=[];closure=[]
for code,parts in r['instances'].items():
 matrix=App.Matrix()
 if code.startswith('R'):matrix.A11=-1;matrix.A14=690
 if code.endswith('L'):matrix.A22=-1
 for name in [f'LiftMotorKey{i}' for i in range(4)]+[f'LiftClampKey{i}' for i in range(2)]:
  shape=doc.getObject(name).Shape.transformGeometry(matrix)
  for other in r['physical']:
   target=doc.getObject(other).Shape;a=shape.BoundBox;b=target.BoundBox
   separated=any(getattr(a,axis+'Max')<getattr(b,axis+'Min') or getattr(b,axis+'Max')<getattr(a,axis+'Min') for axis in ['X','Y','Z'])
   v=0.0 if separated else shape.common(target).Volume;assert v<1e-6,(code,name,other,v)
   access.append(dict(instance=code,tool=name,part=other,overlap_mm3=v))
 lower=doc.getObject(parts['Lower']).Shape;upper=doc.getObject(parts['Upper']).Shape;boom=doc.getObject('Boom'+('Right' if code.endswith('R') else 'Left')).Shape
 split=upper.distToShape(lower)[0];assert abs(split-.5)<1e-6
 moved=lower.copy();moved.translate(App.Vector(0,0,-.4));opened=moved.distToShape(boom)[0];assert opened>0
 moved=lower.copy();moved.translate(App.Vector(0,0,.05));overtravel=moved.common(boom).Volume;assert overtravel>1e-4
 closure.append(dict(instance=code,split_mm=split,opened_0_4_boom_clearance_mm=opened,overtravel_0_05_interference_mm3=overtravel))
assert hashlib.sha256(path.read_bytes()).hexdigest()==digest
(out/'supplemental-verification.json').write_text(json.dumps(dict(source_unchanged=unchanged,all_four_tool_access=access,clamp_closure=closure,saved_native_preserved=True),indent=2))
print('Supplemental checks passed',flush=True)
