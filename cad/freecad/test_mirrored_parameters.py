"""Mirrored physical parts retain source controls without consuming the source."""
import json,struct
from pathlib import Path
import FreeCAD as App
import native,worker
ROOT=Path(__file__).resolve().parents[2];OUT=ROOT/'out/freecad-mirrored-parameters'
OUT.mkdir(parents=True,exist_ok=True)
worker.document('MirrorControls');doc=worker.active();call=worker.METHODS
call['make_parameters'](name='Layout',parameters={'PartLength':{'value':10,'min':5,'max':20}})
call['make_part'](spec={'name':'Right','kind':'box','params':{'x':10,'y':4,'z':3}})
call['place'](object_name='Right',position=[0,10,0])
call['bind_parameter'](object_name='Right',parameter='Length',source_object='Layout',source_parameter='PartLength')
call['mirror'](name='Left',source='Right',normal=[0,2,0])
left=doc.Left
assert left.WrightTypedMirror
assert abs(native.shape_bounds(left.Shape).YMin+14)<1e-7
assert abs(native.shape_bounds(left.Shape).YMax+10)<1e-7
for args in [dict(name='Zero',source='Right',normal=[0,0,0]),dict(name='Bad',source='Absent',normal=[0,1,0]),dict(name='Left',source='Right',normal=[0,1,0]),dict(name='Nan',source='Right',normal=[0,float('nan'),0])]:
 before=[o.Name for o in doc.Objects]
 try:call['mirror'](**args)
 except ValueError:pass
 else:raise AssertionError('invalid mirror accepted')
 assert before==[o.Name for o in doc.Objects]
call['mirror'](name='OffsetPlane',source='Right',normal=[0,1,0],origin=[0,3,0])
assert abs(native.shape_bounds(doc.OffsetPlane.Shape).YMin+8)<1e-7
assert abs(native.shape_bounds(doc.OffsetPlane.Shape).YMax+4)<1e-7
keys={(v['object_name'],v['parameter']) for v in native.parameter_bindings(left)}
assert ('Layout','PartLength') in keys and ('Right','Length') not in keys
assert len(worker.selected_objects(['Right','Left']))==2
call['set_parameter'](object_name='Layout',parameter='PartLength',value=15)
assert all(abs(doc.getObject(n).Shape.Volume-180)<1e-6 for n in ['Right','Left'])
assert worker.check(['Right','Left'])['passed']
exports=worker.save(str(OUT),['Right','Left'])
raw=Path(exports['glb']).read_bytes();length=struct.unpack_from('<I',raw,12)[0]
glb=json.loads(raw[20:20+length]);node=next(n for n in glb['nodes'] if n['name']=='Left')
assert any(p['object_name']=='Layout' and p['parameter']=='PartLength' for p in node['extras']['parameter_bindings'])
App.closeDocument(doc.Name);worker._doc=None;worker.restore(exports['native'])
call['set_parameter'](object_name='Layout',parameter='PartLength',value=12)
assert abs(worker.active().Left.Shape.Volume-144)<1e-6
assert worker.active().Left.WrightTypedMirror
call['place'](object_name='Right',position=[0,20,0])
assert abs(native.shape_bounds(worker.active().Left.Shape).YMin+24)<1e-7
(OUT/'result.json').write_text(json.dumps(dict(passed=True,shared_edit=True,both_physical_outputs=True,
    glb_mirror_controls=True,reopened_edit=True,exports=exports),indent=2))
print('Mirrored shared controls: native edits, two physical outputs, GLB metadata and reopen passed')
