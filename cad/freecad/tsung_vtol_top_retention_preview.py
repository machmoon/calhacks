"""Local cutaway of the saved top-retention B-reps; source remains untouched."""
import base64,hashlib,json
from pathlib import Path
import FreeCAD as App
import Part
import render,worker
ROOT=Path(__file__).resolve().parents[2];out=ROOT/'out/tsung-vtol-top-retention'
source=out/'assembly.FCStd';digest=hashlib.sha256(source.read_bytes()).hexdigest()
if (out/'result.json').exists():report=json.loads((out/'result.json').read_text())
else:
 previous=json.loads((ROOT/'out/tsung-vtol-tail-assembly/result.json').read_text())
 replacements={previous['fin_physical_names']['Right']:'FinTopRetentionRight',previous['fin_physical_names']['Left']:'FinTopRetentionLeft','TopStabilizer':'TopStabilizerRetained'}
 report={'physical':[replacements.get(n,n) for n in previous['physical']]+[f'Top{kind}{side}{i}' for kind in ['Screw','Nut'] for i in [0,1] for side in ['Right','Left']]}
doc=App.openDocument(str(source))
for tag,y0,az,el in [('top-joint',160,-52,24),('nut-pocket-cutaway',175,-90,8)]:
 preview=App.newDocument('TopRetentionPreview')
 clip=Part.makeBox(70,195-y0,50,App.Vector(688,y0,100));selected=[]
 for name in report['physical']:
  shape=doc.getObject(name).Shape.common(clip)
  if shape.isNull() or not shape.Solids:continue
  obj=preview.addObject('Part::Feature',name);obj.Shape=shape;selected.append(obj)
 preview.recompute()
 r=render.selected(selected,worker.geometry_fingerprint,width=640,height=480,azimuth_degrees=az,elevation_degrees=el)
 (out/(tag+'.png')).write_bytes(base64.b64decode(r.pop('content_blocks')[0]['source']['data']))
 r.update(source_native=str(source),display_cut_box_mm=[[688,y0,100],[758,195,150]],scope='Derived local cutaway of actual saved B-reps; not an assembly modification')
 (out/(tag+'.json')).write_text(json.dumps(r,indent=2));App.closeDocument(preview.Name)
assert hashlib.sha256(source.read_bytes()).hexdigest()==digest
App.closeDocument(doc.Name)
