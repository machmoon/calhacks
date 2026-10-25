"""Validity caching never accepts changed or invalid BREP on prior identity."""
import json
from pathlib import Path
import FreeCAD as App
import Part
import native
OUT=Path(__file__).resolve().parents[2]/'out/freecad-validity-cache'
OUT.mkdir(parents=True,exist_ok=True)
native._validity_cache.clear()

class Probe:
    def __init__(self,shape):self.shape=shape;self.calls=0
    def isNull(self):return self.shape.isNull()
    def cleaned(self):return self.shape.cleaned()
    def isValid(self):self.calls+=1;return self.shape.isValid()

box=Probe(Part.makeBox(10,20,30))
assert native.shape_is_valid(box) and native.shape_is_valid(box)
assert box.calls==1
copy=Probe(box.shape.copy())
assert native.shape_is_valid(copy) and copy.calls==0
# Same wrapper, changed native geometry: prior identity must not grant a hit.
box.shape.translate(App.Vector(1,0,0))
assert native.shape_is_valid(box) and box.calls==2
crossed=Part.makePolygon([App.Vector(0,0,0),App.Vector(10,10,0),
    App.Vector(0,10,0),App.Vector(10,0,0),App.Vector(0,0,0)])
invalid=Part.Face(crossed)
assert not invalid.isValid()
box.shape=invalid
assert not native.shape_is_valid(box) and box.calls==3
assert not native.shape_is_valid(box) and box.calls==3
assert not native.shape_is_valid(Part.Shape())
# A reopened serialized shape may reuse the exact geometry result, but edited
# geometry cannot. Bound memory without retaining full shapes or BREP strings.
path=OUT/'box.brep';copy.shape.exportBrep(str(path))
reopened=Part.Shape();reopened.read(str(path))
assert native.shape_is_valid(reopened)
# A realistic construction working set must survive a second full traversal.
# Mutation still changes its exact BREP key and forces kernel revalidation.
native._validity_cache.clear()
working_set=[Probe(Part.makeBox(1,2,3,App.Vector(i,10,0))) for i in range(2048)]
for item in working_set:assert native.shape_is_valid(item)
for item in working_set:assert native.shape_is_valid(item) and item.calls==1
working_set[200].shape.translate(App.Vector(0,0,.1))
assert native.shape_is_valid(working_set[200]) and working_set[200].calls==2
for i in range(native._VALIDITY_CACHE_LIMIT+1):
    shape=Part.makeBox(1,1,1,App.Vector(i,100,0))
    assert native.shape_is_valid(shape)
assert len(native._validity_cache)==native._VALIDITY_CACHE_LIMIT
(OUT/'result.json').write_text(json.dumps(dict(passed=True,identical_geometry_reuses_result=True,
    placement_change_revalidated=True,invalid_replacement_rejected=True,
    null_rejected=True,reopened_native_valid=True,working_set_2048_reused=True,
    changed_working_set_member_revalidated=True,cache_limit=native._VALIDITY_CACHE_LIMIT),indent=2))
print('Validity cache: unchanged reuse, changed/invalid/null BREP checks and bounded storage passed')
