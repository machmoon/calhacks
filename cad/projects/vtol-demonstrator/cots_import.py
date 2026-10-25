"""Verified vendor STEP imports for the VTOL candidate; no layout or fabrication claims.

import_component(doc, model_key, name, placement) returns an App::Part whose
Part::Feature children retain source coordinates. Its explicit Placement is the
only assembly transform. physical_objects(instance) excludes reference surfaces.
All geometry/provenance survives save/reopen without Python proxies.
"""
from pathlib import Path
import hashlib
import json
import math
import re

ROOT = Path(__file__).resolve().parent / 'package-v2'
MODELS = {
    'fc_current': ('sources/holybro-cad/fc_current.stp', 'b0855e2bd5117b0c8e4fc82755142bee34e5a394b7fc0d7f6300e3b78348c188', 2, 0,
        'Holybro Pixhawk 6C Mini current Model A; manufacturer casing CAD'),
    'gps_shared_full': ('sources/holybro-cad/gps_shared_full.stp', '42587789183f82d0ab59da80f0149d355ff4df99695bb2ddc071354225197fce', 7, 0,
        'Holybro shared M8N/M9N/M10 housing assembly; not exact M10 electronic internals'),
    'pdb_top': ('sources/holybro-cad/pdb_top.stp', 'e342cd094063dc7eb70f65d90f48e774106b6569a8ddca65de76b83452158e65', 27, 0,
        'Holybro 18138 top-entry PDB; no side-entry substitution'),
    'hs65mg': ('sources/servo-cad/HS-65MG_221121.stp', '1a9ea39e82de930becafc749ad61b78fa6d360c4b810980e2504305eaf84453c', 12, 0,
        'Hitec HS65MG 2022-11-21 manufacturer STEP with horn and internal geometry'),
}


def measured_interface(model_key):
    """Source-frame measurements only; no inferred mounting/thread permission."""
    if model_key not in MODELS:
        raise ValueError('unknown verified model')
    common = {'frame': 'unchanged vendor STEP coordinates, mm', 'installed_fit_verified': False,
              'strength_verified': False, 'thread_engagement_authorized': False}
    if model_key == 'hs65mg':
        common.update(json.loads((ROOT / 'servo-interface.json').read_text()))
        common['source_geometry'] = '12 maximal solids;13 constituent shells include an inner shell, not a free reference surface'
        common['topology_correction'] = 'Maximal entity traversal supersedes prior shell-minus-solid inference'
    elif model_key == 'pdb_top':
        common.update(mounting={'hole_centers_mm': [[x, y, 0] for x in (90.00018034036, 140.0002794005)
                                                   for y in (110.0002209804, 160.0003200406)],
                               'axis': [0, 0, 1], 'bore_diameter_mm': 3.30000508001,
                               'PCB_z_mm': [0, 1.317602235204],
                               'lowest_solder_z_mm': -1.682397764797,
                               'meaning': 'clearance bores, not tapped M3 threads'},
                      published_envelope_mm=[60, 80, 23.5])
    else:
        report = json.loads((ROOT / 'holybro-geometry-inspection.json').read_text())
        row = next(x for x in report['parts'] if x['id'] == model_key)
        common.update(bounds_mm=row['bounds_mm'], measured_mounting=None,
                      mounting_status='No aircraft attachment interface qualified; use actual source geometry for cradle design. Casing screw features are not authorized mounting points.')
    return common


def _entities(shape):
    # Stop at solids so their constituent shells/faces cannot be exported twice.
    if shape.ShapeType in ('Solid', 'Shell', 'Face'):
        return [shape]
    if shape.ShapeType in ('Compound', 'CompSolid'):
        return [part for child in shape.childShapes() for part in _entities(child)]
    raise ValueError('unsupported source entity type: ' + shape.ShapeType)


def _property(obj, name, value):
    obj.addProperty('App::PropertyString', name, 'COTS provenance')
    setattr(obj, name, value if isinstance(value, str) else json.dumps(value, sort_keys=True))
    obj.setEditorMode(name, 1)


def import_component(doc, model_key, name, placement):
    import FreeCAD as App
    import Part
    if model_key not in MODELS:
        raise ValueError('unknown verified model')
    if not isinstance(name, str) or not re.fullmatch(r'[A-Za-z][A-Za-z0-9_]{0,63}', name):
        raise ValueError('name must be a bounded native identifier')
    if not isinstance(placement, App.Placement):
        raise TypeError('explicit FreeCAD.Placement required')
    if not all(math.isfinite(float(x)) for x in list(placement.Base) + list(placement.Rotation.Q)):
        raise ValueError('placement must be finite')
    if doc is None or doc.getObject(name):
        raise ValueError('document required and instance name must be unused')
    rel, sha, ns, nr, scope = MODELS[model_key]
    source = ROOT / rel
    if hashlib.sha256(source.read_bytes()).hexdigest() != sha:
        raise ValueError('vendor source SHA256 mismatch')
    shape = Part.Shape()
    shape.read(str(source))
    entities = _entities(shape)
    solids = [s for s in entities if s.ShapeType == 'Solid']
    refs = [s for s in entities if s.ShapeType != 'Solid']
    if len(solids) != ns or len(refs) != nr or not shape.isValid():
        raise ValueError('source shape inventory/validity changed')
    if any(not s.isValid() or s.Volume <= 0 for s in solids):
        raise ValueError('invalid physical source solid')
    if any(not s.isValid() or not s.Faces or s.Area <= 0 for s in refs):
        raise ValueError('invalid reference surface')
    names = [name] + [f'{name}_Solid{i:03}' for i in range(ns)] + [f'{name}_Reference{i:03}' for i in range(nr)]
    if any(doc.getObject(n) for n in names):
        raise ValueError('native object name collision')
    interface = measured_interface(model_key)
    before = {o.Name for o in doc.Objects}
    doc.openTransaction('Import verified COTS ' + name)
    try:
        group = doc.addObject('App::Part', name)
        group.Placement = placement
        _property(group, 'COTSModelKey', model_key)
        _property(group, 'COTSSourceSHA256', sha)
        _property(group, 'COTSSourcePath', str(source))
        _property(group, 'COTSScope', scope)
        _property(group, 'COTSMeasuredInterface', interface)
        _property(group, 'COTSCoverage', {'physical_solids': ns, 'reference_surfaces': nr,
                                        'all_source_entities_retained': True,
                                        'physical_only_export_omits_reference_surfaces': bool(nr)})
        for role, shapes in [('Solid', solids), ('Reference', refs)]:
            for i, entity in enumerate(shapes):
                obj = doc.addObject('Part::Feature', f'{name}_{role}{i:03}')
                obj.Shape = entity.copy()
                group.addObject(obj)
                # Metadata interoperates with Wright's existing reference classification.
                _property(obj, 'WrightStepImport', {'source_sha256': sha, 'source_path': str(source),
                    'source_type': 'manufacturer_STEP', 'model_key': model_key,
                    'import_instance': name, 'source_solid_count': ns,
                    'source_reference_surface_count': nr,
                    'geometry_role': 'physical_solid' if role == 'Solid' else 'reference_surface',
                    'source_entity_index': i, 'scope': scope})
                obj.Label = f'{name}: {role.lower()} {i + 1}'
        doc.recompute()
        doc.commitTransaction()
        return group
    except Exception:
        doc.abortTransaction()
        # Defensive cleanup for builds whose transaction did not remove new objects.
        for obj in reversed(doc.Objects):
            if obj.Name not in before:
                doc.removeObject(obj.Name)
        doc.recompute()
        raise


def physical_objects(instance):
    """Return direct solid leaves, never the aggregate parent or reference shell."""
    return [o for o in instance.Group if hasattr(o, 'WrightStepImport')
            and json.loads(o.WrightStepImport).get('geometry_role') == 'physical_solid']
