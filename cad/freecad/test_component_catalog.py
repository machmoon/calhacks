"""Exact COTS lookup and manufacturer flight-controller import; no substitutions."""
import hashlib,json
from pathlib import Path
import FreeCAD as App
import worker,component_library
ROOT=Path(__file__).resolve().parents[2];OUT=ROOT/'out/native-component-catalog';OUT.mkdir(parents=True,exist_ok=True)
worker.document('ComponentCatalog');doc=worker.active()
listing=worker.METHODS['component_catalog']();assert len(listing['components'])==3
for query in ['AOS2807','Gemfan7042','Speedybee F405 Wing','tmotor_mn2806_650kv']:
    assert worker.METHODS['component_catalog'](query=query)['components']==[]
try:worker.METHODS['import_component'](component_id='speedybee_f405_wing_app',name='Missing')
except ValueError:pass
else:raise AssertionError('unavailable exact model silently substituted')
assert not doc.Objects
source=ROOT/'cad/components/vendor/holybro/pixhawk6x-pro.stp';source_hash=hashlib.sha256(source.read_bytes()).hexdigest()
try:worker.METHODS['import_component'](component_id='holybro_pixhawk6x_pro',name='Pixhawk6XPro')
except ValueError as error:
    assert 'non-solid geometry' in str(error)
else:raise AssertionError('mixed solid/shell source silently accepted')
assert not doc.Objects and hashlib.sha256(source.read_bytes()).hexdigest()==source_hash
assert component_library.catalog('tmotor_mn2806_evo_650kv')['components'][0]['id']=='tmotor_mn2806_evo'
assert component_library.catalog('tmotor_p12x4_cf')['components'][0]['id']=='tmotor_p12x4'
assert component_library.catalog('tmotor_mn2806_650kv')['components']==[]
assert component_library.catalog('tmotor_mn2806_evo_650kv')['components'][0]['identity']['mounting_frame_verified'] is False

(OUT/'result.json').write_text(json.dumps(dict(passed=True,catalog=listing,flight_controller_import_passed=False,mixed_solid_shell_import_rejected=True,rejection_leaves_document_empty=True,missing_selected_models_not_substituted=True,scope='Holybro source remains unsupported by solid-only import; no sheets were silently dropped'),indent=2))
print('Exact catalog identity and missing/mixed-geometry model rejection passed; Holybro import remains unsupported')
App.closeDocument(doc.Name)

