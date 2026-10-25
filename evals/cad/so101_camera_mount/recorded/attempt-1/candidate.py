import os
import json
import hashlib
import FreeCAD as App
import Part

HOLES = [(2.0,2.0),(23.0,2.0),(2.0,14.5),(23.0,14.5)]
EXPECTED_HASH = '7164b2b4b70f08b2a58a00f1df3ff164eb411befdd93761ba788d2af3ea39cc6'

def build(input_path, output_dir, checkpoint=None):
    with open(input_path, 'rb') as f:
        source_hash = hashlib.sha256(f.read()).hexdigest()
    if source_hash != EXPECTED_HASH:
        raise RuntimeError('Input hash differs from measured source')
    doc = App.openDocument(input_path)
    original = list(doc.Objects)
    original_names = [o.Name for o in original]
    link = doc.getObject('Link_gripper_link')
    visual = doc.getObject('Visual_gripper_link_1')
    if link is None or visual is None or not hasattr(link, 'addObject'):
        raise RuntimeError('Expected moving wrist container and source visual missing')

    def snapshot(o):
        d = {'label':o.Label, 'expressions':str(o.ExpressionEngine) if hasattr(o,'ExpressionEngine') else '', 'parents':sorted(p.Name for p in o.InList)}
        if hasattr(o,'Placement'):
            d['placement'] = list(o.Placement.toMatrix().A)
            d['global'] = list(o.getGlobalPlacement().toMatrix().A)
        if hasattr(o,'Shape'):
            d['brep'] = hashlib.sha256(o.Shape.exportBrepToString().encode()).hexdigest()
        if hasattr(o,'Mesh'):
            d['mesh'] = hashlib.sha256(str(o.Mesh.Topology).encode()).hexdigest()
        if hasattr(o,'Group'):
            d['children'] = [p.Name for p in o.Group]
        if 'joint' in (o.Name+' '+o.TypeId).lower():
            d['joint'] = {p:str(getattr(o,p)) for p in o.PropertiesList if p not in ['Shape','Mesh','Proxy','Group']}
        return d

    before = {o.Name:snapshot(o) for o in original}
    def stage(label):
        doc.recompute()
        if checkpoint is not None:
            checkpoint(label)
    stage('Opened original robot assembly')
    for name in ['CameraMount','CameraPlateFrame','CameraFrame','CameraMountParameters']:
        if doc.getObject(name):
            raise RuntimeError('Reserved object already exists: '+name)
    params = doc.addObject('App::DocumentObjectGroup','CameraMountParameters')
    params.addProperty('App::PropertyLength','PlateThickness','Dimensions')
    params.addProperty('App::PropertyLength','StandoffHeight','Dimensions')
    params.PlateThickness = 3
    params.StandoffHeight = 6
    params.addProperty('App::PropertyString','DesignNotes')
    params.DesignNotes = 'Prototype: T=3 mm, H=6 mm; H must remain >=5 mm. Nominal through-bolt construction; no threads modeled.'
    root = doc.addObject('App::Part','CameraMountAssembly')
    link.addObject(root)
    attachment = App.Placement(App.Vector(-0.95,-24.2182144793536,24.3498679609405),App.Rotation(App.Vector(1,0,0),App.Vector(0,0,1),App.Vector(0,-1,0),'ZXY'))
    root.Placement = visual.Placement.multiply(attachment)
    frame = doc.addObject('App::Part','CameraPlateFrame')
    root.addObject(frame)
    frame.Placement.Base = App.Vector(-12.5,12,0)
    camera = doc.addObject('App::Part','CameraFrame')
    frame.addObject(camera)
    camera.setExpression('Placement.Base.z','CameraMountParameters.PlateThickness + CameraMountParameters.StandoffHeight')
    camera.addProperty('App::PropertyString','CoordinateConvention')
    camera.CoordinateConvention = 'PCB rear-face lower-left origin; +X right, +Y up, +Z optical direction'
    physical = []
    def obj(kind,name,container=frame):
        o = doc.addObject(kind,name)
        container.addObject(o)
        if container == frame:
            physical.append(o)
        return o
    def box(name,x,y,z,l,w,h,container=frame):
        o = obj('Part::Box',name,container)
        o.Length=l; o.Width=w; o.Height=h
        o.Placement.Base=App.Vector(x,y,z)
        return o
    def cyl(name,x,y,z,r,h,container=frame):
        o=obj('Part::Cylinder',name,container)
        o.Radius=r; o.Height=h
        o.Placement.Base=App.Vector(x,y,z)
        return o
    def cut(name,a,b,container=frame):
        o=obj('Part::Cut',name,container)
        o.Base=a; o.Tool=b
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
    wt=fuse('WristBoreTools',wrist_tools)
    wrist=cut('WristInterface',base,wt)
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
    envelope.DimensionalSource='https://datasheets.raspberrypi.com/camera/camera-module-3-standard-mechanical-drawing.pdf ; SHA256 bde27f5ab38dd46aa05713bbb17c6e8acc4cb1d48ff9109df511256f6fbc66ac'
    envelope.addProperty('App::PropertyString','Simplification')
    envelope.Simplification='Reference only, not manufacturer CAD. Drawing PCB dimensions and total depth; conservative assumed central front box and 5 mm central rear reservation. Exact component footprints unavailable.'
    ribbon=box('CameraRibbonClearance',3.5,23.862,-3,18,15,2,camera)
    ribbon.addProperty('App::PropertyString','Assumption')
    ribbon.Assumption='Reference-only chosen +Y edge exit, 18 mm width x 2 mm thickness x 15 mm length; actual connector edge and bend radius unverified.'
    optical=cyl('CameraOpticalClearance',12.5,14.4,11.3,2.875,30,camera)
    optical.addProperty('App::PropertyString','Assumption')
    optical.Assumption='Axial lens continuation only; not a field-of-view or focus guarantee.'
    stage('Moving camera and ribbon reference envelopes')

    def in_plate(o):
        s=o.Shape.copy()
        s.Placement=frame.getGlobalPlacement().inverse().multiply(o.getGlobalPlacement())
        return s
    overlaps={}
    for o in [envelope,ribbon,optical]:
        overlaps[o.Name]=mount.Shape.common(in_plate(o)).Volume
    containment={o.Name:o.Shape.cut(mount.Shape).Volume for o in [plate]+bosses}
    validation={'mount_valid':mount.Shape.isValid(),'solid_count':len(mount.Shape.Solids),'volume_mm3':mount.Shape.Volume,'reference_overlap_mm3':overlaps,'construction_outside_final_mm3':containment}
    changed=[]
    for o in original:
        after=snapshot(o)
        expected=dict(before[o.Name])
        if o == link and 'children' in expected:
            expected['children']=expected['children']+[root.Name]
        if after != expected:
            changed.append(o.Name)
    validation['original_objects_changed_except_added_child']=changed
    validation['original_visibility_preserved']=True
    validation['fresh_process_parameter_test']='Not executed by authoring module; persistent native expressions saved for host inspection and reopen testing.'
    validation['robot_collision_check']='Unresolved: original wrist is mesh, no exact BRep obstacle evaluation or whole-motion clearance certification performed.'
    if changed:
        raise RuntimeError('Original object preservation check failed: '+str(changed))
    if not validation['mount_valid'] or validation['solid_count'] != 1:
        raise RuntimeError('Mount is not a valid single solid')
    if any(v>1e-7 for v in overlaps.values()) or any(v>1e-7 for v in containment.values()):
        raise RuntimeError('Reference clearance or feature containment failed: '+str(validation))
    for o in physical:
        o.Visibility=False
    for o in camera.Group:
        o.Visibility=False
    mount.Visibility=True
    mount.ViewObject.ShapeColor=(0.25,0.45,0.75)
    for o in [envelope,ribbon,optical]:
        o.Visibility=True
        o.ViewObject.Transparency=85
    envelope.ViewObject.ShapeColor=(0.2,0.75,0.3)
    ribbon.ViewObject.ShapeColor=(1.0,0.65,0.15)
    stage('Final native solid and measured reference clearance checks')
    os.makedirs(output_dir,exist_ok=True)
    assembly_path=os.path.join(output_dir,'assembly.FCStd')
    manifest_path=os.path.join(output_dir,'camera-mount-manifest.json')
    doc.recompute()
    doc.saveAs(assembly_path)
    manifest={
        'camera_identity':envelope.CameraModel,
        'dimensional_source':envelope.DimensionalSource,
        'camera_coordinate_convention':camera.CoordinateConvention,
        'input_sha256':source_hash,
        'final_mount_name':mount.Name,
        'new_objects':[o.Name for o in doc.Objects if o.Name not in original_names],
        'measured_wrist_frame':{'parent':link.Name,'visual':visual.Name,'visual_local_matrix':list(visual.Placement.toMatrix().A),'attachment_part_matrix':list(attachment.toMatrix().A),'attachment_in_link_matrix':list(root.Placement.toMatrix().A),'rule':'Parent to moving Link_gripper_link; source visual local placement composed with measured U,V,N attachment frame','bores_part_mm':[[-5,-24.2182144793536,24.3498679609405],[3.1,-24.2182144793536,24.3498679609405]],'measured_diameter_mm':3.2,'wall_depth_mm':3.5,'nut_recess_AF_mm':5.6,'nut_recess_depth_mm':3.3},
        'native_parameter_bindings':{o.Name:[(str(a),str(b)) for a,b in o.ExpressionEngine] for o in doc.Objects if o.Name not in original_names and hasattr(o,'ExpressionEngine') and o.ExpressionEngine},
        'design_choices':{'PlateThickness_mm':3,'StandoffHeight_mm':6,'minimum_standoff_mm':5,'plate_origin_in_attachment_mm':[-12.5,12,0],'plate_xy_bounds_mm':[-1,-1,26,24.862],'boss_radius_mm':2.5,'camera_holes_mm':HOLES,'camera_clearance_diameter_mm':2.2,'wrist_clearance_diameter_mm':3.4,'rear_reservation':'5 mm behind PCB central area x=5..20; corner mounting lands excluded','orientation':'Camera optical +Z equals measured outward normal'},
        'hardware_assumptions':{'camera':'4 M2 through screws, 4 M2 nuts; optional insulating washers not included in stack. PCB 1.12 + H 6 + T 3 = 10.12 mm grip; assume 1.6 mm nut plus ~0.8 mm projection, select nominal M2x14 for prototype review. Screw heads on front PCB mounting lands, nuts below plate. Actual head footprint, component clearance and screw-tip clearance require verification. No engagement is claimed.','wrist':'2 M3 screws, 2 M3 nuts. T 3 + original wall 3.5 = 6.5 mm grip; assume 2.4 mm nut, select nominal M3x10, giving 1.1 mm beyond assumed nut. Remaining motor clearance unverified. Motor6 removal required to insert nuts per supplied guide. Right bore bearing is partial; no strength approval.','parameter_change_note':'Recalculate hardware lengths after thickness or standoff edits; hardware lengths are proposals, not native fastener geometry.'},
        'cable_keepout':{'name':ribbon.Name,'frame':'CameraFrame','bounds_mm':[3.5,23.862,-3,21.5,38.862,-1],'chosen_exit':'+Y','actual_connector_geometry':'Unknown from supplied dimensions; assumption requires physical verification'},
        'validation':validation,
        'limitations':['Prototype only; no manufacturing or strength certification.','Exact rear components and connector geometry unavailable; central envelopes are assumptions.','Full optical field of view and complete robot motion clearance unverified.','No mesh-to-solid conversion used. Original source file not modified.','Native expressions support independent edits; fresh-process testing remains for host evaluation.','Minimum H=5 mm is a design constraint, not enforced by the unrestricted length property.']
    }
    with open(manifest_path,'w') as f:
        json.dump(manifest,f,indent=2)
    return {'assembly':assembly_path,'manifest':manifest_path,'outcome':'Saved native editable camera mount; actual native validity, reference overlaps and preservation checks recorded. Robot motion and hardware access remain unverified.'}
