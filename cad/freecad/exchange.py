"""Use FreeCAD's Open CASCADE exporter and verify nonempty preview geometry."""
import json
import struct
import FreeCAD as App
import Import
import Part
import re
import hashlib


def export_glb(objects,path,geometry_check=None,reference_objects=None):
    import import_geometry
    physical=list(objects);references=list(reference_objects or [])
    if any(import_geometry.is_reference(o) for o in physical):raise ValueError('reference surfaces must be explicitly selected as references')
    for obj in references:import_geometry.validate_reference(obj)
    objects=physical+references
    if len({o.Name for o in objects})!=len(objects):raise ValueError('duplicate preview objects')
    # The native headless exporter loses triangulation on placed shapes. Bake
    # their world placement into temporary display copies, never into the
    # editable source feature tree, then tessellate those copies.
    original=App.ActiveDocument
    preview=App.newDocument('WrightPreviewExport')
    try:
        copies=[]
        for obj in objects:
            from assembly_frames import world_shape
            shape=world_shape(obj)
            placement=shape.Placement
            shape.Placement=App.Placement()
            shape=shape.transformGeometry(placement.toMatrix())
            feature=preview.addObject('Part::Feature',obj.Name)
            feature.Label=obj.Name
            feature.Shape=shape
            feature.Shape.tessellate(0.1)
            copies.append(feature)
        Import.export(copies,str(path),keepPlacement=True)
    finally:
        App.closeDocument(preview.Name)
        if original is not None:App.setActiveDocument(original.Name)
    data=path.read_bytes()
    magic,version,total=struct.unpack_from('<4sII',data,0)
    if magic!=b'glTF' or version!=2 or total!=len(data):raise ValueError('invalid GLB header')
    length,kind=struct.unpack_from('<II',data,12)
    if kind!=0x4e4f534a:raise ValueError('GLB JSON chunk missing')
    document=json.loads(data[20:20+length])
    nodes=document.get('nodes',[])
    meshes=document.get('meshes',[])
    physical_nodes=[node for node in nodes if 'mesh' in node]
    names={node.get('name') for node in physical_nodes}
    if len(physical_nodes)!=len(objects) or names!={o.Name for o in objects}:raise ValueError('GLB omitted selected geometry')
    if any(not 0<=node['mesh']<len(meshes) for node in physical_nodes):raise ValueError('GLB has invalid mesh references')
    for mesh in meshes:
        primitives=mesh.get('primitives',[])
        if not primitives:raise ValueError('empty GLB mesh')
        for primitive in primitives:
            accessor=document['accessors'][primitive['attributes']['POSITION']]
            if accessor['count']<3:raise ValueError('GLB has no triangles')
    # OCCT exports metres in glTF's Y-up coordinates. Explicit metadata prevents
    # the desktop's legacy extent heuristic from mis-scaling small/large parts.
    import native
    by_name={obj.Name:obj for obj in objects}
    for node in physical_nodes:
        obj=by_name[node['name']]
        bindings=native.parameter_bindings(obj)
        reference=import_geometry.is_reference(obj)
        role='reference_surface' if reference else 'physical_solid'
        metadata=dict(native_object=obj.Name,label=obj.Label,native_type=obj.TypeId,backend='FreeCAD',
                      params={b['label']:b['value'] for b in bindings},parameter_bindings=bindings,
                      interferes_with=[],reference_geometry=reference,geometry_role=role)
        metadata['params'].update(geometry_role=role,excluded_from_mass_and_collision=reference)
        source=import_geometry.metadata(obj)
        if source:metadata['step_import']=source
        if reference:metadata['label']=obj.Label+' [reference surface; not physical]'
        if geometry_check is not None and geometry_check.get('status')=='checked':
            metadata['interferes_with']=sorted({p['b'] if p['a']==obj.Name else p['a'] for p in geometry_check.get('pairs',[]) if p['clash'] and obj.Name in (p['a'],p['b'])})
        node['extras']={**node.get('extras',{}),**metadata}
    summary={k:v for k,v in (geometry_check or {'status':'not_checked'}).items() if k!='pairs'}
    document['asset']['extras']={'wright_units':'m','wright_up_axis':'Y','source_backend':'FreeCAD','geometry_check':summary,'physical_nodes':len(physical),'reference_nodes':len(references),'scene_nodes':len(objects),
        'reference_objects':[o.Name for o in references],'geometry_coverage':import_geometry.coverage(objects)}
    encoded=json.dumps(document,separators=(',',':'),allow_nan=False).encode()
    encoded+=b' '*((-len(encoded))%4)
    rest=data[20+length:]
    path.write_bytes(struct.pack('<4sII',b'glTF',2,20+len(encoded)+len(rest))+struct.pack('<II',len(encoded),0x4e4f534a)+encoded+rest)
    return dict(meshes=len(meshes),physical_nodes=len(physical),reference_nodes=len(references),scene_nodes=len(objects),units='m',up_axis='Y')


def export_step(objects,path):
    import import_geometry
    if any(import_geometry.is_reference(o) for o in objects) or import_geometry.coverage(objects)['source_has_reference_geometry']:
        raise ValueError('checked STEP export cannot certify retained reference geometry from this import instance')
    validation_path=path.with_suffix(path.suffix+'.validation.json')
    # Never leave a previous success report beside a newly failed export.
    validation_path.unlink(missing_ok=True)
    Part.setStaticValue('write.step.schema','AP242DIS')
    # Workbench features can carry Group links. Exporting them directly may
    # recursively include children in addition to explicitly selected parts.
    # Plain display copies preserve the selected B-reps and names exactly.
    original=App.ActiveDocument
    preview=App.newDocument('WrightStepExport')
    try:
        copies=[]
        for obj in objects:
            feature=preview.addObject('Part::Feature',obj.Name)
            from assembly_frames import world_shape
            feature.Label=obj.Label;feature.Shape=world_shape(obj);copies.append(feature)
        Import.export(copies,str(path),keepPlacement=True)
    finally:
        App.closeDocument(preview.Name)
        if original is not None:App.setActiveDocument(original.Name)
    schema=re.search(r'FILE_SCHEMA\s*\((.*?)\);',path.read_text(),re.S)
    if schema is None or 'AP242' not in schema.group(1):
        raise ValueError('STEP exporter did not write requested AP242 schema')
    imported=Part.read(str(path))
    from assembly_frames import world_shape
    expected=Part.makeCompound([world_shape(o) for o in objects])
    if not imported.isValid() or len(imported.Solids)!=len(expected.Solids):
        raise ValueError('STEP reimport lost or invalidated solids')
    volume_delta=abs(imported.Volume-expected.Volume)
    verification=dict(expected_volume_mm3=expected.Volume,imported_volume_mm3=imported.Volume,
                      volume_delta_mm3=volume_delta,method='volume and bounds',solid_count=len(expected.Solids))
    # Matching aggregate volume and bounds can hide moved bores, swapped parts
    # or compensating additions/removals. Always prove one-to-one geometry.
    import native
    candidates=list(imported.Solids)
    matched_solids=[]
    keys=['XMin','YMin','ZMin','XMax','YMax','ZMax']
    candidate_bounds=[native.shape_bounds(target) for target in candidates]
    for source_index,source in enumerate(expected.Solids):
        matched=None
        source_bounds=native.shape_bounds(source)
        for index,target in enumerate(candidates):
            if any(abs(getattr(source_bounds,k)-getattr(candidate_bounds[index],k))>1e-5 for k in keys):continue
            removed=source.cut(target);added=target.cut(source)
            if not removed.Solids and not added.Solids:
                matched=index;break
        if matched is None:raise ValueError('STEP solid geometry equivalence failed')
        matched_solids.append(dict(source_solid_index=source_index,empty_removed_solid=True,empty_added_solid=True))
        candidates.pop(matched);candidate_bounds.pop(matched)
    verification['method']='one-to-one solids with empty bidirectional differences'
    verification['solid_matches']=matched_solids
    verification['volume_integral_disagrees']=volume_delta>max(1e-5,expected.Volume*1e-8)
    verification['step_sha256']=hashlib.sha256(path.read_bytes()).hexdigest()
    validation_path.write_text(json.dumps(verification,indent=2,allow_nan=False))
    return 'AP242'
