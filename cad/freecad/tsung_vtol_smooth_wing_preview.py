"""Independent preview of the actual saved non-ruled wing candidate."""
import hashlib,json
from pathlib import Path
import FreeCAD as App
import surface_preview
ROOT=Path(__file__).resolve().parents[2];out=ROOT/'out/tsung-vtol-smooth-aircraft';source=out/'wing-candidate.FCStd'
digest=hashlib.sha256(source.read_bytes()).hexdigest();doc=App.openDocument(str(source))
objects=[doc.RightSkin,doc.LeftSkin]
for name,az,el in [('wing-proposed',-15,28),('wing-planform',0,90)]:
 r=surface_preview.selected(objects,out/(name+'.png'),width=640,height=480,azimuth=az,elevation=el)
 r.update(source_native=str(source),native_outer_faces=len(doc.RightOuterContour.Shape.Faces),ruled=bool(doc.RightOuterContour.Ruled),geometry_audit_status='pending independent wall, section fidelity, aircraft interference and servo recheck',selected_view='Skin exterior view; ribs and spar remain in native model')
 (out/(name+'.json')).write_text(json.dumps(r,indent=2));print('Saved',name,flush=True)
assert hashlib.sha256(source.read_bytes()).hexdigest()==digest
