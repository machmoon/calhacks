"""Display-only hidden-skin views of the verified native assembly."""
import hashlib,json
from pathlib import Path
import FreeCAD as App
import surface_preview
ROOT=Path(__file__).resolve().parents[2];OUT=ROOT/'out/tsung-vtol-smooth-aircraft-final'
report=json.loads((OUT/'result.json').read_text());assert report['status']=='complete'
source=OUT/'assembly.FCStd';digest=hashlib.sha256(source.read_bytes()).hexdigest();doc=App.openDocument(str(source))
physical=report['physical'];skin_names=['RightSkin','LeftSkin'];servo='MG90SStaticEnvelope'
views=[('aircraft-internals',[n for n in physical if n not in skin_names]+[servo],640,480,-52,24),
       ('wing-internals',[n for n in physical if n.startswith(('RightRib','LeftRib','RightSpar','LeftSpar'))]+[servo],800,600,-15,28)]
results=[]
for name,names,width,height,az,el in views:
 objects=[doc.getObject(n) for n in names]
 before={o.Name:hashlib.sha256(o.Shape.cleaned().exportBrepToString().encode()).hexdigest() for o in objects}
 r=surface_preview.selected(objects,OUT/(name+'.png'),width=width,height=height,azimuth=az,elevation=el)
 assert before=={o.Name:hashlib.sha256(o.Shape.cleaned().exportBrepToString().encode()).hexdigest() for o in objects}
 r.update(scope='Display-only skins-hidden view of verified native geometry; no material removed from the model',
    source_native=str(source),source_sha256=digest,hidden_physical_parts=skin_names,
    visible_nonphysical_envelopes=[servo],servo_scope='Blue box is the checked conservative static servo envelope; not detailed servo CAD, horn, bracket, hatch or cable routing')
 (OUT/(name+'.json')).write_text(json.dumps(r,indent=2));results.append(r);print('Saved',name,flush=True)
assert hashlib.sha256(source.read_bytes()).hexdigest()==digest
(OUT/'internals-preview.json').write_text(json.dumps(dict(source_preserved=True,views=results),indent=2))
