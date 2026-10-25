"""Read-only compatibility probe of the previously saved hardpoint and new wing."""
import hashlib,json
from pathlib import Path
import FreeCAD as App
ROOT=Path(__file__).resolve().parents[2];out=ROOT/'out/tsung-vtol-wing-boom-assembly';out.mkdir(exist_ok=True,parents=True)
paths=[ROOT/'out/tsung-vtol-smooth-aircraft-final/assembly.FCStd',ROOT/'out/tsung-vtol-wing-hardpoint-linked/assembly.FCStd'];hashes={str(p):hashlib.sha256(p.read_bytes()).hexdigest() for p in paths}
a=App.openDocument(str(paths[0]));b=App.openDocument(str(paths[1]));upper=b.HPNutSeat1.Shape;skin=a.RightSkin.Shape
rows=[]
for shift in [0,10]:
 shape=upper.copy();shape.translate(App.Vector(0,shift,0));rib=a.RightRib2.Shape
 rows.append(dict(boom_half_spacing_mm=220+shift,rib2_overlap_mm3=shape.common(rib).Volume,rib2_clearance_mm=shape.distToShape(rib)[0]))
cut=skin.cut(upper)
r=dict(source_sha256=hashes,prior_hardpoint_shape_valid=upper.isValid(),current_skin_shape_valid=skin.isValid(),nominal_skin_passage_valid=cut.isValid(),nominal_skin_passage_solids=len(cut.Solids),nominal_skin_hardpoint_intersection_mm3=skin.common(upper).Volume,prior20mm_span_rib_probe=rows,scope='Prior saved20mm hardpoint checked against actual final smooth skin/rib; no source edits')
assert all(hashlib.sha256(Path(p).read_bytes()).hexdigest()==h for p,h in hashes.items());(out/'prior-integration-probe.json').write_text(json.dumps(r,indent=2));print(json.dumps(r),flush=True)
