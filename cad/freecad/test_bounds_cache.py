"""Exact geometry cache must preserve trimmed/conservative bounds after edits."""
import json
from pathlib import Path
import FreeCAD as App
import Part
import native
class Counted:
 def __init__(self,shape):self.shape=shape;self.calls=0
 def isNull(self):return self.shape.isNull()
 def cleaned(self):return self.shape.cleaned()
 def optimalBoundingBox(self,*args):self.calls+=1;return self.shape.optimalBoundingBox(*args)
def values(b):return [b.XMin,b.YMin,b.ZMin,b.XMax,b.YMax,b.ZMax]
def same(a,b):assert max(abs(x-y) for x,y in zip(values(a),values(b)))<1e-10
native._bounds_cache.clear()
s=Counted(Part.makeSphere(10).common(Part.makeBox(12,9,8,App.Vector(-4,-3,-2))))
for conservative in [False,True]:
 expected=s.shape.optimalBoundingBox(False,conservative);first=native.shape_bounds(s,conservative);same(first,expected)
 first.add(App.Vector(1000,1000,1000));same(native.shape_bounds(s,conservative),expected)
assert s.calls==2
s.shape.translate(App.Vector(3,4,5));same(native.shape_bounds(s),s.shape.optimalBoundingBox(False,False));assert s.calls==3
s.shape=Part.makeCylinder(5,20);same(native.shape_bounds(s),s.shape.optimalBoundingBox(False,False));assert s.calls==4
out=Path(__file__).resolve().parents[2]/'out/freecad-bounds-cache';out.mkdir(parents=True,exist_ok=True)
doc=App.newDocument('BoundsCache');obj=doc.addObject('Part::Feature','Part');obj.Shape=s.shape;doc.saveAs(str(out/'fixture.FCStd'));App.closeDocument(doc.Name);doc=App.openDocument(str(out/'fixture.FCStd'));same(native.shape_bounds(doc.Part.Shape),doc.Part.Shape.optimalBoundingBox(False,False));App.closeDocument(doc.Name)
# Small configured budget makes LRU behavior observable without expensive geometry.
old=native._BOUNDS_CACHE_LIMIT;native._BOUNDS_CACHE_LIMIT=2;native._bounds_cache.clear()
for radius in [1,2,3]:native.shape_bounds(Part.makeSphere(radius))
assert len(native._bounds_cache)==2
native._BOUNDS_CACHE_LIMIT=old;native._bounds_cache.clear()
(out/'result.json').write_text(json.dumps(dict(passed=True,trimmed_geometry=True,conservative_modes_separate=True,returned_box_mutation_isolated=True,placement_and_geometry_edits_recomputed=True,reopen_bounds_verified=True,lru_bounded=True),indent=2))
print('Exact bounds cache: native geometry, edits, modes, mutation isolation, reopen and LRU pass')
