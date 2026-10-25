"""Saved hardpoint bearing checks and an explicit unresolved spar-slide probe."""
import hashlib,json
from pathlib import Path
import FreeCAD as App
ROOT=Path(__file__).resolve().parents[2];out=ROOT/'out/tsung-vtol-wing-boom-assembly';path=out/'assembly.FCStd';digest=hashlib.sha256(path.read_bytes()).hexdigest();r=json.loads((out/('result.json' if (out/'result.json').exists() else 'checks-before-export.json')).read_text());doc=App.openDocument(str(path));rows=[]
for side,suffix in [('Right',''),('Left','Left')]:
 upper=doc.getObject('WBUpperFinal'+suffix).Shape;lower=doc.getObject('WBLowerDrill1'+suffix).Shape
 for i,y in enumerate([206,234]):
  screw=doc.getObject(f'WBScrew{i}'+suffix).Shape;nut=doc.getObject(f'WBNut{i}'+suffix).Shape
  moved=screw.copy();moved.translate(App.Vector(0,0,.1));head_stop=moved.common(lower).Volume;assert head_stop>1e-4
  moved=nut.copy();moved.translate(App.Vector(0,0,-.1));nut_stop=moved.common(upper).Volume;assert nut_stop>1e-4
  moved=nut.copy();moved.rotate(App.Vector(400,y if side=='Right' else -y,-6.4),App.Vector(0,0,1),6);rotation_stop=moved.common(upper).Volume;assert rotation_stop>1e-4
  rows.append(dict(side=side,index=i,head_up_0_1_stop_mm3=head_stop,nut_down_0_1_stop_mm3=nut_stop,nut_rotation_6deg_stop_mm3=rotation_stop))
slides=[]
for side,sign in [('Right',1),('Left',-1)]:
 spar=doc.getObject(side+'Spar').Shape.copy();spar.translate(App.Vector(0,sign,0));pairs=[]
 for name in r['physical']:
  if name==side+'Spar':continue
  target=doc.getObject(name).Shape;a=spar.BoundBox;b=target.BoundBox
  separated=any(getattr(a,t+'Max')<getattr(b,t+'Min') or getattr(b,t+'Max')<getattr(a,t+'Min') for t in ['X','Y','Z'])
  volume=0 if separated else spar.common(target).Volume
  if volume>1e-6:pairs.append(dict(part=name,overlap_mm3=volume))
 slides.append(dict(side=side,outboard_translation_mm=1,interferences=pairs,interpretation='Any absence of interference demonstrates an unresolved axial slide in this geometric model; radial contact is not axial retention'))
assert hashlib.sha256(path.read_bytes()).hexdigest()==digest
(out/'retention-checks.json').write_text(json.dumps(dict(fastener_bearing_stops=rows,spar_axial_slide_probes=slides,native_preserved=True),indent=2));print('Retention checks complete; inspect explicit axial-slide result',flush=True)
