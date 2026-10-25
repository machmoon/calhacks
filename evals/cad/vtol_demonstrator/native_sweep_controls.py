"""Native controls: a static-clear obstacle can intersect the full-turn envelope."""
import sys
from pathlib import Path
sys.path.insert(0,str(Path(__file__).resolve().parents[3]/'cad/freecad'))
import FreeCAD as App,Part
from lift_sweep import check
doc=App.newDocument('SweepControls');names=[]
for i in range(4):
 o=doc.addObject('Part::Feature','LiftProp'+str(i));o.Shape=Part.makeBox(200,10,2,App.Vector(-100,-5,0)).cut(Part.makeCylinder(3,2));o.Placement.Base=App.Vector(i*1000,0,0);names.append(o.Name)
doc.recompute();assert check(doc,names)['passed']
obstacle=doc.addObject('Part::Feature','LiftMountUpper0');obstacle.Shape=Part.makeBox(4,4,2,App.Vector(-2,88,0));doc.recompute()
assert doc.getObject(names[0]).Shape.common(obstacle.Shape).Volume<1e-8
result=check(doc,names+[obstacle.Name]);assert not result['passed'];assert result['checks'][0]['clashes'][0]['object']=='LiftMountUpper0'
group=doc.addObject('App::Part','RotatedParent')
for name in names+[obstacle.Name]:group.addObject(doc.getObject(name))
group.Placement=App.Placement(App.Vector(100,200,300),App.Rotation(App.Vector(1,0,0),90));doc.recompute()
nested=check(doc,names+[obstacle.Name]);assert not nested['passed'];row=nested['checks'][0]
assert row['clashes'][0]['object']==obstacle.Name
assert abs(row['clashes'][0]['overlap_mm3']-result['checks'][0]['clashes'][0]['overlap_mm3'])<1e-6
assert all(abs(a-b)<1e-7 for a,b in zip(row['center_world'],[100,200,300]))
assert all(abs(a-b)<1e-7 for a,b in zip(row['axis_world'],[0,-1,0]))
doc.getObject(names[0]).Shape=Part.makeBox(200,10,2,App.Vector(-100,-5,0));doc.recompute()
try:check(doc,names)
except ValueError as error:assert 'hub cylinder absent' in str(error)
else:raise AssertionError('Missing verified hub accepted')
doc.getObject(names[0]).Shape=Part.makeBox(200,10,2,App.Vector(-100,-5,0)).cut(Part.makeCylinder(3,1));doc.recompute()
try:check(doc,names)
except ValueError as error:assert 'shaft exclusion intersects' in str(error)
else:raise AssertionError('Blind bore incorrectly treated as clear through-shaft void')
print('PASS: clear envelopes, own mount swept collision, nested parent rotation, missing hub and blind-bore rejection')
