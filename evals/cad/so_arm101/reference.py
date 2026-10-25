"""Independent standard-library URDF frame extraction; no FreeCAD imports."""
import json,math,xml.etree.ElementTree as ET
from pathlib import Path
DEFAULT_SOURCE=Path(__file__).resolve().parents[3]/'cad/projects/so-arm101/upstream/Simulation/SO101/so101_new_calib.urdf'
def identity():return [[float(i==j) for j in range(4)] for i in range(4)]
def mul(a,b):return [[sum(a[i][k]*b[k][j] for k in range(4)) for j in range(4)] for i in range(4)]
def origin(element):
 xyz=[float(x) for x in element.get('xyz','0 0 0').split()];r,p,y=[float(x) for x in element.get('rpy','0 0 0').split()]
 cr,sr,cp,sp,cy,sy=math.cos(r),math.sin(r),math.cos(p),math.sin(p),math.cos(y),math.sin(y)
 return [[cy*cp,cy*sp*sr-sy*cr,cy*sp*cr+sy*sr,xyz[0]],[sy*cp,sy*sp*sr+cy*cr,sy*sp*cr-cy*sr,xyz[1]],[-sp,cp*sr,cp*cr,xyz[2]],[0.,0.,0.,1.]]
def revolute(axis,q):
 x,y,z=axis;n=math.sqrt(x*x+y*y+z*z);x,y,z=x/n,y/n,z/n;c=math.cos(q);s=math.sin(q);v=1-c
 return [[c+x*x*v,x*y*v-z*s,x*z*v+y*s,0.],[y*x*v+z*s,c+y*y*v,y*z*v-x*s,0.],[z*x*v-y*s,z*y*v+x*s,c+z*z*v,0.],[0.,0.,0.,1.]]
def fk(q,source=DEFAULT_SOURCE):
 root=ET.parse(source).getroot();world={'base_link':identity()};pending=list(root.findall('joint'));axes={}
 while pending:
  progress=False
  for joint in list(pending):
   parent=joint.find('parent').get('link');child=joint.find('child').get('link');name=joint.get('name')
   if parent not in world:continue
   frame=mul(world[parent],origin(joint.find('origin')));motion=identity()
   if joint.get('type')=='revolute':
    axis=[float(x) for x in joint.find('axis').get('xyz').split()];angle=q.get(name,0.);bounds=joint.find('limit')
    if not float(bounds.get('lower'))<=angle<=float(bounds.get('upper')):raise ValueError('joint outside official CAD-description limit: '+name)
    axes[name]={'origin_mm':[1000*frame[i][3] for i in range(3)],'direction':[sum(frame[i][k]*axis[k] for k in range(3)) for i in range(3)]};motion=revolute(axis,angle)
   world[child]=mul(frame,motion);pending.remove(joint);progress=True
  if not progress:raise ValueError('cyclic or disconnected source tree')
 visuals=[]
 for link in root.findall('link'):
  for i,v in enumerate(link.findall('visual')):visuals.append({'link':link.get('name'),'index':i,'mesh':v.find('geometry/mesh').get('filename'),'world_transform_metres':mul(world[link.get('name')],origin(v.find('origin')))})
 return {'joint_values_radians':q,'link_world_transforms_metres':world,'joint_axes_world':axes,'visual_instances':visuals}
