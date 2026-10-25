"""Small native negative controls for opt-in mixed STEP reference preservation."""
import json
from pathlib import Path
import FreeCAD as App
import Part
import worker,native,step_import
OUT=Path(__file__).resolve().parents[2]/'out/native-reference-contract';OUT.mkdir(parents=True,exist_ok=True)
source=OUT/'mixed.step';Part.makeCompound([Part.makeBox(2,3,4),Part.makePlane(2,3,App.Vector(5,0,0))]).exportStep(str(source))
# Nested source containers must compose their placements, not flatten local frames.
inner=Part.makeCompound([Part.makeBox(1,2,3),Part.makePlane(2,3,App.Vector(5,0,0))]);inner.Placement=App.Placement(App.Vector(10,20,30),App.Rotation(App.Vector(0,0,1),30))
outer=Part.makeCompound([inner]);outer.Placement=App.Placement(App.Vector(100,200,300),App.Rotation(App.Vector(1,0,0),15))
entities=step_import._entities(outer,True);expected=[outer.Solids[0],outer.Faces[-1]]
assert len(entities)==2
for (actual,role),reference in zip(entities,expected):
    assert max(abs(a-b) for a,b in zip(actual.Placement.toMatrix().A,reference.Placement.toMatrix().A))<1e-9
    assert (actual.CenterOfMass-reference.CenterOfMass).Length<1e-9
worker.document('ReferenceContract');doc=worker.active()
for options in [{},{'preserve_reference_geometry':'yes'},{'preserve_reference_geometry':1}]:
    try:worker.METHODS['import_step'](path=str(source),name='Rejected',**options)
    except ValueError:pass
    else:raise AssertionError('default or nonboolean reference mode accepted')
    assert not doc.Objects
result=worker.METHODS['import_step'](path=str(source),name='Mixed',preserve_reference_geometry=True)
assert result['solid_count']==1 and result['reference_surface_count']==1
solid=result['object_names'][0];reference=result['reference_object_names'][0]
summary=worker.inspect_tree(summary=True);assert [o['reference_geometry'] for o in summary['objects']]==[False,True]
for action in [lambda:worker.check([reference]),lambda:worker.render([solid],reference_objects=[solid]),lambda:worker.save(str(OUT/'export'),[solid])]:
    try:action()
    except ValueError:pass
    else:raise AssertionError('reference role boundary bypassed')
assert worker.check([solid])['geometry_coverage']['omitted_reference_objects']==[reference]
# An ordinary unrelated solid is not incorrectly marked partial.
native.make_part(doc,dict(name='Other',kind='box',params=dict(x=1,y=1,z=1)))
assert worker.check(['Other'])['geometry_coverage']['complete_source_geometry_checked']
assert worker.check_containment(solid,['Other'])['geometry_coverage']['complete_source_geometry_checked'] is False
print('Native explicit reference mode, strict booleans, roles, export rejection and coverage pass')
App.closeDocument(doc.Name)
