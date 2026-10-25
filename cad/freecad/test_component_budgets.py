"""Default limits remain strict; explicit bounded budgets preserve validation."""
import hashlib,json
from pathlib import Path
import FreeCAD as App
import Part
import worker,native
ROOT=Path(__file__).resolve().parents[2];OUT=ROOT/'out/native-component-budgets';OUT.mkdir(parents=True,exist_ok=True)
source=OUT/'many-solids.step'
Part.makeCompound([Part.makeBox(1,1,1,App.Vector((i%19)*3,(i//19)*3,0)) for i in range(513)]).exportStep(str(source))
assert source.stat().st_size>1024*1024
original=hashlib.sha256(source.read_bytes()).hexdigest();worker.document('Budgets');doc=worker.active()
for options in [{},{'max_source_mib':1},{'max_entities':4097},{'max_source_mib':201},{'max_entities':True},{'summary':'yes'}]:
    try:worker.METHODS['import_step'](path=str(source),name='Rejected',**options)
    except ValueError:pass
    else:raise AssertionError('default or invalid budgets accepted')
    assert not doc.Objects
result=worker.METHODS['import_step'](path=str(source),name='Many',max_source_mib=20,max_entities=513,summary=True)
names=result['object_names'];assert len(names)==513 and result['validated_object_count']==513
assert 'objects' not in result and 'all_object_names' not in result
assert result['summary'] and result['objects_omitted'] and result['all_object_count']==513
assert len(json.dumps(result).encode())<=512*1024
assert result['provenance']['resource_budget']==dict(max_source_mib=20,max_entities=513,max_source_faces=100000)
assert worker.inspect_tree(summary=True)['next_offset']==100 and worker.inspect_tree([names[-1]])[0]['valid']
# Summary cannot bypass validation or transaction rollback.
single=OUT/'one.step';Part.makeBox(1,1,1).exportStep(str(single));doc.addObject('Part::Feature','Invalid');doc.recompute();before=[o.Name for o in doc.Objects]
try:worker.METHODS['import_step'](path=str(single),name='Rollback',summary=True)
except ValueError:pass
else:raise AssertionError('summary bypassed document validation')
assert [o.Name for o in doc.Objects]==before;doc.removeObject('Invalid');doc.recompute()
for options in [{},{'max_objects':True},{'max_objects':129,'max_source_faces':700},{'max_objects':129,'max_source_faces':100001}]:
    try:worker.render(names[:129],width=128,height=128,**options)
    except ValueError:pass
    else:raise AssertionError('render default/invalid budgets accepted')
render=worker.render(names[:129],width=128,height=128,max_objects=129,max_source_faces=774)
assert len(render['objects'])==129 and render['resource_budget']['max_objects']==129
try:worker.save_preview(str(OUT/'default-preview'),names)
except ValueError:pass
else:raise AssertionError('preview default512 silently raised')
preview=worker.save_preview(str(OUT/'explicit-preview'),names,max_objects=513)
assert preview['preview']['physical_nodes']==513 and preview['resource_budget']['max_objects']==513
assert preview['preview_coverage']['complete_source_geometry_shown']
assert hashlib.sha256(source.read_bytes()).hexdigest()==original
(OUT/'result.json').write_text(json.dumps(dict(passed=True,default_entity_and_preview_limits_preserved=True,default_render_limit_preserved=True,explicit_import_entities=513,explicit_render_objects=129,explicit_preview_objects=513,summary_validation_rollback=True,source_unchanged=True,import_result=result,preview=preview),indent=2))
print('Explicit component budgets pass:513 imported/previewed,129 rendered, defaults/summary-validation/source integrity preserved')
App.closeDocument(doc.Name)
