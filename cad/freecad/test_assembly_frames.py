"""Native regression: nested assembly transforms, rotation and immutability."""
import FreeCAD as App
import Part
from assembly_frames import world_shape

doc=App.newDocument('FrameControls')
outer=doc.addObject('App::Part','Outer')
outer.Placement=App.Placement(App.Vector(100,20,0),App.Rotation(App.Vector(0,0,1),90))
inner=doc.addObject('App::Part','Inner');outer.addObject(inner)
inner.Placement.Base=App.Vector(10,0,5)
leaf=doc.addObject('Part::Feature','Leaf');inner.addObject(leaf)
leaf.Shape=Part.makeBox(2,3,4);leaf.Placement.Base=App.Vector(1,2,3)
doc.recompute()
before=leaf.Shape.exportBrepToString()
shape=world_shape(leaf)
expected=Part.makeBox(2,3,4)
expected.Placement=App.Placement(App.Vector(98,31,8),App.Rotation(App.Vector(0,0,1),90))
assert shape.cut(expected).Volume+expected.cut(shape).Volume<1e-7
assert leaf.Shape.exportBrepToString()==before
flat=doc.addObject('Part::Feature','Flat');flat.Shape=Part.makeBox(2,3,4)
flat.Placement.Base=App.Vector(100,20,5);doc.recompute()
assert world_shape(flat).cut(flat.Shape).Volume+flat.Shape.cut(world_shape(flat)).Volume<1e-7
obstacle=Part.makeBox(2,3,4);obstacle.Placement=expected.Placement
assert world_shape(leaf).common(obstacle).Volume>23.999
assert leaf.Shape.common(obstacle).Volume<1e-7
import native, worker
worker._doc=doc
fixed=doc.addObject('Part::Feature','Obstacle');fixed.Shape=obstacle;doc.recompute()
report=worker.check(['Leaf','Obstacle'])
assert report['overlap_count']==1
old_fingerprint=worker.geometry_fingerprint([leaf])
description=native.describe(leaf)
assert abs(description['bbox_mm'][0][0]-95)<1e-7
outer.Placement.Base.x+=100;doc.recompute()
assert worker.geometry_fingerprint([leaf])!=old_fingerprint
assert worker.check(['Leaf','Obstacle'])['overlap_count']==0
import tempfile
from pathlib import Path
from exchange import export_step
with tempfile.TemporaryDirectory() as directory:
    path=Path(directory)/'placed.step'
    export_step([leaf],path)
    roundtrip=Part.read(str(path));expected_world=world_shape(leaf)
    assert roundtrip.cut(expected_world).Volume+expected_world.cut(roundtrip).Volume<1e-7
App.closeDocument(doc.Name)
worker._doc=None
worker.document('NestedBooleanControls');doc=worker.active()
group=doc.addObject('App::Part','Group');group.Placement.Base=App.Vector(100,0,0)
tool=doc.addObject('Part::Box','Tool');group.addObject(tool);tool.Length=5;tool.Width=5;tool.Height=5
base=doc.addObject('Part::Box','Base');base.Length=10;base.Width=10;base.Height=10;base.Placement.Base=App.Vector(100,0,0)
doc.recompute()
worker.METHODS['boolean']('Pocket','cut','Base','Tool',retain_tool_as_part=True)
assert abs(doc.Pocket.Shape.Volume-875)<1e-7
assert worker.check(['Pocket','Tool'])['overlap_count']==0
group.Placement.Base.x=108;doc.recompute()
assert abs(doc.Pocket.Shape.Volume-950)<1e-7
assert worker.check(['Pocket','Tool'])['overlap_count']==0
with tempfile.TemporaryDirectory() as directory:
    saved=worker.save(directory,['Pocket','Tool'])
    assert saved['roundtrip_valid']
group.Placement.Base.x=0;base.Placement.Base.x=0;doc.recompute()
base2=doc.addObject('Part::Box','BaseAtOrigin');base2.Length=10;base2.Width=10;base2.Height=10
doc.recompute()
worker.METHODS['boolean']('OriginPocket','cut','BaseAtOrigin','Tool',retain_tool_as_part=True)
assert abs(doc.OriginPocket.Shape.Volume-875)<1e-7
group.Placement.Base.x=8;doc.recompute()
assert abs(doc.OriginPocket.Shape.Volume-950)<1e-7
App.closeDocument(doc.Name);worker._doc=None
print('Nested assembly placement, collision, fingerprint, description and STEP export pass')
