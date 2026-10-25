"""Atomic binding handles coupled geometry and restores prior expressions on failure."""
import json
from pathlib import Path
import FreeCAD as App
import native,layout,worker
worker.document('AtomicBindings');doc=worker.active()
layout.create(doc,'Dimensions',{'Bore':dict(value=3,min=2,max=7)})
native.make_part(doc,dict(name='Tube',kind='tube',params=dict(radius=6,bore_radius=5,height=10)))
def binding(obj,offset=0,scale=1):return dict(object_name=obj,parameter='Radius',source_object='Dimensions',source_parameter='Bore',offset_mm=offset,scale=scale)
# The first target alone would make outer radius4 smaller than the old bore5.
r=layout.bind_many(doc,[binding('Tube_Outer',1),binding('Tube_Bore')]);assert r['binding_count']==2
assert abs(doc.Tube.Shape.Volume-70*3.141592653589793)<1e-7
before={o.Name:list(o.ExpressionEngine) for o in [doc.Tube_Outer,doc.Tube_Bore]};volume=doc.Tube.Shape.Volume
try:layout.bind_many(doc,[binding('Tube_Outer',1),binding('Tube_Bore',4)])
except ValueError:pass
else:raise AssertionError('invalid coupled geometry accepted')
assert before=={o.Name:list(o.ExpressionEngine) for o in [doc.Tube_Outer,doc.Tube_Bore]}
assert abs(doc.Tube.Shape.Volume-volume)<1e-7
invalid=[[],[binding('Tube_Outer')]*129,[binding('Tube_Outer'),binding('Tube_Outer')],[dict(binding('Tube_Outer'),expression='arbitrary')],[binding('Missing')],[dict(binding('Tube_Outer'),scale=True)]]
for batch in invalid:
 try:layout.bind_many(doc,batch)
 except (ValueError,TypeError):pass
 else:raise AssertionError('invalid binding batch accepted')
 assert before=={o.Name:list(o.ExpressionEngine) for o in [doc.Tube_Outer,doc.Tube_Bore]}
out=Path(__file__).resolve().parents[2]/'out/freecad-bind-many';out.mkdir(parents=True,exist_ok=True);doc.saveAs(str(out/'fixture.FCStd'));App.closeDocument(doc.Name);worker._doc=None;worker.open_document(str(out/'fixture.FCStd'));doc=worker.active()
native.set_parameter(doc,'Dimensions','Bore',4);assert abs(doc.Tube.Shape.Volume-90*3.141592653589793)<1e-7
(out/'result.json').write_text(json.dumps(dict(passed=True,coupled_geometry_updated_atomically=True,failed_batch_restores_expressions_and_geometry=True,invalid_batches_rejected=len(invalid),reopen_parameter_edit_passed=True),indent=2))
print('Atomic typed bindings: coupled geometry, rollback, invalid inputs and reopened edits pass')
