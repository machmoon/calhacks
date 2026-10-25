"""Native inspector metadata and geometry-check freshness regression."""
import json,os,struct
from pathlib import Path
import FreeCAD as App
import native,worker
from exchange import export_glb
out=Path(os.environ['WRIGHT_CAD_OUT']).resolve();out.mkdir(parents=True,exist_ok=True)
worker.document('Metadata')
doc=worker.active()
for name in ('A','B'):
    native.make_part(doc,dict(name=name,kind='box',params=dict(x=10,y=10,z=10)))
    doc.getObject(name).Label='Duplicate human label'
selected=worker.selected_objects(['A','B'])
report=worker.check(['A','B'])
assert not report['passed'] and report['overlap_count']==1
report['pairs'].clear() # Returned reports must not mutate cached evidence.
def exported(name):
    path=out/(name+'.glb')
    export_glb(selected,path,worker.current_check(selected))
    raw=path.read_bytes();length=struct.unpack_from('<I',raw,12)[0]
    return json.loads(raw[20:20+length])
checked=exported('checked')
nodes={n['name']:n for n in checked['nodes'] if 'mesh' in n}
assert set(nodes)=={'A','B'}
assert nodes['A']['extras']['params']['Length']==10
assert nodes['A']['extras']['interferes_with']==['B']
assert checked['asset']['extras']['geometry_check']['status']=='checked'
# Bypass the action API: freshness must track actual geometry, not action counts.
doc.B.Placement.Base=App.Vector(30,0,0);doc.recompute()
stale=exported('stale')
assert stale['asset']['extras']['geometry_check']=={'status':'not_checked'}
assert all(not n['extras']['interferes_with'] for n in stale['nodes'] if 'mesh' in n)
assert worker.check(['A','B'])['passed']
assert worker.current_check([doc.A])['status']=='not_checked'
native.make_part(doc,dict(name='Tube',kind='tube',params=dict(radius=5,bore_radius=4,height=30)))
bindings=native.parameter_bindings(doc.Tube)
assert [o['name'] for o in worker.inspect_tree(['Tube','Tube_Outer'])]==['Tube','Tube_Outer']
assert set(worker.inspect_tree(['Tube'])[0]['dependencies'])=={'Tube_Outer','Tube_Bore'}
summary=worker.inspect_tree(['Tube','Tube_Outer'],summary=True,limit=1)
assert summary['total']==2 and summary['next_offset']==1 and not summary['geometry_checked']
assert summary['objects'][0]['name']=='Tube' and 'bbox_mm' not in summary['objects'][0]
assert worker.inspect_tree(['Tube','Tube_Outer'],summary=True,offset=1,limit=1)['next_offset'] is None
for kwargs in [dict(summary=1),dict(summary=True,limit=101),dict(offset=1),dict(summary=True,offset=-1)]:
    try:worker.inspect_tree(**kwargs)
    except ValueError:pass
    else:raise AssertionError('invalid summary inspection accepted')
for names in [[],['Tube','Tube'],['Missing'],['Tube',2]]:
    try:worker.inspect_tree(names)
    except ValueError:pass
    else:raise AssertionError('invalid scoped inspection accepted')
keys={(b['object_name'],b['parameter']) for b in bindings}
assert ('Tube_Outer','Height') in keys and ('Tube_Bore','Height') not in keys
for binding in bindings:
    assert binding['parameter'] in native.editable_parameters(doc.getObject(binding['object_name']))
shaft=doc.addObject('Part::Cylinder','Shaft');shaft.Radius=1;shaft.Height=10
head=doc.addObject('Part::Cylinder','Head');head.Radius=2;head.Height=2;head.Placement.Base.z=10
bolt=doc.addObject('Part::Fuse','Bolt');bolt.Base=shaft;bolt.Tool=head;doc.recompute()
assert ('Shaft','Height') in {(b['object_name'],b['parameter']) for b in native.parameter_bindings(bolt)}
try:worker.selected_objects(['Bolt','Shaft'])
except ValueError:pass
else:raise AssertionError('Fuse construction was accepted as another physical output')
worker.save(str(out/'saved'),['A','B'])
assert json.loads((out/'saved/geometry-check.json').read_text())['passed']
(out/'metadata-tests.json').write_text(json.dumps(dict(passed=True,duplicate_labels=True,parameters=True,check_freshness=True,selection_scope=True,scoped_inspection=True,paged_summary=True,expression_filter=True)))
print('Native metadata and freshness regression passed')
