"""Actual saved hardpoint/wing local previews; source BReps remain unchanged."""
import base64,hashlib,json
from pathlib import Path
import FreeCAD as App
import Part
import render,worker
ROOT=Path(__file__).resolve().parents[2];out=ROOT/'out/tsung-vtol-wing-boom-assembly';path=out/'assembly.FCStd';digest=hashlib.sha256(path.read_bytes()).hexdigest();r=json.loads((out/('result.json' if (out/'result.json').exists() else 'checks-before-export.json')).read_text());doc=App.openDocument(str(path))
for tag,y0,omit_skin,az,el in [('hardpoint-cutaway',220,False,-70,15),('hardpoint-internals',190,True,-50,18)]:
 preview=App.newDocument('WingBoomPreview');clip=Part.makeBox(50,250-y0,70,App.Vector(375,y0,-25));selected=[]
 for name in r['physical']:
  if omit_skin and name in r['skin_outputs'].values():continue
  shape=doc.getObject(name).Shape.common(clip)
  if shape.isNull() or not shape.Solids:continue
  o=preview.addObject('Part::Feature',name);o.Shape=shape;selected.append(o)
 preview.recompute();image=render.selected(selected,worker.geometry_fingerprint,width=640,height=480,azimuth_degrees=az,elevation_degrees=el,deflection_mm=1,max_triangles=100000)
 (out/(tag+'.png')).write_bytes(base64.b64decode(image.pop('content_blocks')[0]['source']['data']));image.update(source_native=str(path),clip_mm=[[375,y0,-25],[425,250,45]],skin_omitted=omit_skin,scope='Local clipping of saved actual BReps; no assembly modification')
 (out/(tag+'.json')).write_text(json.dumps(image,indent=2));App.closeDocument(preview.Name)
assert hashlib.sha256(path.read_bytes()).hexdigest()==digest
