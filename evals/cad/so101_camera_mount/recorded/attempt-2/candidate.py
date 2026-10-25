import os
import json
import hashlib
import struct
import FreeCAD as App
import Part

HOLES = [(2.0,2.0),(23.0,2.0),(2.0,14.5),(23.0,14.5)]
EXPECTED_HASH = '7164b2b4b70f08b2a58a00f1df3ff164eb411befdd93761ba788d2af3ea39cc6'
DRAWING_URL = 'https://datasheets.raspberrypi.com/camera/camera-module-3-standard-mechanical-drawing.pdf'
DRAWING_HASH = 'bde27f5ab38dd46aa05713bbb17c6e8acc4cb1d48ff9109df511256f6fbc66ac'

def placement_record(p):
    return {'base':[p.Base.x,p.Base.y,p.Base.z], 'quaternion':list(p.Rotation.Q), 'matrix':list(p.toMatrix().A)}

def typed_value(value):
    if value is None or isinstance(value,(bool,int,float,str)):
        return value
    if isinstance(value,App.Placement):
        return placement_record(value)
    if isinstance(value,App.Vector):
        return [value.x,value.y,value.z]
    if isinstance(value,(tuple,list)):
        return [typed_value(v) for v in value]
    if isinstance(value,dict):
        return {str(k):typed_value(v) for k,v in value.items()}
    if hasattr(value,'Value') and hasattr(value,'Unit'):
        return {'value':float(value.Value),'unit':str(value.Unit)}
    if hasattr(value,'Name') and hasattr(value,'Document'):
        return {'object':value.Name,'document':value.Document.Name}
    raise TypeError('Unsupported explicit control value type: '+type(value).__name__)

def mesh_record(mesh):
    points,facets = mesh.Topology
    digest = hashlib.sha256()
    digest.update(struct.pack('<QQ',len(points),len(facets)))
    for p in points:
        digest.update(struct.pack('<ddd',p.x,p.y,p.z))
    for face in facets:
        digest.update(struct.pack('<Q',len(face)))
        for index in face:
            digest.update(struct.pack('<Q',int(index)))
    return {'sha256':digest.hexdigest(),'points':len(points),'facets':len(facets)}

def control_names(o):
    selected=[]
    joint = 'joint' in (o.Name+' '+o.TypeId).lower()
    exact = {'axis','origin','parent','child','parentlink','childlink','jointtype','jointaxis','jointorigin','jointname','offset','position','velocity','effort','mimic','multiplier','lower','upper','minimum','maximum','min','max','enabled'}
    for p in o.PropertiesList:
        if p.startswith('_'):
            continue
        key=p.lower().replace('_','')
        group=o.getGroupOfProperty(p).lower()
        if 'angle' in key or 'limit' in key or (joint and key in exact) or 'joint' in group:
            if p not in ('Shape','Mesh','Group','Placement','ExpressionEngine','Proxy'):
                selected.append(p)
    return sorted(selected)

def snapshot(o):
    d={'name':o.Name,'type':o.TypeId,'label':o.Label,
       'parents':sorted(p.Name for p in o.InList),
       'expressions':[(str(path),str(expression)) for path,expression in o.ExpressionEngine] if hasattr(o,'ExpressionEngine') else []}
    if hasattr(o,'Placement'):
        d['placement']=placement_record(o.Placement)
        d['global']=placement_record(o.getGlobalPlacement())
    if hasattr(o,'Group'):
        d['children']=[child.Name for child in o.Group]
    if hasattr(o,'Visibility'):
        d['visibility']=bool(o.Visibility)
    if o.ViewObject is not None and hasattr(o.ViewObject,'Visibility'):
        d['view_visibility']=bool(o.ViewObject.Visibility)
    # App::Part.Shape is an authorized descendant aggregate, not source leaf content.
    if o.isDerivedFrom('Mesh::Feature'):
        d['mesh']=mesh_record(o.Mesh)
    if o.isDerivedFrom('Part::Feature') and not o.isDerivedFrom('App::Part'):
        d['brep']=hashlib.sha256(o.Shape.exportBrepToString().encode('utf-8')).hexdigest()
    # Typed, explicitly selected controls; never dump all joint properties.
    # _Part_ShapeCache and other runtime internals are not source controls.
    controls={}
    for p in control_names(o):
        record={'type':o.getTypeIdOfProperty(p),'value':typed_value(getattr(o,p))}
        if record['type']=='App::PropertyEnumeration':
            record['choices']=list(o.getEnumerationsOfProperty(p))
        controls[p]=record
    d['controls']=controls
    return d

def build(input_path, output_dir, checkpoint=None):
    with open(input_path,'rb') as f:
        source_hash=hashlib.sha256(f.read()).hexdigest()
    if source_hash != EXPECTED_HASH:
        raise RuntimeError('Input hash differs from measured source')
    assembly_path=os.path.join(output_dir,'assembly.FCStd')
    manifest_path=os.path.join(output_dir,'camera-mount-manifest.json')
    if os.path.realpath(assembly_path)==os.path.realpath(input_path):
        raise RuntimeError('Output must not replace source')
    # Refuse to overwrite artifacts, including any retained failed attempt.
    if os.path.exists(assembly_path) or os.path.exists(manifest_path):
        raise RuntimeError('Use a separate empty attempt output directory; prior artifacts must be preserved')
    doc=App.openDocument(input_path)
    original=list(doc.Objects)
    original_names=[o.Name for o in original]
    link=doc.getObject('Link_gripper_link')
    visual=doc.getObject('Visual_gripper_link_1')
    if link is None or visual is None or not hasattr(link,'addObject'):
        raise RuntimeError('Expected moving wrist container and source visual missing')
    before={o.Name:snapshot(o) for o in original}

    def stage(label):
        doc.recompute()
        if checkpoint is not None:
            checkpoint(label)

    def assert_preserved(root_name=None):
        differences={}
        for name in original_names:
            current=doc.getObject(name)
            if current is None:
                differences[name]={'missing':True}
                continue
            expected=dict(before[name])
            if name==link.Name and root_name is not None:
                expected['children']=expected['children']+[root_name]
            actual=snapshot(current)
            delta={k:{'before':expected.get(k),'after':actual.get(k)} for k in set(expected)|set(actual) if expected.get(k)!=actual.get(k)}
            if delta:
                differences[name]=delta
        if differences:
            raise RuntimeError('Original source preservation failed: '+json.dumps(differences,sort_keys=True))
        return differences

    stage('Opened original robot assembly')
    assert_preserved()
    for name in ['CameraMount','CameraMountAssembly','CameraPlateFrame','CameraFrame','CameraMountParameters','CameraPlate']+['CameraStandoff'+str(i) for i in range(4)]:
        if doc.getObject(name):
            raise RuntimeError('Reserved object already exists: '+name)
    params=doc.addObject('App::DocumentObjectGroup','CameraMountParameters')
    params.addProperty('App::PropertyLength','PlateThickness','Dimensions')
    params.addProperty('App::PropertyLength','StandoffHeight','Dimensions')
    params.PlateThickness=3
    params.StandoffHeight=6
    params.addProperty('App::PropertyString','DesignNotes')
    params.DesignNotes='Prototype T=3 mm and H=6 mm; H must remain >=5 mm. Through-bolt construction; no modeled threads. Hardware lengths require review after edits.'
    root=doc.addObject('App::Part','CameraMountAssembly')
    link.addObject(root)
    attachment=App.Placement(App.Vector(-0.95,-24.2182144793536,24.3498679609405),App.Rotation(App.Vector(1,0,0),App.Vector(0,0,1),App.Vector(0,-1,0),'ZXY'))
    root.Placement=visual.Placement.multiply(attachment)
    frame=doc.addObject('App::Part','CameraPlateFrame')
    root.addObject(frame)
    frame.Placement.Base=App.Vector(-12.5,12,0)
    camera=doc.addObject('App::Part','CameraFrame')
    frame.addObject(camera)
    camera.setExpression('Placement.Base.z','CameraMountParameters.PlateThickness + CameraMountParameters.StandoffHeight')
    camera.addProperty('App::PropertyString','CoordinateConvention')
    camera.CoordinateConvention='PCB rear-face lower-left origin; +X right, +Y up, +Z optical direction'
    physical=[]

    def obj(kind,name,container=frame):
        if doc.getObject(name):
            raise RuntimeError('Object name already present: '+name)
        o=doc.addObject(kind,name)
        container.addObject(o)
        if container==frame:
            physical.append(o)
        return o

    def box(name,x,y,z,l,w,h,container=frame):
        o=obj('Part::Box',name,container)
        o.Length=l
        o.Width=w
        o.Height=h
        o.Placement.Base=App.Vector(x,y,z)
        return o

    def cyl(name,x,y,z,r,h,container=frame):
        o=obj('Part::Cylinder',name,container)
        o.Radius=r
        o.Height=h
        o.Placement.Base=App.Vector(x,y,z)
        return o

    def cut(name,a,b,container=frame):
        o=obj('Part::Cut',name,container)
        o.Base=a
        o.Tool=b
        return o

    def fuse(name,items,container=frame):
        o=obj('Part::MultiFuse',name,container)
        o.Shapes=items
        o.Refine=True
        return o

    def thickness(o):
        o.setExpression('Height','CameraMountParameters.PlateThickness')

    foot=box('WristFootBlank',5.5,-16,0,14,8,3)
    thickness(foot)
    stem=box('CameraSupportWeb',10,-10,0,5,12,3)
    thickness(stem)
    base=fuse('WristSupportBlank',[foot,stem])
    wrist_tools=[]
    for i,x in enumerate([8.45,16.55]):
        tool=cyl('WristBoreTool'+str(i),x,-12,-1,1.7,5)
        tool.setExpression('Height','CameraMountParameters.PlateThickness + 2 mm')
        wrist_tools.append(tool)
    wrist=cut('WristInterface',base,fuse('WristBoreTools',wrist_tools))
    stage('Measured two-bore wrist interface and connecting web')
    blank=box('CameraPlateBlank',-1,-1,0,27,25.862,3)
    thickness(blank)
    plate_tools=[]
    bosses=[]
    for i,(x,y) in enumerate(HOLES):
        tool=cyl('CameraPlateHoleTool'+str(i),x,y,-1,1.1,5)
        tool.setExpression('Height','CameraMountParameters.PlateThickness + 2 mm')
        plate_tools.append(tool)
        boss=cyl('CameraBossBlank'+str(i),x,y,3,2.5,6)
        boss.setExpression('Placement.Base.z','CameraMountParameters.PlateThickness')
        boss.setExpression('Height','CameraMountParameters.StandoffHeight')
        bore=cyl('CameraBossBore'+str(i),x,y,2,1.1,8)
        bore.setExpression('Placement.Base.z','CameraMountParameters.PlateThickness - 1 mm')
        bore.setExpression('Height','CameraMountParameters.StandoffHeight + 2 mm')
        bosses.append(cut('CameraStandoff'+str(i),boss,bore))
    plate=cut('CameraPlate',blank,fuse('CameraPlateHoleTools',plate_tools))
    stage('Camera plate with final clearance holes and four drilled standoffs')
    mount=fuse('CameraMount',[wrist,plate]+bosses)
    stage('Connected camera support solid')
    board=box('CameraBoardEnvelopeBlank',0,0,0,25,23.862,1.12,camera)
    pcbtools=[cyl('ReferencePCBHole'+str(i),x,y,-0.1,1.1,1.32,camera) for i,(x,y) in enumerate(HOLES)]
    pcb=cut('CameraPCBReference',board,fuse('ReferencePCBTools',pcbtools,camera),camera)
    front=box('CameraFrontConservativeEnvelope',5,3,1.12,15,18,10.18,camera)
    rear=box('CameraRearComponentReservation',5,0,-5,15,23.862,5,camera)
    envelope=fuse('CameraReferenceEnvelope',[pcb,front,rear],camera)
    envelope.addProperty('App::PropertyString','CameraModel')
    envelope.CameraModel='Raspberry Pi Camera Module 3 Standard (not Wide or NoIR)'
    envelope.addProperty('App::PropertyString','DimensionalSource')
    envelope.DimensionalSource=DRAWING_URL+' ; SHA256 '+DRAWING_HASH
    envelope.addProperty('App::PropertyString','Simplification')
    envelope.Simplification='Reference only, not manufacturer CAD. Drawing PCB dimensions and total depth; assumed central front box and 5 mm central rear component reservation. Exact component footprints unavailable.'
    ribbon=box('CameraRibbonClearance',3.5,23.862,-3,18,15,2,camera)
    ribbon.addProperty('App::PropertyString','Assumption')
    ribbon.Assumption='Reference-only chosen +Y edge exit, 18 mm width x 2 mm thickness x 15 mm length. Actual connector edge and bend radius unverified.'
    optical=cyl('CameraOpticalClearance',12.5,14.4,11.3,2.875,30,camera)
    optical.addProperty('App::PropertyString','Assumption')
    optical.Assumption='Simplified axial lens continuation only; not a field-of-view or focus guarantee.'
    stage('Moving camera and ribbon reference envelopes')

    def in_plate(o):
        s=o.Shape.copy()
        s.Placement=frame.getGlobalPlacement().inverse().multiply(o.getGlobalPlacement())
        return s

    def validate_geometry():
        overlaps={o.Name:mount.Shape.common(in_plate(o)).Volume for o in [envelope,ribbon,optical]}
        containment={o.Name:o.Shape.cut(mount.Shape).Volume for o in [plate]+bosses}
        measured={'mount_valid':mount.Shape.isValid(),'solid_count':len(mount.Shape.Solids),'volume_mm3':mount.Shape.Volume,'reference_overlap_mm3':overlaps,'construction_outside_final_mm3':containment,
                  'construction_validity':{o.Name:o.Shape.isValid() for o in [wrist,plate]+bosses},
                  'plate_z_bounds_mm':[plate.Shape.BoundBox.ZMin,plate.Shape.BoundBox.ZMax],
                  'standoff_z_bounds_mm':{o.Name:[o.Shape.BoundBox.ZMin,o.Shape.BoundBox.ZMax] for o in bosses},
                  'camera_frame_local':placement_record(camera.Placement)}
        if not measured['mount_valid'] or measured['solid_count']!=1 or measured['volume_mm3']<=0:
            raise RuntimeError('Mount is not a valid single solid: '+json.dumps(measured))
        if not all(measured['construction_validity'].values()):
            raise RuntimeError('Invalid construction BRep')
        if any(v>1e-7 for v in overlaps.values()) or any(v>1e-7 for v in containment.values()):
            raise RuntimeError('Reference clearance or feature containment failed: '+json.dumps(measured))
        t=params.PlateThickness.Value
        h=params.StandoffHeight.Value
        if h<5:
            raise RuntimeError('Prototype standoff must be at least 5 mm')
        actual=[plate.Shape.BoundBox.ZMin,plate.Shape.BoundBox.ZMax,camera.Placement.Base.z]
        target=[0,t,t+h]
        for boss in bosses:
            actual.extend([boss.Shape.BoundBox.ZMin,boss.Shape.BoundBox.ZMax])
            target.extend([t,t+h])
        if any(abs(a-b)>1e-7 for a,b in zip(actual,target)):
            raise RuntimeError('Plate/camera/standoff dimensional contract failed')
        return measured

    validation=validate_geometry()
    assert_preserved(root.Name)
    for o in physical:
        o.Visibility=False
    for o in camera.Group:
        o.Visibility=False
    mount.Visibility=True
    if mount.ViewObject is not None:
        mount.ViewObject.ShapeColor=(0.25,0.45,0.75)
    for o in [envelope,ribbon,optical]:
        o.Visibility=True
        if o.ViewObject is not None:
            o.ViewObject.Transparency=85
    if envelope.ViewObject is not None:
        envelope.ViewObject.ShapeColor=(0.2,0.75,0.3)
        ribbon.ViewObject.ShapeColor=(1.0,0.65,0.15)
    stage('Final native solid and measured reference clearance checks')
    validation=validate_geometry()
    validation['original_objects_changed_except_added_child']=assert_preserved(root.Name)
    validation['original_visibility_preserved']=True
    validation['preservation_scope']={
        'mesh_objects':[o.Name for o in original if o.isDerivedFrom('Mesh::Feature')],
        'brep_objects':[o.Name for o in original if o.isDerivedFrom('Part::Feature') and not o.isDerivedFrom('App::Part')],
        'controls':{name:before[name]['controls'] for name in original_names if before[name]['controls']},
        'compared':'Exact original names, types, labels, parent references, ordered Group memberships, local/global placements, expressions, visibility, typed Angle/limit and joint controls, mesh coordinates/topology, and Part::Feature BRep payloads.',
        'only_membership_exception':'Append CameraMountAssembly to Link_gripper_link.Group',
        'excluded':'App::Part computed descendant Shape aggregates and transient runtime cache properties, including _Part_ShapeCache; no original physical geometry exemption.'}
    validation['fresh_process_parameter_test']='Not executed by this module. Native expressions are persisted for independent host reopen/edit tests.'
    validation['robot_collision_check']='Unresolved: original visuals include meshes; no exact BRep obstacle evaluation, installed hardware clearance, or whole-motion certification performed.'
    os.makedirs(output_dir,exist_ok=True)
    doc.recompute()
    assert_preserved(root.Name)
    doc.saveAs(assembly_path)
    assert_preserved(root.Name)
    with open(input_path,'rb') as f:
        final_source_hash=hashlib.sha256(f.read()).hexdigest()
    if final_source_hash!=source_hash:
        raise RuntimeError('Input file changed during build')
    validation['input_file_hash_unchanged']=True
    manifest={
        'attempt':'attempt2',
        'prior_attempt':{'attempt':'attempt1','status':'failed','candidate_sha256':'5970b8e6ca6769e2ed7a36304a90370720a103e29147ff3c3fb3c738084065b4','failure':'Source-preservation assertion included computed App::Part aggregates and transient _Part_ShapeCache representations.','evidence':'User-supplied host trace reports no original leaf geometry, placement, expression or joint-control change; only authorized mount child appended.','retention':'This module neither loads nor modifies prior attempt artifacts and refuses to overwrite existing output artifacts.'},
        'camera_identity':envelope.CameraModel,
        'dimensional_source':{'url':DRAWING_URL,'sha256':DRAWING_HASH,'scope':'Supplied manufacturer mechanical drawing dimensions; reference envelopes are simplified design assumptions, not imported CAD'},
        'camera_coordinate_convention':camera.CoordinateConvention,
        'input_sha256':source_hash,
        'final_mount_name':mount.Name,
        'new_objects':[o.Name for o in doc.Objects if o.Name not in original_names],
        'measured_wrist_frame':{'parent':link.Name,'visual':visual.Name,'visual_local_matrix':list(visual.Placement.toMatrix().A),'attachment_part_matrix':list(attachment.toMatrix().A),'attachment_in_link_matrix':list(root.Placement.toMatrix().A),'rule':'Parent to moving Link_gripper_link; compose actual source visual local placement with measured U,V,N part frame. No fixed world attachment.','bores_part_mm':[[-5,-24.2182144793536,24.3498679609405],[3.1,-24.2182144793536,24.3498679609405]],'spacing_mm':8.1,'measured_diameter_mm':3.2,'wall_depth_mm':3.5,'nut_recess_AF_mm':5.6,'nut_recess_depth_mm':3.3,'bearing_note':'14 x 8 mm foot is a design choice. Measured 14 x 8 bearing area 85.77160705164283 mm2 versus 95.91504561362026 mm2 rectangle-minus-bores; right-hole bearing is partial, not a fully supported rectangle.'},
        'native_parameter_bindings':{o.Name:[(str(a),str(b)) for a,b in o.ExpressionEngine] for o in doc.Objects if o.Name not in original_names and hasattr(o,'ExpressionEngine') and o.ExpressionEngine},
        'source_verified_camera_dimensions':{'board_mm':[25,23.862,1.12],'overall_depth_mm':11.3,'holes_board_xy_mm':HOLES,'hole_diameter_mm':2.2,'lens_center_xy_mm':[12.5,14.4],'lens_barrel_diameter_mm':5.75},
        'design_choices':{'PlateThickness_mm':3,'StandoffHeight_mm':6,'minimum_standoff_mm':5,'plate_origin_in_attachment_mm':[-12.5,12,0],'plate_xy_bounds_mm':[-1,-1,26,24.862],'boss_radius_mm':2.5,'camera_clearance_diameter_mm':2.2,'wrist_clearance_diameter_mm':3.4,'rear_reservation':'5 mm behind PCB central area x=5..20; corner mounting lands excluded. H=6 leaves 1 mm between this rear reservation and plate. Exact rear footprint is unknown.','orientation':'Camera optical +Z equals measured outward normal','dimensional_contract':'Plate z=0..T; drilled bosses z=T..T+H; CameraFrame rear PCB face z=T+H, with matching XY axes.'},
        'hardware_assumptions':{
            'camera':{'screw_count':4,'nut_count':4,'nominal_thread':'M2','proposed_screw_length_mm':14,'clearance_bore_mm':2.2,'grip_formula':'T + H + 1.12 mm PCB','actual_grip_mm':params.PlateThickness.Value+params.StandoffHeight.Value+1.12,'assumed_nut_thickness_mm':1.6,'projection_after_assumed_nut_mm':14-(params.PlateThickness.Value+params.StandoffHeight.Value+1.12+1.6),'access':'Heads at front PCB mounting lands; nuts below plate. No engagement, actual head/component clearance, tool access or screw-tip clearance is verified. Insulating washers excluded from stack.'},
            'wrist':{'screw_count':2,'nut_count':2,'nominal_thread':'M3','proposed_screw_length_mm':10,'clearance_bore_mm':3.4,'source_bore_mm':3.2,'grip_formula':'T + 3.5 mm source wall','actual_grip_mm':params.PlateThickness.Value+3.5,'assumed_nut_thickness_mm':2.4,'projection_after_assumed_nut_mm':10-(params.PlateThickness.Value+3.5+2.4),'access':'Supplied official guide requires motor6 removal to insert nuts and then motor reattachment. Remaining motor/screw clearance unverified; right bore bearing is partial.'},
            'note':'Through hardware only; no fake threads, claimed engagement or modeled physical fasteners. Recalculate proposed lengths after parameter edits. Hardware is not part of CameraMount.'},
        'cable_keepout':{'name':ribbon.Name,'reference_only':True,'frame':'CameraFrame','bounds_mm':[3.5,23.862,-3,21.5,38.862,-1],'width_mm':18,'thickness_mm':2,'length_mm':15,'chosen_exit':'+Y','actual_connector_geometry':'Cannot be established from supplied dimensions; connector side and bend radius require physical verification.'},
        'validation':validation,
        'limitations':['Prototype only; no manufacturing or strength certification.','Exact rear components and connector geometry unavailable; central component envelopes are assumptions.','Full optical field of view and complete robot motion clearance remain unverified.','No mesh-to-solid conversion. Original source file and prior attempt artifacts are not modified.','Native expressions require independent fresh-process host testing; independent evaluation is not claimed.','H >= 5 mm is a declared design constraint, checked during build but not enforced by unrestricted PropertyLength after edits.','Hardware access and motor clearance require verification before installation.']
    }
    with open(manifest_path,'w',encoding='utf-8') as f:
        json.dump(manifest,f,indent=2,sort_keys=True)
    return {'assembly':assembly_path,'manifest':manifest_path,'outcome':'Saved attempt2 native editable camera mount with actual build-time BRep, clearance and typed preservation results in manifest. Prior attempt remains failed. Fresh-process parameter testing, robot motion clearance and hardware access remain for independent verification.'}
