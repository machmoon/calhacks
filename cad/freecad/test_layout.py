"""Shared internal layout API: dependency edits, rollback and native persistence."""
import json,os
from pathlib import Path
import FreeCAD as App
import worker,native,layout
out=Path(os.environ.get('WRIGHT_CAD_OUT','out/freecad-layout')).resolve();out.mkdir(parents=True,exist_ok=True)
worker.document('LayoutTest');doc=worker.active()
worker.METHODS['make_parameters'](name='Dims',parameters={'Gap':{'value':12,'min':4,'max':20}})
for n in ['Spacer','Deck']:
    native.make_part(doc,dict(name=n,kind='box',params=dict(x=10,y=10,z=2)))
worker.METHODS['bind_parameter'](object_name='Spacer',parameter='Height',source_object='Dims',source_parameter='Gap')
worker.METHODS['bind_parameter'](object_name='Deck',parameter='Placement.Base.z',source_object='Dims',source_parameter='Gap')
assert worker.check(['Spacer','Deck'],[['Spacer','Deck']],require_connected=True)['passed']
worker.METHODS['set_parameter'](object_name='Dims',parameter='Gap',value=16)
assert doc.Spacer.Height.Value==16 and doc.Deck.Shape.BoundBox.ZMin==16
assert worker.current_check([doc.Spacer,doc.Deck])['status']=='not_checked'
for obj,param,val in [('Dims','Gap',21),('Spacer','Height',8)]:
    try:native.set_parameter(doc,obj,param,val)
    except ValueError:pass
    else:raise AssertionError('invalid or driven edit accepted')
assert doc.Dims.Gap.Value==16 and doc.Spacer.Height.Value==16
# A binding that destroys the target geometry must roll back atomically.
before=list(doc.Spacer.ExpressionEngine)
try:layout.bind(doc,'Spacer','Height','Dims','Gap',scale=1,offset_mm=-16)
except ValueError:pass
else:raise AssertionError('zero-height dependent shape accepted')
assert list(doc.Spacer.ExpressionEngine)==before and doc.Spacer.Height.Value==16
assert any(b['object_name']=='Dims' for b in native.parameter_bindings(doc.Spacer))
assert any(o.get('parameter_set') for o in worker.inspect_tree())
try:worker.selected_objects(['Dims'])
except ValueError:pass
else:raise AssertionError('shared parameter set accepted as physical output')
worker.check(['Spacer','Deck'],[['Spacer','Deck']],require_connected=True)
saved=worker.save(str(out),['Spacer','Deck'])
App.closeDocument(doc.Name);worker._doc=None
worker.restore(saved['native']);doc=worker.active()
native.set_parameter(doc,'Dims','Gap',18)
assert doc.Spacer.Height.Value==18 and doc.Deck.Shape.BoundBox.ZMin==18
assert worker.check(['Spacer','Deck'],[['Spacer','Deck']],require_connected=True)['passed']
(out/'layout-tests.json').write_text(json.dumps(dict(passed=True,source_edit=True,bounds_rollback=True,driven_edit_rejected=True,bad_binding_rollback=True,reopened_dependencies=True)))
print('Shared native layout tests passed')
