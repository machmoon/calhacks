"""Exact-identity local COTS CAD catalog; no inferred BOM substitutions."""
import hashlib,json
from pathlib import Path
import step_import
ROOT=Path(__file__).resolve().parents[1]/'components'


def _entries():
    catalog=json.loads((ROOT/'catalog.json').read_text())
    if catalog.get('schema_version')!=1:raise ValueError('unsupported component catalog schema')
    entries=catalog['components']
    if not isinstance(entries,list) or len(entries)>256:raise ValueError('invalid component catalog size')
    if len({e['id'] for e in entries})!=len(entries):raise ValueError('duplicate component IDs')
    return entries


def _resolve(entry):
    path=(ROOT/entry['file']).resolve();metadata=(ROOT/entry['source_metadata']).resolve()
    if not path.is_relative_to(ROOT.resolve()) or not metadata.is_relative_to(ROOT.resolve()):raise ValueError('component catalog path escapes root')
    source=json.loads(metadata.read_text())
    if source['model']!=entry['model'] or source['sha256']!=entry['sha256'] or source['file']!=path.name:
        raise ValueError('component identity disagrees with source metadata')
    if hashlib.sha256(path.read_bytes()).hexdigest()!=entry['sha256']:raise ValueError('component source hash mismatch')
    return path,dict(catalog_id=entry['id'],model=entry['model'],category=entry['category'],
        bom_catalog_ids=entry.get('bom_catalog_ids',[]),source_metadata=source,
        mounting_frame_verified=False,bom_identity_evidence=entry.get('bom_identity_evidence'),
        scope='Exact selected manufacturer model, not an inferred substitute for another BOM part')


def catalog(query=''):
    if not isinstance(query,str) or len(query)>200:raise ValueError('catalog query must be at most200 characters')
    rows=[]
    for entry in _entries():
        if query.lower() not in (entry['id']+' '+entry['model']+' '+entry['category']+' '+' '.join(entry.get('bom_catalog_ids',[]))).lower():continue
        row=dict(entry)
        try:
            _,identity=_resolve(entry);row.update(source_hash_verified=True,identity=identity)
        except (ValueError,OSError) as error:row.update(source_hash_verified=False,error=str(error))
        rows.append(row)
    return dict(components=rows,scope='Local CAD availability only; missing exact models remain unavailable, and the resolved BOM is not changed')


def import_component(doc,component_id,name,position=(0,0,0),axis=(0,0,1),angle_degrees=0,preserve_reference_geometry=False,max_source_mib=50,max_entities=512,summary=False,preserve_labels=True):
    matches=[entry for entry in _entries() if entry['id']==component_id]
    if len(matches)!=1:raise ValueError('unknown exact component ID; use component_catalog')
    entry=matches[0];path,identity=_resolve(entry)
    return step_import.import_step(doc,str(path),name,position,axis,angle_degrees,
        expected_sha256=entry['sha256'],component_identity=identity,preserve_reference_geometry=preserve_reference_geometry,max_source_mib=max_source_mib,max_entities=max_entities,summary=summary,preserve_labels=preserve_labels)
