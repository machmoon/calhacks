"""Saved BRep rotor/fin clearance view including the real sweep envelope."""
import base64,hashlib,json
from pathlib import Path
import FreeCAD as App
import Part
import render,worker
ROOT=Path(__file__).resolve().parents[2];out=ROOT/'out/tsung-vtol-rear-rotor-layout';path=out/'assembly.FCStd';digest=hashlib.sha256(path.read_bytes()).hexdigest();r=json.loads((out/('result.json' if (out/'result.json').exists() else 'checks-before-export.json')).read_text());doc=App.openDocument(str(path))
for tag,az,el in [('rear-rotor-fin-clearance',-60,30),('rear-rotor-side-clearance',-90,0)]:
 preview=App.newDocument('RotorClearancePreview');clip=Part.makeBox(255,200,185,App.Vector(465,120,-25));selected=[]
 for n in r['physical']+['SweepRR']:
  shape=doc.getObject(n).Shape.common(clip)
  if shape.isNull() or not shape.Solids:continue
  o=preview.addObject('Part::Feature',n);o.Shape=shape;selected.append(o)
 preview.recompute();image=render.selected(selected,worker.geometry_fingerprint,width=640,height=480,azimuth_degrees=az,elevation_degrees=el,deflection_mm=2,max_triangles=100000)
 (out/(tag+'.png')).write_bytes(base64.b64decode(image.pop('content_blocks')[0]['source']['data']));image.update(scope='Actual saved BReps plus native rear rotor swept envelope; local display clipping only',clip_mm=[[465,120,-25],[720,320,160]],source_native=str(path))
 (out/(tag+'.json')).write_text(json.dumps(image,indent=2));App.closeDocument(preview.Name)
assert hashlib.sha256(path.read_bytes()).hexdigest()==digest
