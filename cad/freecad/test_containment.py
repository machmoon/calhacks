"""Native containment rejects outside parts even when boundary distance is large."""
import hashlib
import json
from pathlib import Path
import FreeCAD as App
import Part
import containment

doc = App.newDocument('ContainmentTest')
def add(name, shape):
    obj = doc.addObject('Part::Feature', name)
    obj.Shape = shape
    return obj
add('Outer', Part.makeBox(20,20,20))
add('Cavity', Part.makeBox(18,18,18,App.Vector(1,1,1)))
add('PartInside', Part.makeBox(2,2,2,App.Vector(3,3,3)))
add('Outside', Part.makeBox(1,1,1,App.Vector(30,30,30)))
add('Touching', Part.makeBox(1,1,1))
add('Crossing', Part.makeBox(2,2,2,App.Vector(19,1,1)))
add('Holed', Part.makeBox(20,20,20).cut(Part.makeCylinder(3,20,App.Vector(10,10,0))))
add('InHole', Part.makeBox(1,1,1,App.Vector(9.5,9.5,5)))
add('Shell', Part.makePlane(3,3))
add('Mixed', Part.makeCompound([Part.makeBox(2,2,2), Part.makePlane(3,3)]))
doc.recompute()
def hashes():
    return {o.Name:hashlib.sha256(o.Shape.cleaned().exportBrepToString().encode()).hexdigest() for o in doc.Objects}
before = hashes()
wall = containment.check(doc, 'Outer', ['Cavity'], 1)
assert wall['passed'] and abs(wall['results'][0]['boundary_distance_mm']-1)<1e-7
assert not containment.check(doc, 'Outer', ['Cavity'], 1.1)['passed']
assert containment.check(doc, 'Cavity', ['PartInside'], 2)['passed']
assert not containment.check(doc, 'Outer', ['Outside'], 1)['passed']
assert not containment.check(doc, 'Outer', ['Crossing'])['passed']
assert containment.check(doc, 'Outer', ['Touching'])['passed']
assert not containment.check(doc, 'Outer', ['Touching'], .1)['passed']
assert not containment.check(doc, 'Holed', ['InHole'])['passed']
assert not containment.check(doc, 'Outer', ['PartInside','Outside'])['passed']
for args in [('Outer', [], 0), ('Outer', ['Outer'], 0), ('Outer', ['Cavity']*2, 0),
             ('Missing', ['Cavity'], 0), ('Outer', ['Shell'], 0), ('Outer', ['Mixed'], 0), ('Outer', ['Cavity'], True),
             ('Outer', ['Cavity'], float('nan')), ('Outer', ['Cavity'], -1),
             ('Outer', ['Cavity'], 1001)]:
    try: containment.check(doc, *args)
    except ValueError: pass
    else: raise AssertionError(('invalid request accepted', args))
assert before == hashes()
out = Path(__file__).resolve().parents[2]/'out/freecad-containment'
out.mkdir(parents=True,exist_ok=True)
doc.saveAs(str(out/'fixture.FCStd'))
App.closeDocument(doc.Name)
doc=App.openDocument(str(out/'fixture.FCStd'))
assert containment.check(doc,'Outer',['Cavity'],1)['passed']
(out/'result.json').write_text(json.dumps(dict(passed=True,wall=wall,
    source_geometry_preserved=True,reopen_passed=True,
    negative_controls=['outside','crossing','touching_clearance','internal_hole','mixed_contents','invalid_inputs']),indent=2))
App.closeDocument(doc.Name)
print('Containment native checks, negative controls, source preservation and reopen passed')
