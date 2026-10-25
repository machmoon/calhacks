"""Classification and coverage for explicitly retained STEP reference surfaces."""
import json


def metadata(obj):
    if not hasattr(obj,'WrightStepImport'):return None
    try:source=json.loads(obj.WrightStepImport)
    except (ValueError,TypeError):raise ValueError('invalid STEP import provenance')
    if not isinstance(source,dict):raise ValueError('invalid STEP import provenance')
    return source


def is_reference(obj):
    source=metadata(obj)
    return source is not None and source.get('geometry_role')=='reference_surface'


def validate_reference(obj):
    source=metadata(obj)
    if not source or source.get('geometry_role')!='reference_surface':raise ValueError('unmarked geometry is not an imported reference surface')
    if obj.TypeId!='Part::Feature' or not source.get('import_instance') or len(source.get('source_sha256',''))!=64:
        raise ValueError('invalid imported reference provenance')
    shape=obj.Shape
    if shape.ShapeType not in ('Shell','Face') or shape.Solids or not shape.Faces or shape.Area<=0:
        raise ValueError('reference geometry must remain a non-solid surface')


def coverage(objects):
    """Report excluded surfaces from the same selected import instances."""
    instances=set();documents={};expected={};observed={}
    selected={(o.Document.Name,o.Name) for o in objects if o.Document}
    pending=list(objects);seen=set()
    while pending:
        obj=pending.pop();key=(obj.Document.Name,obj.Name)
        if key in seen:continue
        seen.add(key);pending.extend(getattr(obj,'OutList',[]))
        source=metadata(obj)
        if source and source.get('import_instance'):
            instance=source['import_instance'];instances.add(instance)
            expected[instance]=(source.get('source_solid_count',0),source.get('source_reference_surface_count',0))
        if obj.Document:documents[obj.Document.Name]=obj.Document
    references=[];unselected_solids=[]
    for doc in documents.values():
        for obj in doc.Objects:
            source=metadata(obj)
            if source and source.get('import_instance') in instances:
                instance=source['import_instance'];observed[instance]=observed.get(instance,0)+1
                if is_reference(obj):references.append(obj.Name)
                elif (doc.Name,obj.Name) not in selected:unselected_solids.append(obj.Name)
    missing=sum(max(0,sum(counts)-observed.get(instance,0)) for instance,counts in expected.items())
    source_has_references=bool(references) or any(counts[1]>0 for counts in expected.values())
    return dict(complete_source_geometry_checked=not source_has_references and not unselected_solids and missing==0,
        missing_source_entity_count=missing,source_has_reference_geometry=source_has_references,
        unselected_source_solid_objects=sorted(unselected_solids),
        omitted_reference_objects=sorted(references),reference_surfaces_checked=False,
        scope='Collision and volume checks cover selected solids only; retained reference surfaces are excluded')
