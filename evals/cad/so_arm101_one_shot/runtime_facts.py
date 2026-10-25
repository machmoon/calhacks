"""Small independent API capability probe; no model candidate is repaired."""
import json,sys
from pathlib import Path
import FreeCAD as App
out=Path(sys.argv[1]);out.parent.mkdir(parents=True,exist_ok=True)
doc=App.newDocument('RuntimeFacts');facts={'freecad_version':App.Version()}
try:doc.addObject('App::Feature','Invalid');facts['App::Feature_supported']=True
except TypeError as error:facts.update({'App::Feature_supported':False,'App::Feature_error':str(error)})
controls=doc.addObject('App::DocumentObjectGroup','JointParameters');controls.addProperty('App::PropertyAngle','test_angle');controls.test_angle=30
frame=doc.addObject('App::Part','Link_probe');frame.Placement=App.Placement(App.Vector(),App.Rotation(App.Vector(0,0,1),0));frame.setExpression('Placement.Rotation.Angle','JointParameters.test_angle');doc.recompute()
facts.update({'App::DocumentObjectGroup_custom_Angle':controls.getTypeIdOfProperty('test_angle')=='App::PropertyAngle','App::Part_Angle_expression':list(frame.ExpressionEngine),'angle_degrees':frame.Placement.Rotation.Angle*180/3.141592653589793})
assert facts['App::Feature_supported'] is False and facts['App::DocumentObjectGroup_custom_Angle']
assert abs(facts['angle_degrees']-30)<1e-8
out.write_text(json.dumps(facts,indent=2));print(json.dumps(facts));App.closeDocument(doc.Name)
