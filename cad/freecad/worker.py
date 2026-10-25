"""Persistent native FreeCAD JSON-RPC worker. Launch with bundled Python.

Supports document, make_part, set_parameter, inspect and save. This is a new
native API, not a drop-in replacement for the older build123d worker contract.
"""
import inspect
import json
import os
from pathlib import Path
import sys
_proto=os.fdopen(os.dup(1),'w',buffering=1)
os.dup2(2,1)
sys.stdout=sys.stderr
sys.path.insert(0,str(Path(__file__).resolve().parent))
import FreeCAD as App
import Part
import native
import layout
import motion
import booleans
import cut_proofs
import mirrors
import spline_conversion
import step_import
import component_library
import import_geometry
import containment
import wing_analysis
import resource_budgets
import render as native_render
from exchange import export_glb, export_step
_doc=None
_last_check=None


def document(name='Wright'):
    global _doc, _last_check
    if _doc is not None: raise ValueError('document already open')
    _doc=App.newDocument(native.name(name))
    _doc.UndoMode=1
    _last_check=None
    return {'name':_doc.Name}


def open_checked(path, defer_source_cut_proofs=False, restore_policy=False):
    if not isinstance(defer_source_cut_proofs,bool):raise ValueError("defer_source_cut_proofs must be boolean")
    target=Path(path).resolve()
    if target.suffix.lower()!='.fcstd' or not target.is_file():raise ValueError('expected an existing FCStd document')
    before=set(App.listDocuments())
    try:
        opened=App.openDocument(str(target))
        if opened is None:raise ValueError('native document could not be reopened')
        opened.UndoMode=1
        marked=cut_proofs.has_marker(opened)
        if marked and not (defer_source_cut_proofs or restore_policy):
            raise ValueError('document has unverified source-cut proofs; reopen with defer_source_cut_proofs:true')
        opened.recompute(None,True,True)
        if defer_source_cut_proofs or (restore_policy and marked):cut_proofs.defer_source(opened)
        native.validate_document(opened)
        return opened
    except Exception:
        for name in set(App.listDocuments())-before:
            cut_proofs.clear(App.getDocument(name));App.closeDocument(name)
        raise

def restore(path):
    global _doc, _last_check
    if _doc is not None:raise ValueError('document already open')
    _doc=open_checked(path,restore_policy=True)
    _last_check=None
    return {'name':_doc.Name,'restored':True,'cut_proofs':cut_proofs.status(_doc)}

def open_document(path, defer_source_cut_proofs=False):
    global _doc, _last_check
    if _doc is not None and _doc.Objects:raise ValueError('open_document requires an empty CAD session; existing edits are preserved')
    previous=_doc
    try:opened=open_checked(path,defer_source_cut_proofs=defer_source_cut_proofs)
    except Exception:
        if previous is not None:App.setActiveDocument(previous.Name)
        raise
    if previous is not None:
        cut_proofs.clear(previous);App.closeDocument(previous.Name)
    _doc=opened;_last_check=None;App.setActiveDocument(opened.Name)
    return {'name':opened.Name,'opened':True,'source':str(Path(path).resolve()),'objects':inspect_tree(),'cut_proofs':cut_proofs.status(opened)}


def checkpoint(directory):
    out=Path(directory).resolve()
    out.mkdir(parents=True,exist_ok=True)
    path=out/'working.FCStd'
    proof_status=cut_proofs.persist_policy(active())
    active().saveAs(str(path))
    return {'checkpoint':str(path),'cut_proofs':proof_status}


def active():
    if _doc is None: raise ValueError('create a document first')
    return _doc


def inspect_tree(objects=None,summary=False,offset=0,limit=100):
    if not isinstance(exhaustive_distances,bool):raise ValueError('exhaustive_distances must be boolean')
    if require_connected and not exhaustive_distances:raise ValueError('Connectivity certification requires exhaustive distances')
    if not isinstance(summary,bool):raise ValueError('summary must be boolean')
    if isinstance(offset,bool) or not isinstance(offset,int) or offset<0:raise ValueError('offset must be a nonnegative integer')
    if isinstance(limit,bool) or not isinstance(limit,int) or not 1<=limit<=100:raise ValueError('limit must be an integer from 1 to 100')
    if not summary and (offset!=0 or limit!=100):raise ValueError('pagination requires summary mode')
    if objects is None:
        selected=[o for o in active().Objects if hasattr(o,'Shape') or layout.is_parameters(o)]
    else:
        if not isinstance(objects,list) or not 1<=len(objects)<=256 or any(not isinstance(n,str) for n in objects):
            raise ValueError('inspect objects must contain 1 to 256 names')
        if len(set(objects))!=len(objects):raise ValueError('duplicate inspection objects')
        selected=[]
        for n in objects:
            obj=active().getObject(native.name(n))
            if obj is None or not (hasattr(obj,'Shape') or layout.is_parameters(obj)):
                raise ValueError('unknown or unsupported inspection object '+n)
            selected.append(obj)
    if summary:
        page=selected[offset:offset+limit]
        return dict(objects=[dict(name=o.Name,label=o.Label,type=o.TypeId,
            dependencies=[d.Name for d in o.OutList],reference_geometry=import_geometry.is_reference(o),
            editable_parameter_names=sorted(native.editable_parameters(o))) for o in page],
            total=len(selected),offset=offset,
            next_offset=offset+len(page) if offset+len(page)<len(selected) else None,
            geometry_checked=False,cut_proofs=cut_proofs.status(active()),
            scope='Object discovery only; inspect named objects and run check for current geometry evidence')
    records=[]
    for obj in selected:
        record=native.describe(obj)
        if 'WrightStepImport' in obj.PropertiesList:record['step_import']=json.loads(obj.WrightStepImport)
        records.append(record)
    return records


def selected_objects(objects,allow_references=False,measure=True):
    if not isinstance(objects,list) or not objects or any(not isinstance(n,str) for n in objects):
        raise ValueError('select a nonempty list of object names')
    if len(set(objects)) != len(objects):raise ValueError('duplicate output objects')
    selected=[]
    for n in objects:
        obj=active().getObject(native.name(n))
        if obj is None:raise ValueError('unknown output object '+n)
        if measure:evidence=native.describe(obj)
        else:
            from assembly_frames import world_shape
            native.validate_live_feature(obj)
            shape=world_shape(obj)
            reference=import_geometry.is_reference(obj)
            if reference:import_geometry.validate_reference(obj)
            if 'Invalid' in obj.State or not native.shape_is_valid(shape) or (not reference and (not shape.Solids or shape.Volume<=0)):
                raise ValueError(f'{obj.Name}: invalid or empty geometry')
            evidence={'assembly_container':native.proxy_type(obj) in {'Rocket.FeatureRocket.FeatureRocket','Rocket.FeatureStage.FeatureStage'},'solids':len(shape.Solids),'reference_geometry':reference}
        if evidence['assembly_container'] or (evidence['solids']<1 and not (allow_references and evidence.get('reference_geometry'))):raise ValueError('profiles, reference surfaces and assembly containers are not physical outputs')
        selected.append(obj)
    def construction_tree(obj):
        pending=[obj];seen=set();retained=set()
        while pending:
            source=pending.pop()
            if source.Name in seen:continue
            if source.TypeId=='Part::Cut' and getattr(source,'WrightRetainToolAsPart',False):
                retained.add(source.Tool.Name)
                if source.Tool.TypeId=='PartDesign::ShapeBinder' and getattr(source.Tool,'WrightFrameSource',None):
                    retained.add(source.Tool.WrightFrameSource.Name)
            seen.add(source.Name);pending.extend(native.geometric_sources(source))
        seen.discard(obj.Name)
        return seen,retained
    for obj in selected:
        for parent in selected:
            if parent==obj:continue
            sources,retained=construction_tree(parent)
            if obj.Name in sources:
                if obj.Name not in retained:
                    raise ValueError('select final physical outputs, not their construction tools or bases')
                from assembly_frames import world_shape
                if world_shape(parent).common(world_shape(obj)).Volume>1e-6:
                    raise ValueError('retained mating tool overlaps final physical part')
    return selected


def place(object_name, position, axis=(0,0,1), angle_degrees=0):
    import math
    obj=active().getObject(native.name(object_name))
    if obj is None:raise ValueError('unknown object')
    if (native.proxy_type(obj) or '').startswith('Rocket.'):
        raise ValueError('Rocket placement is assembly-controlled; use supported AxialOffset edits instead of direct Placement')
    position,axis=native.vector(position),native.vector(axis)
    if axis.Length<=0:raise ValueError('rotation axis cannot be zero')
    if isinstance(angle_degrees,bool) or not isinstance(angle_degrees,(int,float)) or not math.isfinite(angle_degrees):
        raise ValueError('rotation angle must be finite')
    doc=active()
    doc.openTransaction('Place '+object_name)
    try:
        obj.Placement=App.Placement(position,App.Rotation(axis,angle_degrees))
        doc.recompute()
        native.validate_document(doc)
        result=native.describe(obj)
        doc.commitTransaction()
        return result
    except Exception:
        doc.abortTransaction();doc.recompute();raise


def contact_components(objects,pairs,tolerance_mm):
    # Connectivity uses measured, nonpenetrating contacts. A collision must not
    # count as a successful structural attachment.
    neighbors={name:set() for name in objects}
    for pair in pairs:
        if not pair['clash'] and pair['distance_mm'] is not None and pair['distance_mm']<=tolerance_mm:
            neighbors[pair['a']].add(pair['b']);neighbors[pair['b']].add(pair['a'])
    unseen=set(objects);components=[]
    while unseen:
        stack=[min(unseen)];component=[]
        while stack:
            node=stack.pop()
            if node not in unseen:continue
            unseen.remove(node);component.append(node);stack.extend(neighbors[node]&unseen)
        components.append(sorted(component))
    return components


def geometry_fingerprint(objects):
    import hashlib
    digest=hashlib.sha256()
    for obj in sorted(objects,key=lambda obj:obj.Name):
        digest.update(obj.Name.encode()+b'\0')
        from assembly_frames import world_shape
        digest.update(world_shape(obj).cleaned().exportBrepToString().encode())
    return digest.hexdigest()


def current_check(selected):
    if _last_check is None or _last_check[0]!=geometry_fingerprint(selected):
        return {'status':'not_checked'}
    return dict(_last_check[1],status='checked')


def check(objects, contacts=None, tolerance_mm=0.1, require_connected=False, summary=False, exhaustive_distances=True):
    global _last_check
    import math
    selected=selected_objects(objects,measure=False)
    if not isinstance(exhaustive_distances,bool):raise ValueError('exhaustive_distances must be boolean')
    if require_connected and not exhaustive_distances:raise ValueError('Connectivity certification requires exhaustive distances')
    if not isinstance(summary,bool):raise ValueError('summary must be boolean')
    if not isinstance(require_connected,bool):raise ValueError('require_connected must be boolean')
    if isinstance(tolerance_mm,bool) or not isinstance(tolerance_mm,(int,float)) or not math.isfinite(tolerance_mm) or not 0<=tolerance_mm<=1:
        raise ValueError('contact tolerance must be finite and between 0 and 1 mm')
    contacts=[] if contacts is None else contacts
    if not isinstance(contacts,list):raise ValueError('contacts must be pairs of selected names')
    required=set()
    for pair in contacts:
        if not isinstance(pair,list) or len(pair)!=2 or any(n not in objects for n in pair) or pair[0]==pair[1]:
            raise ValueError('contact must name two different selected objects')
        required.add(tuple(sorted(pair)))
    pairs=[]
    from assembly_frames import world_shape
    shapes={o.Name:world_shape(o) for o in selected}
    bounds={name:native.shape_bounds(shape,conservative=True) for name,shape in shapes.items()}
    for i,a in enumerate(selected):
        for b in selected[i+1:]:
            contact=tuple(sorted([a.Name,b.Name])) in required
            ba,bb=bounds[a.Name],bounds[b.Name]
            gaps=[max(0,getattr(ba,k+'Min')-getattr(bb,k+'Max'),getattr(bb,k+'Min')-getattr(ba,k+'Max')) for k in 'XYZ']
            lower=math.sqrt(sum(v*v for v in gaps))
            if lower>tolerance_mm:
                pairs.append(dict(a=a.Name,b=b.Name,overlap_mm3=0,distance_mm=None,
                                  distance_lower_bound_mm=lower,broad_phase=True,
                                  required_contact=contact,clash=False,floating=contact))
                continue
            volume=shapes[a.Name].common(shapes[b.Name]).Volume
            # Positive intersection volume already proves zero separation.
            distance=0.0 if volume>1e-6 else shapes[a.Name].distToShape(shapes[b.Name])[0] if contact or exhaustive_distances else None
            if not math.isfinite(volume) or (distance is not None and not math.isfinite(distance)):raise ValueError('nonfinite geometry evidence')
            pairs.append(dict(a=a.Name,b=b.Name,overlap_mm3=volume,distance_mm=distance,
                              distance_lower_bound_mm=lower,broad_phase=False,
                              required_contact=contact,clash=volume>1e-6,floating=contact and distance>tolerance_mm))
    components=contact_components(objects,pairs,tolerance_mm)
    connected=len(components)==1
    report=dict(passed=all(not p['clash'] and not p['floating'] for p in pairs) and (connected or not require_connected),
                connected=connected if exhaustive_distances else None,components=components,require_connected=require_connected,
                connectivity_evaluated=exhaustive_distances,distance_scope='all close pairs' if exhaustive_distances else 'declared contacts and penetrations only; component list contains measured contacts only',
                scope='Selected solid geometry and explicitly required contacts only; not manufacturing or flight approval',
                objects=list(objects),pairs=pairs,pairs_total=len(pairs),tolerance_mm=tolerance_mm,
                overlap_count=sum(p['clash'] for p in pairs),unmet_contact_count=sum(p['floating'] for p in pairs))
    report['geometry_coverage']=import_geometry.coverage(selected)
    report['cut_proofs']=cut_proofs.status(active())
    report['pair_checks_passed']=report['passed']
    report['geometry_verified']=report['cut_proofs']['geometry_verified']
    report['passed']=report['passed'] and report['geometry_verified']
    import copy
    _last_check=(geometry_fingerprint(selected),copy.deepcopy(report))
    if summary:
        # Compute every pair as usual and retain the full evidence for export.
        # Compact only the response; never omit a clash or declared contact.
        important=[p for p in pairs if p['clash'] or p['required_contact']]
        report.update(pairs=important,pair_details_scope='all_clashes_and_declared_contacts',
                      omitted_pair_count=len(pairs)-len(important),
                      contact_pair_count=sum(not p['clash'] and p['distance_mm'] is not None and
                                             p['distance_mm']<=tolerance_mm for p in pairs))
    return report


def save(directory, objects):
    doc=active()
    cut_proofs.require_complete(doc)
    native.validate_document(doc)
    cut_proofs.persist_policy(doc)
    selected=selected_objects(objects)
    if import_geometry.coverage(selected)['source_has_reference_geometry']:raise ValueError('checked STEP export cannot certify retained reference geometry from this import instance; native checkpoint remains available')
    out=Path(directory).resolve()
    out.mkdir(parents=True,exist_ok=True)
    # Native Visibility persists without a GUI; hide construction solids so
    # opening the editable document shows the same physical selection as export.
    selected_names={obj.Name for obj in selected}
    for obj in doc.Objects:
        if 'Visibility' in obj.PropertiesList:obj.Visibility=obj.Name in selected_names
    doc.saveAs(str(out/'assembly.FCStd'))
    step=out/'assembly.step'
    schema=export_step(selected,step)
    glb=out/'assembly.glb'
    evidence=current_check(selected)
    preview=export_glb(selected,glb,evidence)
    (out/'geometry-check.json').write_text(json.dumps(evidence,indent=2,allow_nan=False))
    return dict(native=str(out/'assembly.FCStd'),step=str(step),glb=str(glb),preview=preview,step_schema=schema,roundtrip_valid=True)


def save_preview(directory,objects,reference_objects=None,max_objects=512):
    max_objects=resource_budgets.integer(max_objects,'max_objects',1,4096)
    selected=selected_objects(objects)
    references=[] if reference_objects is None or reference_objects==[] else selected_objects(reference_objects,allow_references=True)
    if any(not import_geometry.is_reference(o) for o in references):raise ValueError('reference_objects must name classified imported reference surfaces')
    if len(selected)+len(references)>max_objects:raise ValueError('preview object budget exceeded; set explicit max_objects within 4096')
    doc=active();native.validate_document(doc)
    before=geometry_fingerprint(selected+references)
    out=Path(directory).resolve();out.mkdir(parents=True,exist_ok=True)
    selected_names={o.Name for o in selected+references}
    for obj in doc.Objects:
        if 'Visibility' in obj.PropertiesList:obj.Visibility=obj.Name in selected_names
    proof_status=cut_proofs.persist_policy(doc)
    native_path=out/'preview.FCStd';doc.saveAs(str(native_path))
    glb=out/'preview.glb';evidence=current_check(selected)
    preview=export_glb(selected,glb,evidence,reference_objects=references)
    if geometry_fingerprint(selected+references)!=before:raise ValueError('preview export changed source native geometry')
    coverage=import_geometry.coverage(selected+references)
    omitted_references=sorted(set(coverage['omitted_reference_objects'])-{o.Name for o in references})
    status=dict(native=str(native_path),glb=str(glb),preview=preview,preview_only=True,resource_budget=dict(max_objects=max_objects),
        step_export=dict(status='unavailable_in_preview_contract',reason='No checked STEP export was attempted; mixed reference imports remain unsupported by checked save'),
        geometry_check=evidence,geometry_coverage=coverage,cut_proofs=proof_status,geometry_verified=proof_status['geometry_verified'],
        preview_coverage=dict(complete_source_geometry_shown=not omitted_references and not coverage['unselected_source_solid_objects'] and coverage['missing_source_entity_count']==0,
            missing_source_entity_count=coverage['missing_source_entity_count'],
            unrendered_reference_objects=omitted_references,unrendered_source_solids=coverage['unselected_source_solid_objects']),
        scope='Native document plus visualization only; references are not mass/collision solids and no STEP equivalence is claimed')
    (out/'preview-status.json').write_text(json.dumps(status,indent=2,allow_nan=False))
    return status


def edit_summary(doc, object_name, results, edit):
    # Reverse native dependency links are a conservative discovery aid, not a
    # claim that every dependent shape changed or that assembly contacts pass.
    obj=doc.getObject(object_name)
    pending=list(obj.InList);seen={object_name};dependent=[]
    while pending:
        candidate=pending.pop()
        if candidate.Name in seen:continue
        seen.add(candidate.Name);dependent.append(candidate.Name)
        pending.extend(candidate.InList)
    return dict(edit=edit,geometry_validated=True,validated_object_count=len(results),
                dependent_object_names=sorted(dependent),
                dependency_scope='Transitive native reverse links; candidates for scoped inspection, not proof of changed geometry',
                assembly_contacts_checked=False,
                scope='Full native document validation completed; assembly contacts were not checked. Inspect final physical outputs and run check.')


def set_parameter(object_name,parameter,value,summary=False):
    if not isinstance(exhaustive_distances,bool):raise ValueError('exhaustive_distances must be boolean')
    if require_connected and not exhaustive_distances:raise ValueError('Connectivity certification requires exhaustive distances')
    if not isinstance(summary,bool):raise ValueError('summary must be boolean')
    doc=active()
    results=native.set_parameter(doc,object_name,parameter,value)
    if not summary:return results
    return edit_summary(doc,object_name,results,dict(action='set_parameter',
        object_name=object_name,parameter=parameter,requested_value=value,
        value=float(getattr(doc.getObject(object_name),parameter))))


def set_profile(object_name,points,summary=False):
    if not isinstance(exhaustive_distances,bool):raise ValueError('exhaustive_distances must be boolean')
    if require_connected and not exhaustive_distances:raise ValueError('Connectivity certification requires exhaustive distances')
    if not isinstance(summary,bool):raise ValueError('summary must be boolean')
    doc=active()
    results=native.set_profile(doc,object_name,points)
    if not summary:return results
    obj=doc.getObject(object_name)
    return edit_summary(doc,object_name,results,dict(action='set_profile',
        object_name=object_name,submitted_point_count=len(points),
        geometry_count=obj.GeometryCount,
        profile_type='spline' if getattr(obj,'WrightSplineProfile',False) else 'polygon'))


METHODS={'ping':lambda:dict(pid=os.getpid(),backend='FreeCAD',version=App.Version()),
         'document':document,'restore':restore,'open_document':open_document,'checkpoint':checkpoint,'make_part':lambda spec:native.make_part(active(),spec),
         'boolean':lambda name,operation,base,tool,retain_tool_as_part=False:booleans.create(active(),name,operation,base,tool,retain_tool_as_part),
         'mirror':lambda name,source,normal,origin=(0,0,0):mirrors.create(active(),name,source,normal,origin),
         'make_parameters':lambda name,parameters:layout.create(active(),name,parameters),
         'bind_parameter':lambda object_name,parameter,source_object,source_parameter,scale=1,offset_mm=0:layout.bind(active(),object_name,parameter,source_object,source_parameter,scale,offset_mm),
         'bind_parameters':lambda bindings:layout.bind_many(active(),bindings),
         'convert_spline_loft':lambda source,name,split_index,sampled_tolerance_mm:spline_conversion.convert(active(),source,name,split_index,sampled_tolerance_mm),
         'component_catalog':component_library.catalog,
         'analyze_wing':lambda directory,object_name,velocity_m_s=20,alphas_deg=None,altitude_m=0,samples_per_branch=65,symmetric=True:wing_analysis.analyze(active(),directory,object_name,velocity_m_s,alphas_deg,altitude_m,samples_per_branch,symmetric),
         'import_component':lambda component_id,name,position=(0,0,0),axis=(0,0,1),angle_degrees=0,preserve_reference_geometry=False,max_source_mib=50,max_entities=512,summary=False,preserve_labels=True:component_library.import_component(active(),component_id,name,position,axis,angle_degrees,preserve_reference_geometry,max_source_mib,max_entities,summary,preserve_labels),
         'import_step':lambda path,name,position=(0,0,0),axis=(0,0,1),angle_degrees=0,preserve_reference_geometry=False,max_source_mib=50,max_entities=512,summary=False,preserve_labels=True:step_import.import_step(active(),path,name,position,axis,angle_degrees,preserve_reference_geometry=preserve_reference_geometry,max_source_mib=max_source_mib,max_entities=max_entities,summary=summary,preserve_labels=preserve_labels),
         'set_parameter':set_parameter,
         'set_profile':set_profile,
         'check_containment':lambda container,contents,minimum_clearance_mm=0:check_containment(container,contents,minimum_clearance_mm),
         'check_translation':lambda moving,obstacles,translation,samples=11:check_translation(moving,obstacles,translation,samples),
         'inspect':inspect_tree,'place':place,'check':check,'save':save,'save_preview':save_preview}


def check_translation(moving,obstacles,translation,samples=11):
    moving_objects=selected_objects(moving);obstacle_objects=selected_objects(obstacles)
    result=motion.check_translation(moving_objects,obstacle_objects,translation,samples)
    result['geometry_coverage']=import_geometry.coverage(moving_objects+obstacle_objects)
    return result


def check_containment(container,contents,minimum_clearance_mm=0):
    result=containment.check(active(),container,contents,minimum_clearance_mm)
    result['geometry_coverage']=import_geometry.coverage([active().getObject(n) for n in [container]+contents])
    return result


def render(objects,width=640,height=480,azimuth_degrees=-52,elevation_degrees=24,deflection_mm=.5,max_triangles=50000,angular_deflection_radians=.35,reference_objects=None,max_objects=128,max_source_faces=20000):
    max_objects=resource_budgets.integer(max_objects,'max_objects',1,4096)
    max_source_faces=resource_budgets.integer(max_source_faces,'max_source_faces',1,100000)
    if not isinstance(objects,list) or not 1<=len(objects)<=max_objects:raise ValueError('render object budget exceeded')
    selected=selected_objects(objects)
    references=[] if reference_objects is None or reference_objects==[] else selected_objects(reference_objects,allow_references=True)
    if len(selected)+len(references)>max_objects:raise ValueError('render selection including references exceeds requested object budget')
    if any(not import_geometry.is_reference(o) for o in references):raise ValueError('reference_objects must name classified imported reference surfaces')
    result=native_render.selected(selected+references,geometry_fingerprint,width,height,azimuth_degrees,elevation_degrees,deflection_mm,max_triangles,angular_deflection_radians,max_objects=max_objects,max_source_faces=max_source_faces)
    proof_status=cut_proofs.status(active())
    result.update(physical_objects=list(objects),reference_objects=[o.Name for o in references],geometry_coverage=import_geometry.coverage(selected+references),cut_proofs=proof_status,geometry_verified=proof_status['geometry_verified'])
    return result

METHODS['render']=render

# wright_geom::kernel build programs (stateless; own temporary document)
import kernel_program
METHODS['kernel_build']=kernel_program.run


def emit(message):
    _proto.write(json.dumps(message,allow_nan=False)+'\n')


def main():
    emit(dict(jsonrpc='2.0',method='ready',params=dict(pid=os.getpid(),backend='FreeCAD')))
    for line in sys.stdin:
        if not line.strip():continue
        try:
            req=json.loads(line,parse_constant=lambda v: (_ for _ in ()).throw(ValueError('nonfinite JSON')))
        except ValueError as e:
            emit(dict(jsonrpc='2.0',id=None,error=dict(code=-32700,message=str(e))))
            continue
        if not isinstance(req,dict) or req.get('jsonrpc')!='2.0' or not isinstance(req.get('method'),str) or isinstance(req.get('id'),bool) or not isinstance(req.get('id'),(str,int,type(None))):
            emit(dict(jsonrpc='2.0',id=None,error=dict(code=-32600,message='invalid request')))
            continue
        result=dict(jsonrpc='2.0',id=req.get('id'))
        code=-32000
        try:
            if req['method'] not in METHODS:
                code=-32601
                raise ValueError('unknown method')
            fn=METHODS[req['method']]
            params=req.get('params',{})
            code=-32602
            if not isinstance(params,dict):raise ValueError('params must be object')
            inspect.signature(fn).bind(**params)
            code=-32000
            result['result']=fn(**params)
        except Exception as e:
            result['error']=dict(code=code,message=f'{type(e).__name__}: {e}')
        if 'id' in req:emit(result)
    if _doc is not None: App.closeDocument(_doc.Name)

if __name__=='__main__':main()
