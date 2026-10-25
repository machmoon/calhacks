"""Local B-rep cutaway of saved V2, without changing source/native features."""
import base64,json
from pathlib import Path
import FreeCAD as App
import Part
import render,worker
ROOT=Path(__file__).resolve().parents[2];out=ROOT/'out/tsung-vtol-tail-joint-v2'
report=json.loads((out/'result.json').read_text());doc=App.openDocument(str(out/'assembly.FCStd'))
preview=App.newDocument('TailClampPreview')
clip=Part.makeBox(40,50,33,App.Vector(655,195,-25));selected=[]
for name in report['physical']:
 shape=doc.getObject(name).Shape.common(clip)
 if shape.isNull() or not shape.Solids:continue
 label='UpperFinRoot' if name==report['physical'][1] else 'LowerSaddle' if name==report['physical'][2] else name
 obj=preview.addObject('Part::Feature',label);obj.Shape=shape;selected.append(obj)
preview.recompute()
r=render.selected(selected,worker.geometry_fingerprint,width=640,height=480,azimuth_degrees=-52,elevation_degrees=15)
(out/'root-cutaway.png').write_bytes(base64.b64decode(r.pop('content_blocks')[0]['source']['data']))
r['source_native']=str(out/'assembly.FCStd');r['display_cut_box_mm']=[[655,195,-25],[695,245,8]];r['scope']='Derived cutaway of actual saved B-reps; tall fin, top stabilizer and distant boom omitted for local inspection'
(out/'root-cutaway.json').write_text(json.dumps(r,indent=2))
App.closeDocument(preview.Name);App.closeDocument(doc.Name)
