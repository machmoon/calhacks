import sys, os, json, math, hashlib, pathlib, xml.etree.ElementTree as ET
import FreeCAD as App
import Mesh

source = pathlib.Path(sys.argv[1]).resolve()
out = pathlib.Path(sys.argv[2]).resolve()
out.mkdir(parents=True, exist_ok=True)

def inside(p):
    p = p.resolve()
    if source != p and source not in p.parents:
        raise RuntimeError('Source path escapes INPUT_DIR')
    return p

candidates = sorted(source.rglob('so101_new_calib.urdf'))
if len(candidates) != 1:
    raise RuntimeError('Expected exactly one official follower URDF')
urdf = inside(candidates[0])
raw = urdf.read_bytes()
expected = '3a65d2d35e68a8d2f0c2cc176d19b884506543c93ba72980145b80abe276022c'
if hashlib.sha256(raw).hexdigest() != expected:
    raise RuntimeError('Official URDF identity mismatch')
robot = ET.fromstring(raw)

def identity(p):
    data = p.read_bytes()
    return {'path': str(p.relative_to(source)), 'bytes': len(data), 'sha256': hashlib.sha256(data).hexdigest()}

def vector_attr(el, attr, default):
    return [float(x) for x in (el.get(attr, default) if el is not None else default).split()]

def origin(el):
    xyz = vector_attr(el, 'xyz', '0 0 0')
    r, p, y = vector_attr(el, 'rpy', '0 0 0')
    rot = App.Rotation(App.Vector(0,0,1), math.degrees(y)).multiply(App.Rotation(App.Vector(0,1,0), math.degrees(p))).multiply(App.Rotation(App.Vector(1,0,0), math.degrees(r)))
    return App.Placement(App.Vector(*[1000*x for x in xyz]), rot)

def resolve_mesh(name):
    relative = name
    if relative.startswith('package://'):
        relative = relative[len('package://'):].split('/', 1)[1]
    if relative.startswith('file://'):
        relative = relative[7:]
    for p in (urdf.parent / relative, source / relative):
        p = inside(p)
        if p.is_file():
            return p
    matches = [inside(p) for p in source.rglob(pathlib.Path(relative).name) if p.is_file()]
    if len(matches) != 1:
        raise RuntimeError('Missing or ambiguous source mesh: '+name)
    return matches[0]

doc = App.newDocument('SO101Follower')
params = doc.addObject('App::DocumentObjectGroup', 'JointParameters')
params.Label = 'Joint controls (degrees)'
controls = ['shoulder_pan','shoulder_lift','elbow_flex','wrist_flex','wrist_roll','gripper']
links_xml = robot.findall('link')
joints = robot.findall('joint')
assert len(links_xml) == 8
assert sum(len(x.findall('visual')) for x in links_xml) == 17
assert {j.get('name') for j in joints if j.get('type') == 'revolute'} == set(controls)
frames = {}
for link in links_xml:
    name = link.get('name')
    frame = doc.addObject('App::Part', 'Link_'+name)
    frame.Label = name
    frame.addProperty('App::PropertyString', 'URDFLink', 'Source')
    frame.URDFLink = name
    frames[name] = frame

joint_records = []
moving = []
for j in joints:
    name, kind = j.get('name'), j.get('type')
    parent = frames[j.find('parent').get('link')]
    child = frames[j.find('child').get('link')]
    fixed = origin(j.find('origin'))
    record = {'name':name, 'type':kind, 'parent':parent.Name, 'child':child.Name, 'origin_xyz_m':vector_attr(j.find('origin'),'xyz','0 0 0'), 'origin_rpy_rad':vector_attr(j.find('origin'),'rpy','0 0 0')}
    if kind == 'fixed':
        parent.addObject(child)
        child.Placement = fixed
    elif kind == 'revolute':
        axis = vector_attr(j.find('axis'), 'xyz', '1 0 0')
        axis_v = App.Vector(*axis)
        if axis_v.Length == 0:
            raise RuntimeError('Zero revolute joint axis')
        axis_v.normalize()
        params.addProperty('App::PropertyAngle', name, 'Controls', 'Revolute joint angle; no clamping')
        setattr(params, name, 0.0)
        lim = j.find('limit')
        for suffix in ('lower','upper'):
            prop = name+'_'+suffix
            params.addProperty('App::PropertyAngle', prop, 'URDF limits', 'Exact URDF limit converted from radians')
            setattr(params, prop, math.degrees(float(lim.get(suffix))))
            params.setEditorMode(prop, 1)
        anchor = doc.addObject('App::Part', 'JointOrigin_'+name)
        anchor.Label = name+' — fixed URDF origin'
        parent.addObject(anchor)
        anchor.Placement = fixed
        anchor.addObject(child)
        child.Placement = App.Placement(App.Vector(), App.Rotation(axis_v, 0))
        child.addProperty('App::PropertyVector','LocalJointAxis','URDF')
        child.LocalJointAxis = axis_v
        child.setExpression('Placement.Rotation.Angle', 'JointParameters.'+name)
        moving.append((name,child,axis_v))
        record.update({'axis':axis,'lower_rad':float(lim.get('lower')),'upper_rad':float(lim.get('upper')),'lower_deg':math.degrees(float(lim.get('lower'))),'upper_deg':math.degrees(float(lim.get('upper'))),'origin_object':anchor.Name})
    else:
        raise RuntimeError('Unsupported joint type '+str(kind))
    joint_records.append(record)

# Verify the native subproperty expression unit convention in this runtime.
# These are construction checks, not independent assembly evaluation.
expression_records = {}
for name, child, axis in moving:
    probe = App.Vector(1,0,0) if abs(axis.x) < 0.9 else App.Vector(0,1,0)
    expected_vector = App.Rotation(axis,17.0).multVec(probe)
    conventions = ['JointParameters.'+name, '(JointParameters.'+name+' / 1 deg) * '+repr(math.pi/180.0)]
    found = False
    for expr in conventions:
        child.setExpression('Placement.Rotation.Angle',expr)
        setattr(params,name,17.0)
        doc.recompute()
        if (child.Placement.Rotation.multVec(probe)-expected_vector).Length < 1e-9:
            expression_records[name] = expr
            found = True
            break
    if not found:
        raise RuntimeError('Native angle expression did not produce requested rotation: '+name)
    setattr(params,name,0.0)
    doc.recompute()

materials = {}
for material in robot.findall('material'):
    color = material.find('color')
    if color is not None:
        materials[material.get('name')] = [float(x) for x in color.get('rgba').split()]
mesh_records = {}
visual_records = []
mesh_cache = {}
for link in links_xml:
    name = link.get('name')
    for i, visual in enumerate(link.findall('visual')):
        mesh_xml = visual.find('geometry/mesh')
        if mesh_xml is None:
            raise RuntimeError('Expected supplied mesh visual')
        mesh_path = resolve_mesh(mesh_xml.get('filename'))
        scale = vector_attr(mesh_xml,'scale','1 1 1')
        key = (str(mesh_path),tuple(scale))
        if key not in mesh_cache:
            mesh = Mesh.Mesh(str(mesh_path))
            transform = App.Matrix()
            transform.A11, transform.A22, transform.A33 = [1000*s for s in scale]
            mesh.transform(transform)
            mesh_cache[key] = mesh
        obj = doc.addObject('Mesh::Feature','Visual_'+name+'_'+str(i))
        frames[name].addObject(obj)
        obj.Mesh = mesh_cache[key].copy()
        obj.Placement = origin(visual.find('origin'))
        obj.addProperty('App::PropertyString','SourceMesh','Source')
        obj.SourceMesh = str(mesh_path.relative_to(source))
        obj.addProperty('App::PropertyString','GeometryKind','Source')
        obj.GeometryKind = 'Source STL triangle mesh; not a solid BRep'
        obj.addProperty('App::PropertyString','SourceSHA256','Source')
        if str(mesh_path) not in mesh_records:
            mesh_records[str(mesh_path)] = identity(mesh_path)
        obj.SourceSHA256 = mesh_records[str(mesh_path)]['sha256']
        material = visual.find('material')
        rgba = materials.get(material.get('name'),[0.8,0.8,0.8,1]) if material is not None else [0.8,0.8,0.8,1]
        if material is not None and material.find('color') is not None:
            rgba = [float(x) for x in material.find('color').get('rgba').split()]
        if App.GuiUp:
            obj.ViewObject.ShapeColor = tuple(rgba[:3])
            obj.ViewObject.Transparency = int(round(100*(1-rgba[3])))
            obj.ViewObject.Visibility = True
        visual_records.append({'name':obj.Name,'link':frames[name].Name,'mesh':obj.SourceMesh,'sha256':obj.SourceSHA256,'source_scale':scale,'coordinate_multiplier_to_mm':[1000*s for s in scale],'origin_xyz_m':vector_attr(visual.find('origin'),'xyz','0 0 0'),'origin_rpy_rad':vector_attr(visual.find('origin'),'rpy','0 0 0'),'facets':obj.Mesh.CountFacets})

doc.recompute()
assert all(abs(getattr(params,n).Value) < 1e-12 for n in controls)
assert len([o for o in doc.Objects if o.Name.startswith('Link_') and o.TypeId == 'App::Part']) == 8
assert len([o for o in doc.Objects if o.Name.startswith('Visual_') and o.TypeId == 'Mesh::Feature']) == 17
preview = {'status':'unavailable','reason':'FreeCAD GUI is not active in this interpreter'}
if App.GuiUp:
    try:
        import FreeCADGui as Gui
        Gui.activeDocument().activeView().viewAxonometric()
        Gui.activeDocument().activeView().fitAll()
        Gui.updateGui()
        image_path = out/'assembly-preview.png'
        Gui.activeDocument().activeView().saveImage(str(image_path),1600,1200,'White')
        if not image_path.is_file() or image_path.stat().st_size == 0:
            raise RuntimeError('No preview image was written')
        preview = {'status':'written','file':image_path.name,'view':'exterior axonometric, zero pose'}
    except Exception as exc:
        preview = {'status':'failed','reason':str(exc)}
doc.recompute()
doc.saveAs(str(out/'assembly.FCStd'))

manifest = {
    'artifact':'assembly.FCStd',
    'reference_commit':'a758567c3978dfeefe282ede0500085a48fe8f78',
    'source_license':'Apache-2.0',
    'urdf':identity(urdf),
    'meshes':list(mesh_records.values()),
    'units':{'URDF_translation':'metres','URDF_rotation':'radians','STL_interpretation':'URDF mesh coordinates in metres with explicit mesh scale applied; STL itself is unitless','FreeCAD_length':'millimetres','controls':'degrees','world_frame':'base_link; identity root placement'},
    'links':[{'urdf_name':n,'object':f.Name} for n,f in frames.items()],
    'visuals':visual_records,
    'joints':joint_records,
    'controls':{'object':'JointParameters','type':'App::PropertyAngle','names':controls,'initial_degrees':{n:0 for n in controls},'expressions':expression_records},
    'persistence':'Native App::Part nesting. Each revolute joint has a fixed JointOrigin App::Part under its parent link, then the child Link App::Part with a native ExpressionEngine binding on Placement.Rotation.Angle. Fixed joint links are directly nested with their URDF placement. Mesh visuals carry local URDF origins. No Python proxy, observer, macro, or external code is required after reopening.',
    'native_limitations':['Mesh geometry remains triangle meshes, not solid BReps.','Joint limit properties are metadata; controls are not clamped.','No collision-clearance, dynamics, fabrication, or physical-operation certification.','Inertial, transmission and duplicate collision elements are not modeled as additional geometry.'],
    'construction_checks':['Eight link frames and seventeen distinct visual instances counted.','Six native rotation expressions checked at 17 degrees locally and reset to zero.'],
    'independent_evaluation':'Not performed by this script; no evaluation success claimed.',
    'preview':preview,
    'incomplete_requirements':([] if preview['status']=='written' else ['Exterior preview was not produced: '+preview.get('reason','unknown reason')])
}
(out/'candidate-manifest.json').write_text(json.dumps(manifest,indent=2),encoding='utf-8')
