"""Actual mixed manufacturer component -> native + classified GLB, no STEP claim."""
import hashlib,json,struct
from pathlib import Path
import FreeCAD as App
import worker,native,import_geometry
ROOT=Path(__file__).resolve().parents[2];source=ROOT/'out/native-mixed-step-import/component.FCStd';digest=hashlib.sha256(source.read_bytes()).hexdigest();out=ROOT/'out/native-mixed-step-import/preview'
worker.document('Preview');worker.open_document(str(source));doc=worker.active()
solids=[o.Name for o in doc.Objects if not import_geometry.is_reference(o)];references=[o.Name for o in doc.Objects if import_geometry.is_reference(o)]
result=worker.save_preview(str(out),solids,references)
assert result['preview']['physical_nodes']==18 and result['preview']['reference_nodes']==13 and result['preview']['scene_nodes']==31
assert result['preview_coverage']['complete_source_geometry_shown']
assert result['step_export']['status']=='unavailable_in_preview_contract' and 'roundtrip_valid' not in result and 'step' not in result
assert not list(out.glob('*.step'))
data=Path(result['glb']).read_bytes();length,kind=struct.unpack_from('<II',data,12);gltf=json.loads(data[20:20+length])
nodes=[n for n in gltf['nodes'] if 'mesh' in n];assert len(nodes)==31
for node in nodes:
    expected=node['name'] in references;extra=node['extras']
    assert extra['reference_geometry']==expected
    assert extra['params']['geometry_role']==('reference_surface' if expected else 'physical_solid')
    assert extra['params']['excluded_from_mass_and_collision']==expected
    assert 'mass_g' not in extra and 'mass_kg' not in extra
    assert extra['step_import']['source_sha256']
assert gltf['asset']['extras']['reference_nodes']==13
App.closeDocument(doc.Name);worker._doc=None;worker.open_document(result['native']);doc=worker.active()
assert len(native.validate_document(doc))==31
assert all(doc.getObject(n).Visibility for n in solids+references)
assert hashlib.sha256(source.read_bytes()).hexdigest()==digest
(out/'test-result.json').write_text(json.dumps(dict(passed=True,source_native_unchanged=True,native_reopen_objects=31,classified_glb_nodes=31,physical_nodes=18,reference_nodes=13,visible_role_metadata=True,no_step_or_roundtrip_claim=True,result=result),indent=2))
print('Actual31-object controller native+GLB preview, visible role metadata, source preservation and native reopen pass')
App.closeDocument(doc.Name)
