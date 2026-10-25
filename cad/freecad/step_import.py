"""Bounded STEP solid snapshots with file-derived units and import provenance."""
import hashlib,json,math,re,uuid
from pathlib import Path
import FreeCAD as App
import Import
import Part
import native
import resource_budgets


from step_units import declared_units


def _entities(shape,preserve_reference_geometry=False):
    if shape.ShapeType=='Solid':return [(shape,'physical_solid')]
    if preserve_reference_geometry and shape.ShapeType in ('Shell','Face'):
        return [(shape,'reference_surface')]
    if shape.ShapeType not in ('Compound','CompSolid'):raise ValueError('STEP contains non-solid geometry; nothing was imported')
    return [entity for child in shape.childShapes(True,True) for entity in _entities(child,preserve_reference_geometry)]


def _signature(shape):
    b=native.shape_bounds(shape)
    return [b.XMin,b.YMin,b.ZMin,b.XMax,b.YMax,b.ZMax,shape.Volume]


def _labels(path,reference_solids):
    """Recover labels where native assembly leaves match authoritative solids.

    Geometry always comes from Part.read, not this optional label lookup. This
    avoids importing App::Part aggregate shapes twice or confusing definitions
    with repeated link instances. Ambiguous/missing labels are reported honestly.
    """
    original=App.ActiveDocument;temp=App.newDocument('WrightStepLabels')
    try:
        Import.insert(str(path),temp.Name);temp.recompute()
        candidates=[]
        for obj in temp.Objects:
            if obj.TypeId!='Part::Feature':continue
            shape=obj.Shape.copy()
            shape.Placement=obj.getGlobalPlacement().multiply(obj.Placement.inverse()).multiply(shape.Placement)
            for solid,role in _entities(shape,True):candidates.append((_signature(solid),obj.Label))
        labels=[]
        for solid in reference_solids:
            signature=_signature(solid)
            matched=sorted({label for values,label in candidates if all(abs(a-b)<=max(1e-5,abs(b)*1e-8) for a,b in zip(values,signature))})
            labels.append(matched)
        return labels
    finally:
        App.closeDocument(temp.Name)
        if original is not None:App.setActiveDocument(original.Name)


def import_step(doc,path,name,position=(0,0,0),axis=(0,0,1),angle_degrees=0,expected_sha256=None,component_identity=None,preserve_reference_geometry=False,max_source_mib=50,max_entities=512,summary=False,preserve_labels=True):
    max_source_mib=resource_budgets.integer(max_source_mib,'max_source_mib',1,200)
    max_entities=resource_budgets.integer(max_entities,'max_entities',1,4096)
    if type(preserve_labels) is not bool:raise ValueError('preserve_labels must be boolean')
    if not isinstance(summary,bool):raise ValueError('summary must be boolean')
    if not isinstance(preserve_reference_geometry,bool):raise ValueError('preserve_reference_geometry must be boolean')
    target=Path(path).expanduser().resolve();name=native.name(name)
    if target.suffix.lower() not in ('.step','.stp') or not target.is_file():raise ValueError('expected an existing local STEP file')
    if not 1<=target.stat().st_size<=max_source_mib*1024*1024:raise ValueError(f'STEP file exceeds requested {max_source_mib} MiB limit or is empty')
    raw=target.read_bytes();units=declared_units(raw);digest=hashlib.sha256(raw).hexdigest()
    if expected_sha256 is not None and digest!=expected_sha256:raise ValueError('component source hash mismatch')
    offset=native.vector(list(position));direction=native.vector(list(axis))
    if direction.Length<1e-9:raise ValueError('placement axis must be nonzero')
    if isinstance(angle_degrees,bool) or not isinstance(angle_degrees,(int,float)) or not math.isfinite(angle_degrees) or abs(angle_degrees)>360:
        raise ValueError('placement angle must be finite and within +/-360 degrees')
    transform=App.Placement(offset,App.Rotation(direction,angle_degrees))
    shape=Part.read(str(target));entities=_entities(shape,preserve_reference_geometry);solids=[s for s,role in entities if role=='physical_solid'];references=[s for s,role in entities if role=='reference_surface']
    if not 1<=len(solids)<=max_entities or len(entities)>max_entities or len(shape.Faces)>100000:raise ValueError(f'STEP requires at least one solid, at most {max_entities} total entities and 100000 faces')
    placed=shape.copy();placed.Placement=transform.multiply(placed.Placement)
    native.validate_aggregate_leaves(placed)
    placed_entities=_entities(placed,preserve_reference_geometry)
    placed_entities=[s for s,role in placed_entities if role=='physical_solid']+[s for s,role in placed_entities if role=='reference_surface']
    if any(not math.isfinite(s.Volume) or s.Volume<=0 for s in solids) or any(not math.isfinite(s.Area) or s.Area<=0 for s in references):
        raise ValueError('STEP has invalid or empty solid geometry')
    if any(abs(v)>1e6 for v in _signature(shape)[:6]):raise ValueError('STEP coordinates exceed +/-1e6 mm')
    solid_names=[name] if len(solids)==1 and not references else [name+f'_Solid{i+1:03}' for i in range(len(solids))]
    reference_names=[name+f'_Reference{i+1:03}' for i in range(len(references))]
    entities=[(s,'physical_solid') for s in solids]+[(s,'reference_surface') for s in references]
    names=solid_names+reference_names
    if any(len(n)>96 or doc.getObject(n) is not None for n in names) or doc.getObject(name) is not None:
        raise ValueError('import result name collision or prefix too long')
    labels=_labels(target,[s for s,role in entities]) if preserve_labels else [[] for _ in entities]
    provenance=dict(source_path=str(target),source_sha256=digest,source_file_bytes=len(raw),**units,
        resource_budget=dict(max_source_mib=max_source_mib,max_entities=max_entities,max_source_faces=100000),source_entity_count=len(entities),
        source_solid_count=len(solids),source_reference_surface_count=len(references),import_instance=str(uuid.uuid4()),representation='Native BREP snapshots; explicitly retained reference surfaces are not mass/collision solids; source feature history and ECAD connectivity are not imported',
        preserve_labels=preserve_labels,labels_status='matched_when_unambiguous' if preserve_labels else 'not_requested',
        source_associative=False,source_hierarchy='flattened solid instances; matching component labels retained when unambiguous' if preserve_labels else 'flattened solid instances; supplier labels not requested',
        assembly_placement=dict(position_mm=list(position),axis=list(axis),angle_degrees=angle_degrees))
    if component_identity is not None:provenance['component_identity']=component_identity
    doc.openTransaction('Import STEP '+name)
    try:
        created=[]
        for i,((solid,role),n,candidates) in enumerate(zip(entities,names,labels)):
            obj=doc.addObject('Part::Feature',n)
            # The imported BREP is retained, including native holes and surfaces.
            obj.Shape=placed_entities[i].copy()
            if len(candidates)==1:obj.Label=candidates[0]
            evidence=dict(provenance,geometry_role=role,source_entity_index=i+1,source_solid_index=i+1 if role=='physical_solid' else None,source_reference_index=i-len(solids)+1 if role=='reference_surface' else None,source_label_candidates=candidates,
                source_normalized_brep_sha256=hashlib.sha256(solid.cleaned().exportBrepToString().encode()).hexdigest())
            obj.addProperty('App::PropertyString','WrightStepImport','WrightImport');obj.WrightStepImport=json.dumps(evidence,allow_nan=False);obj.setEditorMode('WrightStepImport',1)
            created.append(obj)
        doc.recompute();validated=native.validate_document(doc)
        if hashlib.sha256(target.read_bytes()).hexdigest()!=digest:raise ValueError('STEP source changed while importing')
        result=dict(provenance=provenance,validated_object_count=len(validated),summary=summary,
            object_names=solid_names,reference_object_names=reference_names,all_object_count=len(names),solid_count=len(solids),reference_surface_count=len(references),assembly_contacts_checked=False,
            scope='Imported source solids plus explicitly classified reference surfaces and editable placement; references excluded from physical checks; not recreated envelopes or source parametric design history')
        if summary:
            result['objects_omitted']=True
            result['inspection_hint']='Use inspect summary with pagination, then scoped inspect of needed objects; all geometry was validated'
            if len(json.dumps(result,allow_nan=False).encode())>512*1024:raise ValueError('import summary exceeds 512 KiB; use a shorter object prefix or lower entity budget')
        else:result.update(objects=[native.describe(o) for o in created],all_object_names=names)
        doc.commitTransaction();return result
    except Exception:
        doc.abortTransaction();doc.recompute();raise
