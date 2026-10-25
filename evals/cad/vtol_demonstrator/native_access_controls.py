"""Linked fastener outward access, including nested frames and underside heads."""
import FreeCAD as App,Part
from service_access import check

def fixture(label,placement,underside=False):
 doc=App.newDocument(label);group=doc.addObject('App::Part','ParentFrame')
 head=doc.addObject('Part::Cylinder','FixtureBoltHead');head.Radius=2.75;head.Height=3;head.Placement.Base=App.Vector(10,20,20 if underside else 40)
 shaft=doc.addObject('Part::Cylinder','FixtureBoltShank');shaft.Radius=1.5;shaft.Height=20;shaft.Placement.Base=App.Vector(10,20,23 if underside else 20)
 bolt=doc.addObject('Part::MultiFuse','FixtureBolt');bolt.Shapes=[shaft,head]
 plate=doc.addObject('Part::Box','Plate');plate.Length=10;plate.Width=10;plate.Height=5;plate.Placement.Base=App.Vector(5,15,5 if underside else 50)
 for obj in [head,shaft,bolt,plate]:group.addObject(obj)
 group.Placement=placement;doc.recompute();before=[o.Shape.exportBrepToString() for o in [head,shaft,bolt,plate]]
 result=check(doc,['FixtureBolt','Plate'],['FixtureBolt','Plate']);assert not result['passed'];assert result['checks'][0]['clashes'][0]['object']=='Plate';assert before==[o.Shape.exportBrepToString() for o in [head,shaft,bolt,plate]]
 App.closeDocument(doc.Name);return result['checks'][0]
flat=fixture('Flat',App.Placement());nested=fixture('Nested',App.Placement(App.Vector(100,200,300),App.Rotation(App.Vector(1,0,0),90)))
assert abs(flat['clashes'][0]['overlap_mm3']-nested['clashes'][0]['overlap_mm3'])<1e-6
assert all(abs(a-b)<1e-7 for a,b in zip(nested['direction'],[0,-1,0]))
below=fixture('Underside',App.Placement(),True);assert below['direction']==[0,0,-1]
print('PASS: flat/nested driver obstruction invariant, underside direction correct, sources unchanged')
