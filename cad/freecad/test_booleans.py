"""Native boolean API, dependency edits, rollback, selection and reopen regression."""
import json
import os
from pathlib import Path
from types import SimpleNamespace
import FreeCAD as App
import booleans
import native
import worker

OUT=Path(os.environ.get('WRIGHT_CAD_OUT','out/freecad-booleans')).resolve()
OUT.mkdir(parents=True,exist_ok=True)
worker.document('BooleanRegression');doc=worker.active()
for name in ('Base','Tool'):
    native.make_part(doc,dict(name=name,kind='box',params=dict(x=10,y=10,z=10)))
worker.place('Tool',[5,0,0])
checks=[]

def rejected(label, fn):
    before={o.Name:(o.Shape.Volume if hasattr(o,'Shape') else None) for o in doc.Objects}
    try:fn()
    except (ValueError,TypeError):pass
    else:raise AssertionError('accepted '+label)
    after={o.Name:(o.Shape.Volume if hasattr(o,'Shape') else None) for o in doc.Objects}
    assert before==after,(label,before,after)
    checks.append(label)

for op,expected in [('cut',500),('fuse',1500),('common',500)]:
    result=worker.METHODS['boolean'](name=op.title(),operation=op,base='Base',tool='Tool')
    assert abs(result['volume_mm3']-expected)<1e-7
    assert set(result['dependencies'])=={'Base','Tool'}
    checks.append(op+'_native_dependency')
rejected('duplicate_name',lambda:booleans.create(doc,'Cut','cut','Base','Tool'))
rejected('self_reference',lambda:booleans.create(doc,'Self','cut','Self','Tool'))
rejected('invalid_name',lambda:booleans.create(doc,'Bad.Name','cut','Base','Tool'))
rejected('unknown_operand',lambda:booleans.create(doc,'Missing','cut','Absent','Tool'))
rejected('identical_operands',lambda:booleans.create(doc,'Same','cut','Base','Base'))
rejected('unknown_operation',lambda:booleans.create(doc,'BadOp','shell','Base','Tool'))
native.make_part(doc,dict(name='Distant',kind='box',params=dict(x=10,y=10,z=10)))
worker.place('Distant',[50,0,0])
rejected('no_effect_cut',lambda:booleans.create(doc,'NoEffect','cut','Base','Distant'))
rejected('disconnected_fuse',lambda:booleans.create(doc,'Disconnected','fuse','Base','Distant'))
rejected('empty_common',lambda:booleans.create(doc,'EmptyCommon','common','Base','Distant'))
native.make_part(doc,dict(name='Cover',kind='box',params=dict(x=20,y=20,z=20)))
rejected('empty_cut',lambda:booleans.create(doc,'EmptyCut','cut','Base','Cover'))
rejected('contained_fuse_no_effect',lambda:booleans.create(doc,'SameFuse','fuse','Base','Cover'))
# Creation cannot introduce cycles because its name must be new, and operands
# must preexist. Exercise rejection of an already-corrupt dependency graph too.
a=SimpleNamespace(Name='A',OutList=[]);b=SimpleNamespace(Name='B',OutList=[a]);a.OutList=[b]
try:booleans._acyclic([a])
except ValueError:checks.append('existing_cycle_rejected')
else:raise AssertionError('cycle accepted')
rejected('parent_and_helper_selection',lambda:worker.selected_objects(['Cut','Tool']))
assert worker.selected_objects(['Cut'])==[doc.Cut]
native.set_parameter(doc,'Base','Width',12)
assert abs(doc.Cut.Shape.Volume-700)<1e-7
assert abs(doc.Fuse.Shape.Volume-1700)<1e-7
assert abs(doc.Common.Shape.Volume-500)<1e-7
checks.append('dependent_edit_recomputes_all_operations')
rejected('dependent_invalid_move_rollback',lambda:worker.place('Tool',[50,0,0]))
assert doc.Tool.Placement.Base.x==5
rejected('dependent_no_effect_cut_edit_rollback',lambda:native.set_parameter(doc,'Base','Length',5))
assert doc.Base.Length.Value==10
checkpoint=worker.checkpoint(str(OUT))['checkpoint']
App.closeDocument(doc.Name);worker._doc=None
worker.restore(checkpoint);doc=worker.active()
assert doc.Cut.WrightTypedBoolean and doc.Cut.Base.Name=='Base'
native.set_parameter(doc,'Base','Width',14)
assert abs(doc.Cut.Shape.Volume-900)<1e-7
checks.append('checkpoint_reopen_dependency_edit')
rejected('reopened_validation_persists',lambda:worker.place('Tool',[50,0,0]))
worker.check(['Cut'])
exports=worker.save(str(OUT/'cut'),['Cut'])
assert exports['roundtrip_valid'] and exports['preview']['physical_nodes']==1
checks.append('final_only_export')
(OUT/'result.json').write_text(json.dumps(dict(passed=True,checks=checks,exports=exports),indent=2))
App.closeDocument(doc.Name);worker._doc=None
print('Native boolean regression passed:',len(checks),'checks')
