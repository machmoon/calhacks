"""Trimmed loft bounds must not retain the whole supporting surface."""
import json
from pathlib import Path
import FreeCAD as App
import Part
import native,worker
ROOT=Path(__file__).resolve().parents[2];OUT=ROOT/'out/freecad-trimmed-bounds'
OUT.mkdir(parents=True,exist_ok=True)
worker.document('TrimmedBounds');doc=worker.active()
# A canted multi-face loft, intersected by distant thin station slabs.
native.make_part(doc,dict(name='Fin',kind='loft',params=dict(rings=[
    [[0,0,0],[10,2,0],[20,0,0],[10,-2,0]],
    [[4,-10,100],[14,-8,100],[24,-10,100],[14,-12,100]]])))
for name,z in [('Low',10),('High',80)]:
    slab=doc.addObject('Part::Box',name+'Slab');slab.Length=100;slab.Width=100;slab.Height=1
    slab.Placement.Base=App.Vector(-30,-40,z)
    rib=doc.addObject('Part::Common',name);rib.Base=doc.Fin;rib.Tool=slab
doc.recompute()
raw=doc.Low.Shape.BoundBox
tight=native.shape_bounds(doc.Low.Shape)
assert abs(tight.ZMin-10)<1e-6 and abs(tight.ZMax-11)<1e-6
assert abs(native.describe(doc.Low)['bbox_mm'][1][2]-11)<1e-6
conservative=native.shape_bounds(doc.Low.Shape,True)
for vertex in doc.Low.Shape.Vertexes:
    p=vertex.Point
    assert all(getattr(conservative,k+'Min')-1e-9<=getattr(p,k.lower())<=getattr(conservative,k+'Max')+1e-9 for k in 'XYZ')
fit=worker.check(['Low','High'])
pair=fit['pairs'][0]
assert pair['broad_phase'] and pair['overlap_mm3']==0 and pair['distance_lower_bound_mm']>68
# Independent exact check validates the separation proof on this trimmed case.
assert doc.Low.Shape.common(doc.High.Shape).Volume<1e-6
assert doc.Low.Shape.distToShape(doc.High.Shape)[0]>=pair['distance_lower_bound_mm']-1e-6
# Do not incorrectly screen out a real overlap after moving the second slab.
doc.HighSlab.Placement.Base.z=10.5;doc.recompute()
collision=worker.check(['Low','High'])
assert collision['overlap_count']==1 and not collision['pairs'][0]['broad_phase']
(OUT/'result.json').write_text(json.dumps(dict(passed=True,
    raw_z_extent_mm=[raw.ZMin,raw.ZMax],trimmed_z_extent_mm=[tight.ZMin,tight.ZMax],
    separated=pair,collision=collision['pairs'][0],conservative_vertices_enclosed=True),indent=2))
print('Trimmed bounds: accurate dimensions, conservative separation and real collision detection passed')
