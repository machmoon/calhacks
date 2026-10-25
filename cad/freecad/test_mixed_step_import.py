"""Preserve actual manufacturer controller surfaces without physical-solid claims."""
import hashlib,json
from pathlib import Path
import FreeCAD as App
import Part
import worker,native,import_geometry,step_import,brep_compare
from exchange import export_step
ROOT=Path(__file__).resolve().parents[2];OUT=ROOT/'out/native-mixed-step-import';OUT.mkdir(parents=True,exist_ok=True)
source=ROOT/'cad/components/vendor/holybro/pixhawk6x-pro.stp';digest=hashlib.sha256(source.read_bytes()).hexdigest()
worker.document('MixedComponent');doc=worker.active()
result=worker.METHODS['import_component'](component_id='holybro_pixhawk6x_pro',name='Pixhawk',preserve_reference_geometry=True)
solids=result['object_names'];references=result['reference_object_names']
assert len(solids)==18 and len(references)==13 and len(doc.Objects)==31
assert all(native.describe(doc.getObject(n))['reference_geometry'] for n in references)
assert all(native.describe(doc.getObject(n))['volume_mm3'] is None for n in references)
assert all(json.loads(o.WrightStepImport)['source_sha256']==digest for o in doc.Objects)
source_entities=step_import._entities(Part.read(str(source)),True)
ordered=[s for s,role in source_entities if role=='physical_solid']+[s for s,role in source_entities if role=='reference_surface']
preservation=[]
for obj in doc.Objects:
    m=json.loads(obj.WrightStepImport);original=ordered[m['source_entity_index']-1]
    comparison=brep_compare.compare(obj.Shape.cleaned().exportBrepToString(),original.cleaned().exportBrepToString())
    assert comparison['passed'],(obj.Name,comparison)
    preservation.append(dict(name=obj.Name,role=m['geometry_role'],comparison=comparison))
partial=worker.check([solids[0]]);assert partial['passed']
assert partial['geometry_coverage']['complete_source_geometry_checked'] is False
assert partial['geometry_coverage']['omitted_reference_objects']==sorted(references)
try:worker.check([references[0]])
except ValueError:pass
else:raise AssertionError('reference surface included as collision solid')
for action in [lambda:export_step([doc.getObject(solids[0])],OUT/'partial.step'),lambda:worker.save(str(OUT/'export'),[solids[0]])]:
    try:action()
    except ValueError as error:assert 'reference' in str(error).lower()
    else:raise AssertionError('partial mixed component exported with checked-solid claim')
assert not (OUT/'partial.step').exists()
rogue=doc.addObject('Part::Feature','UnmarkedShell');rogue.Shape=doc.getObject(references[0]).Shape.copy();doc.recompute()
try:native.validate_document(doc)
except ValueError:pass
else:raise AssertionError('unmarked shell accepted')
doc.removeObject('UnmarkedShell');doc.recompute()
image=worker.render([solids[0]],reference_objects=[references[0]],width=320,height=240,max_triangles=100000)
assert image['reference_objects']==[references[0]] and image['physical_objects']==[solids[0]]
assert image['content_blocks'][0]['type']=='image'
path=OUT/'component.FCStd';doc.saveAs(str(path));App.closeDocument(doc.Name);worker._doc=None
worker.open_document(str(path));doc=worker.active();assert len(native.validate_document(doc))==31
assert len([o for o in doc.Objects if import_geometry.is_reference(o)])==13
assert worker.check([solids[0]])['geometry_coverage']['omitted_reference_objects']==sorted(references)
assert hashlib.sha256(source.read_bytes()).hexdigest()==digest
(OUT/'result.json').write_text(json.dumps(dict(passed=True,source_sha256=digest,preserved_solids=18,preserved_reference_surfaces=13,source_unchanged=True,native_reopen=True,full_brep_preservation=preservation,partial_check=partial,checked_step_export_rejected=True,unmarked_shell_rejected=True,reference_render_verified=True,scope='Manufacturer simplified controller CAD; reference surfaces retained but excluded from physical mass/collision checks'),indent=2))
print('Holybro18solids+13reference surfaces retained, native reopen/render/coverage and strict export rejection passed')
App.closeDocument(doc.Name)
