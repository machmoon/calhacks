"""Selected output export must not recursively duplicate feature children."""
import json,math,os
from pathlib import Path
import FreeCAD as App
import Part
import Import
from unittest.mock import patch
from exchange import export_step,export_glb
out=Path(os.environ['WRIGHT_CAD_OUT']).resolve();out.mkdir(parents=True,exist_ok=True)
doc=App.newDocument('ExchangeRegression')
parent=doc.addObject('Part::FeaturePython','Parent');parent.addExtension('App::GroupExtensionPython')
parent.Shape=Part.makeBox(10,10,10);parent.Placement.Base=App.Vector(3,4,5)
child=doc.addObject('Part::Cylinder','Child');child.Radius=1;child.Height=5;child.Placement.Base=App.Vector(15,0,0)
parent.Group=[child];doc.recompute()
expected=1000+5*math.pi
schema=export_step([parent,child],out/'selected.step')
shape=Part.read(str(out/'selected.step'))
assert schema=='AP242' and len(shape.Solids)==2 and abs(shape.Volume-expected)<1e-6
assert parent.Group==[child] and parent.Placement.Base==App.Vector(3,4,5)
preview=export_glb([parent,child],out/'selected.glb')
assert preview['physical_nodes']==2
original_export=Import.export
def corrupt_export(objects,path,**kwargs):
    # Remove internal material while retaining the source bounding box.
    objects[0].Shape=objects[0].Shape.cut(Part.makeCylinder(1,10,App.Vector(8,9,5)))
    return original_export(objects,path,**kwargs)
with patch('exchange.Import.export',corrupt_export):
    try:export_step([parent],out/'corrupted.step')
    except ValueError as error:assert 'geometry equivalence failed' in str(error)
    else:raise AssertionError('STEP material loss was accepted')
assert abs(parent.Shape.Volume-1000)<1e-6
holed=doc.addObject('Part::Feature','Holed')
block=Part.makeBox(10,10,5)
holed.Shape=block.cut(Part.makeCylinder(1,5,App.Vector(3,5,0)))
wrong_hole=block.cut(Part.makeCylinder(1,5,App.Vector(7,5,0)))
assert abs(holed.Shape.Volume-wrong_hole.Volume)<1e-6
assert all(abs(getattr(holed.Shape.BoundBox,k)-getattr(wrong_hole.BoundBox,k))<1e-6 for k in ['XMin','XMax','YMin','YMax','ZMin','ZMax'])
def move_hole_export(objects,path,**kwargs):
    objects[0].Shape=wrong_hole.copy()
    return original_export(objects,path,**kwargs)
export_step([holed],out/'wrong-hole.step')
assert (out/'wrong-hole.step.validation.json').exists()
with patch('exchange.Import.export',move_hole_export):
    try:export_step([holed],out/'wrong-hole.step')
    except ValueError as error:assert 'geometry equivalence failed' in str(error)
    else:raise AssertionError('equal-volume wrong-hole STEP was accepted')
assert not (out/'wrong-hole.step.validation.json').exists()
assert holed.Shape.common(Part.makeCylinder(1,5,App.Vector(3,5,0))).Volume<1e-6
(out/'exchange-tests.json').write_text(json.dumps(dict(passed=True,physical_solids=2,volume_mm3=shape.Volume,native_graph_preserved=True,material_loss_rejected=True,equal_volume_wrong_hole_rejected=True,step_schema=schema,preview=preview)))
App.closeDocument(doc.Name)
print('Grouped native feature export regression passed')
