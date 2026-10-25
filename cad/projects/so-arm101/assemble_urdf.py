"""Direct FreeCAD assembly from independent official SO101 URDF mesh assets.

Uses native App::Part placements and expressions only: no custom proxy, copied
preassembled STEP, robot-control code, or BRep claims for triangle meshes.
"""
import argparse,hashlib,json,math,xml.etree.ElementTree as ET
from pathlib import Path
import FreeCAD as App
import Mesh
PROJECT=Path(__file__).resolve().parent
ROOT=PROJECT.parents[2]
SOURCE=PROJECT/'upstream/Simulation/SO101/so101_new_calib.urdf'
MANIFEST=PROJECT/'articulation-source-manifest.json'

def vector(text, default='0 0 0'):
    values=[float(v) for v in (text or default).split()]
    if len(values)!=3:raise ValueError('expected three vector components')
    return values

def origin(element):
    xyz=vector(element.get('xyz')) if element is not None else [0,0,0]
    rpy=vector(element.get('rpy')) if element is not None else [0,0,0]
    rotations=[App.Rotation(App.Vector(*axis),math.degrees(angle)) for axis,angle in zip([(1,0,0),(0,1,0),(0,0,1)],rpy)]
    rotation=rotations[2].multiply(rotations[1]).multiply(rotations[0])
    return App.Placement(App.Vector(*[1000*v for v in xyz]),rotation)

def add_string(obj,name,value):
    obj.addProperty('App::PropertyString',name,'Source');setattr(obj,name,value);obj.setEditorMode(name,1)

def build(output):
    provenance=json.loads(MANIFEST.read_text())
    records={f['upstream_path']:f for f in provenance['files']}
    for name,record in records.items():
        path=PROJECT/'upstream'/name
        if hashlib.sha256(path.read_bytes()).hexdigest()!=record['sha256']:raise ValueError('source hash mismatch: '+name)
    robot=ET.parse(SOURCE).getroot();doc=App.newDocument('SO101_Articulated')
    params=doc.addObject('App::DocumentObjectGroup','JointParameters');params.Label='SO101 joint angles (degrees)'
    add_string(params,'SourceCommit',provenance['commit'])
    add_string(params,'GeometryKind','official independent triangle mesh visual assets; no BRep conversion')
    links={e.get('name'):doc.addObject('App::Part','Link_'+e.get('name')) for e in robot.findall('link')}
    for name,obj in links.items():obj.Label=name;add_string(obj,'URDFLink',name)
    joints=[];controls={}
    for joint in robot.findall('joint'):
        name=joint.get('name');kind=joint.get('type');parent=joint.find('parent').get('link');child=joint.find('child').get('link')
        fixed=doc.addObject('App::Part','Origin_'+name);links[parent].addObject(fixed);fixed.Placement=origin(joint.find('origin'));fixed.Label=name+' fixed origin'
        if kind=='fixed':fixed.addObject(links[child])
        elif kind=='revolute':
            axis=vector(joint.find('axis').get('xyz'));assert axis==[0.,0.,1.], 'SO101 source joint axis changed'
            limits=joint.find('limit');low=math.degrees(float(limits.get('lower')));high=math.degrees(float(limits.get('upper')))
            for prop,value in [(name,0),(name+'_lower',low),(name+'_upper',high)]:
                params.addProperty('App::PropertyAngle',prop,'Joint angles' if prop==name else 'Official limits');setattr(params,prop,value)
                if prop!=name:params.setEditorMode(prop,1)
            motion=doc.addObject('App::Part','Motion_'+name);fixed.addObject(motion);motion.Placement=App.Placement(App.Vector(),App.Rotation(App.Vector(0,0,1),0));motion.addObject(links[child])
            motion.setExpression('Placement.Rotation.Angle','JointParameters.'+name)
            controls[name]=dict(lower_degrees=low,upper_degrees=high,motion_object=motion.Name)
        else:raise ValueError('unsupported official joint type '+str(kind))
        joints.append(dict(name=name,type=kind,parent=parent,child=child,origin_object=fixed.Name))
    meshes={};instances=[]
    for element in robot.findall('link'):
        name=element.get('name')
        for i,visual in enumerate(element.findall('visual')):
            spec=visual.find('geometry/mesh')
            if spec is None:raise ValueError('expected official mesh visual')
            filename=spec.get('filename');path=(SOURCE.parent/filename).resolve()
            if not path.is_relative_to(SOURCE.parent.resolve()):raise ValueError('mesh source escapes bundle')
            scale=vector(spec.get('scale'),'1 1 1');key=(filename,tuple(scale))
            if key not in meshes:
                mesh=Mesh.Mesh(str(path));matrix=App.Matrix();matrix.A11=scale[0]*1000;matrix.A22=scale[1]*1000;matrix.A33=scale[2]*1000;mesh.transform(matrix);meshes[key]=mesh
            obj=doc.addObject('Mesh::Feature',f'Visual_{name}_{i}');links[name].addObject(obj);obj.Mesh=meshes[key].copy();obj.Placement=origin(visual.find('origin'));obj.Label=Path(filename).stem
            for prop,value in [('SourceAsset',filename),('SourceSHA256',hashlib.sha256(path.read_bytes()).hexdigest()),('GeometryKind','triangle_mesh'),('URDFLink',name)]:add_string(obj,prop,value)
            obj.addProperty('App::PropertyInteger','URDFVisualIndex','Source');obj.URDFVisualIndex=i;obj.setEditorMode('URDFVisualIndex',1)
            instances.append(dict(object_name=obj.Name,link=name,index=i,source_asset=filename,source_sha256=obj.SourceSHA256,geometry_kind='triangle_mesh',facets=obj.Mesh.CountFacets,source_unit_to_mm=1000))
    doc.recompute()
    assert len(controls)==6 and len(instances)==17 and len(links)==8
    output.mkdir(parents=True,exist_ok=True);path=output/'assembly.FCStd';doc.saveAs(str(path))
    report=dict(native=str(path),native_sha256=hashlib.sha256(path.read_bytes()).hexdigest(),source_urdf=str(SOURCE),source_urdf_sha256=hashlib.sha256(SOURCE.read_bytes()).hexdigest(),source_commit=provenance['commit'],coordinate_frame='Official URDF base_link; translation metres converted to millimetres, Rz(yaw)Ry(pitch)Rx(roll); no registration to preassembled STEP',controls_object=params.Name,controls=controls,links={n:o.Name for n,o in links.items()},joints=joints,visual_instances=instances,geometry_kind='mesh-native articulated FreeCAD assembly',brep_geometry_count=0,custom_proxy_required=False,source_preassembled_cad_used=False,collision_checked=False,structural_checked=False,scope='Editable native articulation and source visuals; not robot control, BRep reconstruction, collision or strength certification')
    (output/'manifest.json').write_text(json.dumps(report,indent=2));return doc,report

def set_pose(doc,values,controls):
    for name,value in values.items():
        if name not in controls or not isinstance(value,(int,float)) or not math.isfinite(value):raise ValueError('unknown or invalid joint angle')
        bound=controls[name]
        if not bound['lower_degrees']<=value<=bound['upper_degrees']:raise ValueError('angle outside official limits: '+name)
    for name,value in values.items():setattr(doc.JointParameters,name,value)
    doc.recompute()

if __name__=='__main__':
    parser=argparse.ArgumentParser();parser.add_argument('--output',type=Path,default=ROOT/'out/so-arm101-candidate');args=parser.parse_args()
    doc,report=build(args.output.resolve());print(json.dumps(dict(native=report['native'],links=len(report['links']),controls=len(report['controls']),visual_instances=len(report['visual_instances']),geometry_kind=report['geometry_kind'])),flush=True)
