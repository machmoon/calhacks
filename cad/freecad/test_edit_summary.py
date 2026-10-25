"""Worker-only compact edit responses preserve native validation and rollback."""
import json
from pathlib import Path
import FreeCAD as App
import native,worker

OUT=Path(__file__).resolve().parents[2]/'out/freecad-edit-summary'
OUT.mkdir(parents=True,exist_ok=True)
worker.document('EditSummary');doc=worker.active()
for name,radius in [('Outer',5),('Bore',2)]:
    worker.METHODS['make_part'](dict(name=name,kind='cylinder',params=dict(radius=radius,height=10)))
worker.METHODS['boolean'](name='Tube',operation='cut',base='Outer',tool='Bore')
worker.METHODS['mirror'](name='MirrorTube',source='Tube',normal=[0,1,0],origin=[0,20,0])
worker.METHODS['make_part'](dict(name='Unrelated',kind='box',params=dict(x=2,y=2,z=2)))
worker.check(['Tube','MirrorTube'])
result=worker.METHODS['set_parameter'](object_name='Bore',parameter='Radius',value=2.5,summary=True)
assert result['edit']==dict(action='set_parameter',object_name='Bore',parameter='Radius',requested_value=2.5,value=2.5)
assert result['geometry_validated'] is True and result['assembly_contacts_checked'] is False
assert result['validated_object_count']==len(native.validate_document(doc))==5
assert result['dependent_object_names']==['MirrorTube','Tube']
assert worker.current_check(worker.selected_objects(['Tube','MirrorTube']))=={'status':'not_checked'}
old=doc.Tube.Shape.Volume
try:worker.METHODS['set_parameter'](object_name='Bore',parameter='Radius',value=6,summary=True)
except ValueError:pass
else:raise AssertionError('invalid dependent cut accepted')
assert float(doc.Bore.Radius)==2.5 and abs(doc.Tube.Shape.Volume-old)<1e-6
# An unrelated invalid object still causes full-document rejection and rollback.
bad=doc.addObject('Part::Feature','Invalid');doc.recompute()
try:worker.METHODS['set_parameter'](object_name='Bore',parameter='Radius',value=2.6,summary=True)
except ValueError:pass
else:raise AssertionError('summary bypassed full validation')
assert float(doc.Bore.Radius)==2.5 and abs(doc.Tube.Shape.Volume-old)<1e-6
doc.removeObject(bad.Name);doc.recompute()
for flag in (1,'true',None):
    try:worker.METHODS['set_parameter'](object_name='Bore',parameter='Radius',value=3,summary=flag)
    except ValueError:pass
    else:raise AssertionError('nonboolean summary accepted')
assert float(doc.Bore.Radius)==2.5
legacy=worker.METHODS['set_parameter'](object_name='Bore',parameter='Radius',value=2)
assert isinstance(legacy,list) and len(legacy)==5
rings=[[[0,0,z],[10,0,z],[10,10,z],[0,10,z]] for z in [0,20]]
worker.METHODS['make_part'](dict(name='Loft',kind='loft',params=dict(rings=rings)))
points=[[0,0,20],[12,0,20],[12,12,20],[0,12,20]]
profile=worker.METHODS['set_profile'](object_name='Loft_Section1',points=points,summary=True)
assert profile['edit']==dict(action='set_profile',object_name='Loft_Section1',submitted_point_count=4,geometry_count=4,profile_type='polygon')
assert profile['dependent_object_names']==['Loft'] and profile['validated_object_count']==8
assert profile['assembly_contacts_checked'] is False
before=doc.Loft.Shape.Volume;bad=doc.addObject('Part::Feature','Invalid');doc.recompute()
try:worker.METHODS['set_profile'](object_name='Loft_Section1',points=rings[1],summary=True)
except ValueError:pass
else:raise AssertionError('profile summary bypassed full validation')
assert abs(doc.Loft.Shape.Volume-before)<1e-6
doc.removeObject(bad.Name);doc.recompute()
assert isinstance(worker.METHODS['set_profile'](object_name='Loft_Section1',points=rings[1]),list)
(OUT/'result.json').write_text(json.dumps(dict(passed=True,parameter_summary=result,profile_summary=profile,invalid_cut_rollback=True,unrelated_invalid_geometry_rejected=True,profile_rollback=True,legacy_lists_preserved=True),indent=2))
print('Compact edits: accurate metadata, transitive dependencies, full validation, rollback and legacy returns passed')
App.closeDocument(doc.Name)
