"""Open, edit, save and resume live Rocket features through the native worker."""
import json,os,hashlib
from pathlib import Path
import FreeCAD as App
import Part
import native,worker
root=Path(__file__).resolve().parents[2]
source=root/'out/freecad-openrocket-verified/A simple model rocket/imported.FCStd'
out=Path(os.environ['WRIGHT_CAD_OUT']).resolve();out.mkdir(parents=True,exist_ok=True)
source_hash=hashlib.sha256(source.read_bytes()).hexdigest()
frozen=App.newDocument('FrozenFixture');obj=frozen.addObject('Part::FeaturePython','MissingProxy');obj.Shape=Part.makeBox(1,2,3)
frozen.saveAs(str(out/'missing-proxy.FCStd'));App.closeDocument(frozen.Name)
worker.document('EmptySession');empty_name=worker.active().Name
try:worker.open_document(str(out/'missing-proxy.FCStd'))
except ValueError as error:assert 'live recompute' in str(error)
else:raise AssertionError('frozen Python feature accepted')
assert worker.active().Name==empty_name and not worker.active().Objects
opened=worker.open_document(str(source));doc=worker.active()
assert opened['opened'] and native.describe(doc.InnerTube)['editable_parameters']['Length']==75
before=doc.InnerTube.Shape.Volume
before_length_x=doc.InnerTube.Placement.Base.x
native.set_parameter(doc,'InnerTube','Length',80)
assert abs(doc.InnerTube.Shape.Volume/before-80/75)<1e-7
assert abs(doc.InnerTube.Placement.Base.x-(before_length_x-5))<1e-6
try:native.set_parameter(doc,'InnerTube','Thickness',1000)
except ValueError:pass
else:raise AssertionError('invalid tube wall accepted')
assert doc.InnerTube.Length.Value==80
assert abs(doc.InnerTube.Shape.Volume/before-80/75)<1e-7
wall=doc.InnerTube.Thickness.Value
engine_diameter=doc.EngineBlock.Diameter.Value
assert doc.EngineBlock.AutoDiameter
native.set_parameter(doc,'InnerTube','Thickness',wall+.1)
assert abs(doc.EngineBlock.Diameter.Value-(engine_diameter-.2))<1e-6
native.set_parameter(doc,'InnerTube','Thickness',wall)
assert abs(doc.EngineBlock.Diameter.Value-engine_diameter)<1e-6
try:worker.open_document(str(source))
except ValueError as error:assert 'empty CAD session' in str(error)
else:raise AssertionError('open_document replaced existing edits')
assert doc.InnerTube.Length.Value==80
try:worker.selected_objects(['Rocket'])
except ValueError:pass
else:raise AssertionError('assembly container selected as physical geometry')
# Physical parent/child membership must not be confused with boolean construction.
exports=worker.save(str(out/'selected'),['BodyTube','InnerTube'])
assert exports['roundtrip_valid']
checkpoint=worker.checkpoint(str(out/'resume'))
App.closeDocument(doc.Name);worker._doc=None
worker.restore(checkpoint['checkpoint']);doc=worker.active()
assert doc.InnerTube.Length.Value==80 and callable(doc.InnerTube.Proxy.execute)
resumed_x=doc.InnerTube.Placement.Base.x
native.set_parameter(doc,'InnerTube','Length',85)
assert abs(doc.InnerTube.Placement.Base.x-(resumed_x-5))<1e-6
assert abs(doc.InnerTube.Shape.Volume/before-85/75)<1e-7
before_offset=doc.InnerTube.AxialOffset.Value;before_x=doc.InnerTube.Placement.Base.x
native.set_parameter(doc,'InnerTube','AxialOffset',-5)
assert abs(doc.InnerTube.Placement.Base.x-before_x-(-5-before_offset))<1e-6
try:worker.place('InnerTube',[0,0,0])
except ValueError as error:assert 'assembly-controlled' in str(error)
else:raise AssertionError('direct Rocket placement accepted')
assert hashlib.sha256(source.read_bytes()).hexdigest()==source_hash
App.closeDocument(doc.Name);worker._doc=None
high=root/'out/freecad-openrocket-verified/Two stage high power rocket/imported.FCStd'
high_hash=hashlib.sha256(high.read_bytes()).hexdigest()
worker.open_document(str(high));doc=worker.active()
length=doc.InnerTube.Length.Value;volume=doc.InnerTube.Shape.Volume
native.set_parameter(doc,'InnerTube','Length',length+1)
assert abs(doc.InnerTube.Shape.Volume/volume-(length+1)/length)<1e-7
assert hashlib.sha256(high.read_bytes()).hexdigest()==high_hash
(out/'rocket-session-tests.json').write_text(json.dumps(dict(passed=True,source_unchanged=True,live_resume_verified=True,invalid_edits_rejected=True,missing_proxy_rejected=True,physical_child_export_verified=True,dependent_positions_verified=True,dependent_diameter_verified=True,high_power_live_edit_verified=True)))
App.closeDocument(doc.Name);worker._doc=None
print('Native Rocket open/edit/rollback/export/resume tests passed')
