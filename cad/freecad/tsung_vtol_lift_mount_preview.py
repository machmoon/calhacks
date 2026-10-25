"""Actual saved mount BRep previews; no source modification."""
import base64,hashlib,json
from pathlib import Path
import FreeCAD as App
import Part
import render,worker
ROOT=Path(__file__).resolve().parents[2];out=ROOT/'out/tsung-vtol-lift-mount';source=out/'assembly.FCStd';digest=hashlib.sha256(source.read_bytes()).hexdigest()
r=json.loads((out/('result.json' if (out/'result.json').exists() else 'checks-before-export.json')).read_text());doc=App.openDocument(str(source))
for tag,y0,az,el in [('lift-mount',193,-52,24),('lift-mount-cutaway',220,-90,8),('lift-mount-underside',193,-52,-45)]:
 preview=App.newDocument('LiftMountPreview');clip=Part.makeBox(50,247-y0,75,App.Vector(80,y0,-25));selected=[]
 for n in r['physical']:
  s=doc.getObject(n).Shape.common(clip)
  if s.isNull() or not s.Solids:continue
  o=preview.addObject('Part::Feature',n);o.Shape=s;selected.append(o)
 preview.recompute();image=render.selected(selected,worker.geometry_fingerprint,width=640,height=480,azimuth_degrees=az,elevation_degrees=el)
 (out/(tag+'.png')).write_bytes(base64.b64decode(image.pop('content_blocks')[0]['source']['data']));image.update(scope='Actual BRep local clipping; motor envelope and threaded depth remain provisional',source_native=str(source),clip_mm=[[80,y0,-25],[130,247,50]])
 (out/(tag+'.json')).write_text(json.dumps(image,indent=2));App.closeDocument(preview.Name)
assert hashlib.sha256(source.read_bytes()).hexdigest()==digest
