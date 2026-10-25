"""Import the pinned official SO-101 STEP assembly using native FreeCAD.

STEP retains boundary geometry and assembly placements, not the author's sketch
history or mechanical joint constraints. This script does not invent either.
"""
import hashlib,json,time
from pathlib import Path
import FreeCAD as App
import Import
ROOT=Path(__file__).resolve().parents[3]
PROJECT=Path(__file__).resolve().parent
OUT=ROOT/'out/so-arm101-native';OUT.mkdir(parents=True,exist_ok=True)
manifest=json.loads((PROJECT/'source-manifest.json').read_text())
source=PROJECT/'upstream/STEP/SO101/SO101 Assembly.step'
expected=next(f['sha256'] for f in manifest['files'] if f['upstream_path']=='STEP/SO101/SO101 Assembly.step')
assert hashlib.sha256(source.read_bytes()).hexdigest()==expected
start=time.time();doc=App.newDocument('SO_ARM101_Official')
print('Importing pinned official STEP assembly',flush=True)
Import.insert(str(source),doc.Name);doc.recompute()
rows=[]
for obj in doc.Objects:
 row=dict(name=obj.Name,label=obj.Label,type=obj.TypeId,dependencies=[o.Name for o in obj.OutList])
 if hasattr(obj,'Placement'):
  p=obj.Placement;row['placement']=dict(position=list(p.Base),quaternion=list(p.Rotation.Q))
 if hasattr(obj,'Shape'):
  shape=obj.Shape
  row.update(null=shape.isNull(),solids=len(shape.Solids),faces=len(shape.Faces),volume_mm3=shape.Volume)
  if not shape.isNull():
   row['valid']=shape.isValid();bb=shape.optimalBoundingBox(False,False)
   row['bounds_mm']=[bb.XMin,bb.YMin,bb.ZMin,bb.XMax,bb.YMax,bb.ZMax]
 rows.append(row)
 print(obj.Name,obj.Label,obj.TypeId,row.get('solids','group'),flush=True)
path=OUT/'official-assembly.FCStd';doc.saveAs(str(path))
result=dict(source=str(source),source_sha256=expected,repository=manifest['repository'],commit=manifest['commit'],license=manifest['license'],native=str(path),native_sha256=hashlib.sha256(path.read_bytes()).hexdigest(),elapsed_seconds=time.time()-start,objects=rows,object_count=len(rows),editability='Native imported BRep parts and assembly placements; original feature/sketch history and kinematic constraints are not present in STEP',fit_checked=False,motion_checked=False)
(OUT/'import-report.json').write_text(json.dumps(result,indent=2));print('SAVED',path,flush=True)
