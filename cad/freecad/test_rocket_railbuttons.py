"""Native source rail-button envelope and rejection regression."""
import os,sys,json,math
from pathlib import Path
sys.path.insert(0,os.environ['WRIGHT_ROCKET_WORKBENCH'])
import FreeCAD as App
from Rocket.Importer.OpenRocket.RailButtonElement import RailButtonElement
values={'outerdiameter':'0.0097','innerdiameter':'0.008','height':'0.0097','baseheight':'0.002','flangeheight':'0.002','screwheight':'0','instancecount':'2','instanceseparation':'0.381','angleoffset':'0'}
def build(overrides):
    doc=App.newDocument('RailButtonRegression')
    try:
        element=RailButtonElement(None,'railbutton',{},None,'fixture.ork',1)
        for tag,value in (values|overrides).items():element.handleEndTag(tag,value)
        element.end();doc.recompute()
        obj=element._feature._obj
        assert obj.Shape.isValid() and len(obj.Shape.Solids)==2
        expected=math.pi/4*(9.7**2*4+8**2*5.7)*2
        assert abs(obj.Shape.Volume-expected)<1e-6
        centers=sorted(s.CenterOfMass.x for s in obj.Shape.Solids)
        assert abs(centers[1]-centers[0]-381)<1e-6
        assert not obj.Fastener and obj.Length.Value==0
    finally:App.closeDocument(doc.Name)
build({})
for bad in [{'height':'nan'},{'angleoffset':'nan'},{'screwheight':'.001'},{'instancecount':'0'},{'height':'.002'}]:
    try:build(bad)
    except ValueError:pass
    else:raise AssertionError('invalid or unsupported rail button accepted: '+str(bad))
out=Path(os.environ['WRIGHT_CAD_OUT']);out.mkdir(parents=True,exist_ok=True)
(out/'railbutton-tests.json').write_text(json.dumps(dict(passed=True,valid_envelope_cases=1,rejected_cases=5,scope='Source envelope only; no screw-bore or mounting design claim')))
print('Native rail-button source envelope and rejection tests passed')
