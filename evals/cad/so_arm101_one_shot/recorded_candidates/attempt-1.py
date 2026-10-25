import sys, os, json, math, hashlib, pathlib, xml.etree.ElementTree as ET
import FreeCAD as App
import Mesh

INPUT = pathlib.Path(sys.argv[1]).resolve()
OUTPUT = pathlib.Path(sys.argv[2]).resolve()
OUTPUT.mkdir(parents=True, exist_ok=True)

def inside(path):
    path = path.resolve()
    if path != INPUT and INPUT not in path.parents:
        raise ValueError('Source path escapes INPUT_DIR')
    return path

def identity(path):
    path = inside(path)
    data = path.read_bytes()
    return {'path': str(path.relative_to(INPUT)), 'bytes': len(data), 'sha256': hashlib.sha256(data).hexdigest()}

found = sorted(INPUT.rglob('so101_new_calib.urdf'))
if len(found) != 1:
    raise ValueError('Expected exactly one so101_new_calib.urdf')
urdf = inside(found[0])
urdf_id = identity(urdf)
if urdf_id['sha256'] != '3a65d2d35e68a8d2f0c2cc176d19b884506543c93ba72980145b80abe276022c':
    raise ValueError('Official URDF identity mismatch')
root = ET.fromstring(urdf.read_bytes())

def numbers(value, default):
    return [float(x) for x in (value if value is not None else default).split()]

def origin(element):
    node = element.find('origin')
    xyz = numbers(node.get('xyz') if node is not None else None, '0 0 0')
    rpy = numbers(node.get('rpy') if node is not None else None, '0 0 0')
    roll, pitch, yaw = [math.degrees(v) for v in rpy]
    rotation = App.Rotation(App.Vector(0,0,1), yaw).multiply(App.Rotation(App.Vector(0,1,0), pitch)).multiply(App.Rotation(App.Vector(1,0,0), roll))
    return App.Placement(App.Vector(*[v*1000 for v in xyz]), rotation), {'xyz_m': xyz, 'rpy_rad': rpy}

def resolve_mesh(filename):
    if '://' in filename or pathlib.Path(filename).is_absolute():
        raise ValueError('Unsupported nonlocal mesh path: ' + filename)
    candidate = inside(urdf.parent / filename)
    if candidate.is_file():
        return candidate
    matches = sorted(INPUT.rglob(pathlib.Path(filename).name))
    if len(matches) != 1:
        raise ValueError('Cannot uniquely resolve mesh: ' + filename)
    return inside(matches[0])

def string_property(obj, name, value):
    obj.addProperty('App::PropertyString', name, 'Source')
    setattr(obj, name, value)

links_xml = root.findall('link')
joints_xml = root.findall('joint')
controls = ['shoulder_pan', 'shoulder_lift', 'elbow_flex', 'wrist_flex', 'wrist_roll', 'gripper']
assert len(links_xml) == 8
assert sum(len(link.findall('visual')) for link in links_xml) == 17
assert sorted(j.get('name') for j in joints_xml if j.get('type') == 'revolute') == sorted(controls)
assert len(joints_xml) == 7

doc = App.newDocument('SO_ARM101_Follower')
params = doc.addObject('App::Feature', 'JointParameters')
params.Label = 'Joint parameters (degrees)'
string_property(params, 'SourceURDF', urdf_id['path'])
string_property(params, 'SourceSHA256', urdf_id['sha256'])
string_property(params, 'Articulation', 'Native nested App::Part frames; Placement.Rotation.Angle expressions; no Python proxy or callback')
for name in controls:
    params.addProperty('App::PropertyAngle', name, 'Joint controls', 'Revolute joint angle in degrees; no clamping')
    setattr(params, name, 0.0)

frames = {}
manifest = {
    'artifact': 'assembly.FCStd',
    'reference_commit': 'a758567c3978dfeefe282ede0500085a48fe8f78',
    'source_license': 'Apache-2.0',
    'urdf': urdf_id,
    'units': {'urdf_translation': 'metre', 'source_STL_coordinates': 'metre under the URDF mesh convention', 'FreeCAD_geometry_and_translation': 'millimetre', 'mesh_multiplier': '1000 times each URDF mesh scale component', 'controls': 'degree'},
    'world_frame': 'base_link; identity world placement; no registration',
    'links': [], 'visuals': [], 'joints': [], 'meshes': [],
    'controls_object': 'JointParameters',
    'persistence': 'Each joint origin is a native App::Part nested under its parent Link frame. The child Link frame is nested under that origin. Its local axis-angle rotation is driven by a native Placement.Rotation.Angle expression referencing JointParameters. All expressions, mesh payloads, and hierarchy are serialized into FCStd; no external Python is needed after reopening.',
    'native_limitations': ['Mesh::Feature geometry is triangle mesh, not a solid BRep.', 'Joint limits are recorded as metadata; controls are not clamped.', 'No collision-clearance or physical-operation certification.', 'URDF inertial and transmission elements are not dynamics or motor-control implementations.', 'Independent evaluation and fresh-process reopen validation are not performed by this construction script.'],
    'incomplete_requirements': [],
    'preview': {}
}

for link in links_xml:
    name = link.get('name')
    frame = doc.addObject('App::Part', 'Link_' + name)
    frame.Label = name
    string_property(frame, 'URDFLink', name)
    frames[name] = frame
    manifest['links'].append({'urdf_link': name, 'object': frame.Name})

children = set()
for joint in joints_xml:
    name, kind = joint.get('name'), joint.get('type')
    parent = joint.find('parent').get('link')
    child = joint.find('child').get('link')
    if child in children:
        raise ValueError('Duplicate parent relationship')
    children.add(child)
    placement, source_origin = origin(joint)
    carrier = doc.addObject('App::Part', 'JointOrigin_' + name)
    carrier.Label = name + ' — fixed URDF origin'
    frames[parent].addObject(carrier)
    carrier.Placement = placement
    carrier.addObject(frames[child])
    frames[child].Placement = App.Placement()
    string_property(carrier, 'URDFJoint', name)
    string_property(carrier, 'JointType', kind)
    record = {'name': name, 'type': kind, 'parent': parent, 'child': child, 'origin_object': carrier.Name, 'origin': source_origin}
    if kind == 'revolute':
        axis = numbers(joint.find('axis').get('xyz'), '1 0 0')
        axis_vector = App.Vector(*axis)
        if axis_vector.Length == 0:
            raise ValueError('Zero revolute joint axis')
        axis_vector.normalize()
        # All six official joints have the same local +Z axis.
        # Keep that local axis below the separate, unmodified origin rotation.
        frames[child].Placement = App.Placement(App.Vector(), App.Rotation(axis_vector, 0))
        frames[child].addProperty('App::PropertyVector', 'URDFJointAxis', 'Source')
        frames[child].URDFJointAxis = App.Vector(*axis)
        expression = 'JointParameters.' + name
        frames[child].setExpression('Placement.Rotation.Angle', expression)
        limit = joint.find('limit')
        limits = {}
        for suffix in ('lower', 'upper'):
            raw = limit.get(suffix)
            radians = float(raw)
            prop = name + '_' + suffix
            params.addProperty('App::PropertyAngle', prop, 'Source limits', 'Official URDF ' + suffix + ' limit, converted from radians')
            setattr(params, prop, math.degrees(radians))
            params.setEditorMode(prop, 1)
            limits[suffix] = {'source_radians_text': raw, 'radians': radians, 'degrees': math.degrees(radians), 'property': prop}
        record.update({'axis_local': axis, 'control': name, 'control_type': 'App::PropertyAngle', 'initial_degrees': 0, 'limits': limits, 'expression': expression, 'expression_target': frames[child].Name + '.Placement.Rotation.Angle'})
    elif kind != 'fixed':
        raise ValueError('Unsupported joint type')
    manifest['joints'].append(record)

assert set(frames) - children == {'base_link'}
frames['base_link'].Placement = App.Placement()
materials = {}
for material in root.findall('material'):
    color = material.find('color')
    if color is not None:
        materials[material.get('name')] = numbers(color.get('rgba'), '0.8 0.8 0.8 1')
mesh_cache = {}
mesh_ids = {}
preview_objects = []
for link in links_xml:
    name = link.get('name')
    for index, visual in enumerate(link.findall('visual')):
        mesh_element = visual.find('geometry/mesh')
        if mesh_element is None:
            raise ValueError('Expected source mesh geometry')
        filename = mesh_element.get('filename')
        path = resolve_mesh(filename)
        scale = numbers(mesh_element.get('scale'), '1 1 1')
        key = (str(path), tuple(scale))
        if key not in mesh_cache:
            mesh = Mesh.Mesh(str(path))
            transform = App.Matrix()
            transform.A11, transform.A22, transform.A33 = [1000.0*v for v in scale]
            mesh.transform(transform)
            mesh_cache[key] = mesh
        if str(path) not in mesh_ids:
            mesh_ids[str(path)] = identity(path)
        obj = doc.addObject('Mesh::Feature', 'Visual_' + name + '_' + str(index))
        frames[name].addObject(obj)
        obj.Mesh = mesh_cache[key].copy()
        obj.Placement, visual_origin = origin(visual)
        obj.Label = name + ' / ' + pathlib.Path(filename).stem + ' [' + str(index) + ']'
        string_property(obj, 'SourceMesh', mesh_ids[str(path)]['path'])
        string_property(obj, 'SourceSHA256', mesh_ids[str(path)]['sha256'])
        string_property(obj, 'GeometryKind', 'Source triangle mesh; not a solid BRep')
        material = visual.find('material')
        rgba = [0.8,0.8,0.8,1]
        if material is not None:
            rgba = materials.get(material.get('name'), rgba)
            local_color = material.find('color')
            if local_color is not None:
                rgba = numbers(local_color.get('rgba'), '0.8 0.8 0.8 1')
        if App.GuiUp:
            obj.ViewObject.ShapeColor = tuple(rgba[:3])
            obj.ViewObject.Transparency = int(round(100*(1-rgba[3])))
            obj.ViewObject.Visibility = True
        preview_objects.append((obj, rgba))
        manifest['visuals'].append({'object': obj.Name, 'link': name, 'index': index, 'source_filename': filename, 'mesh_identity': mesh_ids[str(path)], 'urdf_scale': scale, 'origin': visual_origin, 'facets': obj.Mesh.CountFacets, 'points': obj.Mesh.CountPoints, 'type': obj.TypeId})
manifest['meshes'] = list(mesh_ids.values())
doc.recompute()

# Check only the authored document's native angle-expression connection.
# This is not an independent URDF or fresh-process validation.
for name in controls:
    joint = next(j for j in manifest['joints'] if j['name'] == name)
    child_frame = frames[joint['child']]
    setattr(params, name, 7.0)
    doc.recompute()
    measured = math.degrees(child_frame.Placement.Rotation.Angle)
    if abs(measured - 7.0) > 1e-7:
        raise RuntimeError('Native angle expression did not yield degrees for ' + name + ': ' + str(measured))
    setattr(params, name, 0.0)
    doc.recompute()
manifest['construction_checks'] = {'link_count': len(frames), 'visual_count': len(preview_objects), 'native_angle_expression_probe_degrees': 7, 'initial_controls_restored_to_zero': True, 'scope': 'Same-process local expression check only; no independent evaluation claim'}

preview_errors = []
if App.GuiUp:
    try:
        import FreeCADGui as Gui
        Gui.activeDocument().activeView().viewAxonometric()
        Gui.activeDocument().activeView().fitAll()
        Gui.updateGui()
        destination = OUTPUT / 'assembly-preview.png'
        Gui.activeDocument().activeView().saveImage(str(destination), 1600, 1200, 'White')
        if not destination.is_file() or destination.stat().st_size == 0:
            raise RuntimeError('GUI did not produce an image')
        manifest['preview'] = {'file': destination.name, 'method': 'FreeCAD axonometric exterior view'}
    except Exception as exc:
        preview_errors.append('GUI preview: ' + str(exc))

if not manifest['preview']:
    try:
        # Headless fallback: orthographic painter rendering of every actual
        # source triangle at its authored global placement; no proxy geometry.
        view = App.Vector(1,-1,0.75)
        view.normalize()
        right = App.Vector(1,1,0)
        right.normalize()
        up = view.cross(right)
        up.normalize()
        triangles = []
        xmin = ymin = float('inf')
        xmax = ymax = -float('inf')
        light = App.Vector(0.4,-0.5,1)
        light.normalize()
        for obj, rgba in preview_objects:
            placement = obj.getGlobalPlacement()
            points, facets = obj.Mesh.Topology
            world = [placement.multVec(p) for p in points]
            projected = [(p.dot(right), -p.dot(up), p.dot(view)) for p in world]
            for p in projected:
                xmin, xmax = min(xmin,p[0]), max(xmax,p[0])
                ymin, ymax = min(ymin,p[1]), max(ymax,p[1])
            for facet in facets:
                a,b,c = facet
                normal = (world[b]-world[a]).cross(world[c]-world[a])
                if normal.Length:
                    normal.normalize()
                shade = 0.38 + 0.62*abs(normal.dot(light))
                color = '#%02x%02x%02x' % tuple(max(0,min(255,round(255*v*shade))) for v in rgba[:3])
                vertices = (projected[a],projected[b],projected[c])
                triangles.append((sum(v[2] for v in vertices)/3, vertices, color))
        triangles.sort(key=lambda item: item[0])
        factor = min(1440/max(xmax-xmin,1e-9), 1040/max(ymax-ymin,1e-9))
        cx, cy = (xmin+xmax)/2, (ymin+ymax)/2
        destination = OUTPUT / 'assembly-preview.svg'
        with destination.open('w', encoding='utf-8') as stream:
            stream.write('<svg xmlns="http://www.w3.org/2000/svg" width="1600" height="1200" viewBox="0 0 1600 1200"><title>SO-ARM101 follower — zero-pose exterior mesh preview</title><rect width="1600" height="1200" fill="#f3f5f7"/>')
            for depth, vertices, color in triangles:
                coords = ' '.join('%.3f,%.3f' % (800+(v[0]-cx)*factor,600+(v[1]-cy)*factor) for v in vertices)
                stream.write('<polygon points="' + coords + '" fill="' + color + '"/>')
            stream.write('</svg>')
        manifest['preview'] = {'file': destination.name, 'method': 'Headless orthographic projection of all actual mesh triangles, depth sorted with flat shading', 'limitations': 'Painter ordering is an illustration, not an occlusion or collision test.', 'triangles': len(triangles)}
    except Exception as exc:
        preview_errors.append('Headless preview: ' + str(exc))
        manifest['preview'] = {'status': 'failed'}
        manifest['incomplete_requirements'].append('Exterior preview could not be generated; see preview errors.')
manifest['preview']['errors'] = preview_errors

doc.recompute()
doc.saveAs(str(OUTPUT / 'assembly.FCStd'))
manifest['authored_status'] = 'Construction and save completed; independent host evaluation pending.'
with (OUTPUT / 'candidate-manifest.json').open('w', encoding='utf-8') as stream:
    json.dump(manifest, stream, indent=2, ensure_ascii=False)
print('Authored assembly.FCStd and candidate-manifest.json. Independent evaluation has not been claimed.')
