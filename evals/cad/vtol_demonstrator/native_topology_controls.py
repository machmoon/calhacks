"""A valid compound of disconnected solids is not one manufactured part."""
import FreeCAD as App,Part
from native_probe import valid
doc=App.newDocument('TopologyControls');part=doc.addObject('Part::Feature','PartUnderTest');part.Shape=Part.makeBox(10,10,10);doc.recompute();assert valid(doc,[part.Name])[0]['passed']
part.Shape=Part.makeCompound([Part.makeBox(10,10,10),Part.makeBox(10,10,10,App.Vector(20,0,0))]);doc.recompute();assert part.Shape.isValid();result=valid(doc,[part.Name])[0];assert result['solids']==2 and not result['passed']
print('PASS: valid single solid accepted; valid disconnected two-solid part rejected')
