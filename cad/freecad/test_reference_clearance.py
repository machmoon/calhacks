"""A positive shell distance does not establish clearance from an enclosure."""
import json,hashlib
from pathlib import Path
import FreeCAD as App
import Part
import reference_clearance as rc
shell=Part.makeBox(10,10,10).Shells[0];before=hashlib.sha256(shell.exportBrepToString().encode()).hexdigest()
inside=Part.makeBox(2,2,2,App.Vector(4,4,4));outside=Part.makeBox(2,2,2,App.Vector(20,20,20));cross=Part.makeBox(2,2,2,App.Vector(9,4,4))
assert shell.distToShape(inside)[0]>0
r=rc.obstruction(shell,inside);assert r['obstructed'] and r['reason']=='inside_closed_reference_enclosure' and abs(r['enclosed_gauge_volume_mm3']-8)<1e-8
assert not rc.obstruction(shell,outside)['obstructed']
assert rc.obstruction(shell,cross)['reason']=='reference_surface_contact'
face=Part.makePlane(10,10);assert not rc.obstruction(face,inside)['obstructed']
assert rc.obstruction(face,Part.makeBox(2,2,2,App.Vector(2,2,-1)))['obstructed']
for invalid in [True,-1,float('nan'),2]:
 try:rc.obstruction(shell,inside,invalid)
 except ValueError:pass
 else:raise AssertionError('invalid tolerance accepted')
assert before==hashlib.sha256(shell.exportBrepToString().encode()).hexdigest()
out=Path(__file__).resolve().parents[2]/'out/freecad-reference-clearance';out.mkdir(parents=True,exist_ok=True)
(out/'result.json').write_text(json.dumps(dict(passed=True,enclosed_gauge=r,outside_clear=True,crossing_rejected=True,open_face_semantics_verified=True,source_reference_unchanged=True),indent=2))
print('Reference clearance detects enclosed gauges despite positive surface distance')
