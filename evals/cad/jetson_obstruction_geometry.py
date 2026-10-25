"""Inspect actual source entities obstructing a proposed Jetson mounting gauge."""
import base64,hashlib,json,sys,time
from pathlib import Path
import FreeCAD as App
import Part
ROOT=Path(__file__).resolve().parents[2];sys.path.insert(0,str(ROOT/'cad/freecad'));import worker,native
OUT=ROOT/'out/jetson-obstruction-study';OUT.mkdir(parents=True,exist_ok=True)
report=json.loads((ROOT/'out/vendor-component-inventory/jetson-mount-access.json').read_text());source=ROOT/'cad/components/vendor/nvidia/P3766-P3768SKU4-P3767ENVELOPE.stp';assert hashlib.sha256(source.read_bytes()).hexdigest()==report['source_sha256']
selected=[g for g in report['gauges'] if g['xy_mm']==[92.,0.]];indices=sorted({c['source_solid_index'] for g in selected for c in g['physical_collisions']});assert indices==[1267]
shape=Part.read(str(source));solids=shape.Solids;worker.document('JetsonObstructionStudy');doc=worker.active();parts=[];data=[]
for index in [275]+indices:
 s=solids[index-1];assert s.isValid();o=doc.addObject('Part::Feature','SourceSolid'+str(index));o.Shape=s.copy();parts.append(o.Name)
 b=native.shape_bounds(s);data.append(dict(source_solid_index=index,bounds_mm=[b.XMin,b.YMin,b.ZMin,b.XMax,b.YMax,b.ZMax],volume_mm3=s.Volume,face_count=len(s.Faces),source_geometry_sha256=hashlib.sha256(s.cleaned().exportBrepToString().encode()).hexdigest()))
doc.recompute();doc.saveAs(str(OUT/'scoped-source-extract.FCStd'))
# A cropped display-only board section preserves the actual hole and obstacle.
box=Part.makeBox(18,18,30,App.Vector(82,-9,-12));clip=doc.addObject('Part::Feature','BoardDisplayCrop');clip.Shape=doc.SourceSolid275.Shape.common(box);doc.recompute()
r=worker.render(['BoardDisplayCrop','SourceSolid1267'],width=800,height=600,azimuth_degrees=-45,elevation_degrees=-25,deflection_mm=.05)
(OUT/'underside-obstruction.png').write_bytes(base64.b64decode(r.pop('content_blocks')[0]['source']['data']))
assert hashlib.sha256(source.read_bytes()).hexdigest()==report['source_sha256']
(OUT/'result.json').write_text(json.dumps(dict(source=str(source),source_sha256=report['source_sha256'],source_unchanged=True,selected_hole_xy_mm=[92,0],source_entities=data,full_assembly_imported=False,scope='Scoped source-solid diagnostic extract only. Display-only board crop. Part1267 obstructs provisional support/shaft gauges; source component identity and mounting intent unconfirmed. Not a new payload mount or complete source import.'),indent=2));print(json.dumps(data,indent=2))
