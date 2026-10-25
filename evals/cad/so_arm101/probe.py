"""Fresh-process FreeCAD measurements; no candidate Python helpers imported."""
import argparse,json,sys,time,xml.etree.ElementTree as ET
from pathlib import Path
import FreeCAD as App
sys.path.insert(0,str(Path(__file__).resolve().parent))
from geometry import check_mesh,fingerprint
p=argparse.ArgumentParser();p.add_argument('--candidate',required=True);p.add_argument('--urdf',required=True);p.add_argument('--queries',required=True);p.add_argument('--output',required=True);p.add_argument('--save-state');p.add_argument('--check-geometry',action='store_true');args=p.parse_args()
out=Path(args.output);out.parent.mkdir(parents=True,exist_ok=True);start=time.time();report={'status':'running','states':[],'geometry':{},'errors':[]}
def write():out.write_text(json.dumps(report,indent=2))
def matrix(obj):
 m=obj.getGlobalPlacement().toMatrix();return [[getattr(m,'A'+str(i+1)+str(j+1)) for j in range(4)] for i in range(4)]
def native_origin_feature(obj):
 if obj.TypeId not in ('App::Line','App::Plane','App::Point'):return False
 return any(parent.TypeId=='App::Origin' and obj in parent.OriginFeatures and any(owner.TypeId=='App::Part' and getattr(owner,'Origin',None)==parent for owner in parent.InList) for parent in obj.InList)
def raw_mesh(obj):
 mesh=obj.Mesh.copy();mesh.Placement=App.Placement();return mesh
try:
 root=ET.parse(args.urdf).getroot();doc=App.openDocument(str(Path(args.candidate).resolve()));controls=doc.getObject('JointParameters')
 if controls is None:raise ValueError('missing JointParameters')
 joints=[j for j in root.findall('joint') if j.get('type')=='revolute'];links=[e.get('name') for e in root.findall('link')]
 visuals=[(link.get('name'),i,v.find('geometry/mesh').get('filename')) for link in root.findall('link') for i,v in enumerate(link.findall('visual'))]
 report['controls']={}
 for joint in joints:
  n=joint.get('name');report['controls'][n]={}
  for key in [n,n+'_lower',n+'_upper']:
   if key not in controls.PropertiesList:raise ValueError('missing control or limit '+key)
   report['controls'][n][key]={'type':controls.getTypeIdOfProperty(key),'value_degrees':float(getattr(controls,key))}
 expected_names={'Visual_'+n+'_'+str(i) for n,i,_ in visuals}
 report['custom_proxies']=[o.Name for o in doc.Objects if getattr(o,'Proxy',None) is not None]
 report['visual_names']=[o.Name for o in doc.Objects if o.Name.startswith('Visual_')]
 report['native_origin_features_excluded']=[o.Name for o in doc.Objects if native_origin_feature(o)]
 report['unexpected_geometry']=[{'name':o.Name,'type':o.TypeId} for o in doc.Objects if (hasattr(o,'Mesh') or hasattr(o,'Shape')) and o.Name not in expected_names and not native_origin_feature(o)]
 for n in links:
  o=doc.getObject('Link_'+n)
  if o is None or o.TypeId!='App::Part':raise ValueError('missing/non-App::Part link '+n)
 for n,i,path in visuals:
  name='Visual_'+n+'_'+str(i);o=doc.getObject(name)
  if o is None or o.TypeId!='Mesh::Feature':raise ValueError('missing/non-Mesh::Feature visual '+name)
  if args.check_geometry:
   mesh=raw_mesh(o);report['geometry'][name]=check_mesh(mesh,Path(args.urdf).parent/path)
   report['geometry'][name].update(is_solid_mesh=bool(mesh.isSolid()),raw_geometry_fingerprint=fingerprint(mesh))
 write()
 queries=[{'id':'loaded','angles_degrees':None}]+json.loads(Path(args.queries).read_text())
 for query in queries:
  if query['angles_degrees'] is not None:
   for n,value in query['angles_degrees'].items():setattr(controls,n,value)
   doc.recompute()
  state={'id':query['id'],'angles_degrees':{j.get('name'):float(getattr(controls,j.get('name'))) for j in joints},'links':{},'visuals':{}}
  for n in links:state['links'][n]=matrix(doc.getObject('Link_'+n))
  for n,i,_ in visuals:
   name='Visual_'+n+'_'+str(i);o=doc.getObject(name);state['visuals'][name]=matrix(o)
  report['states'].append(state);write();print('Measured',query['id'],flush=True)
 if args.save_state:doc.saveAs(str(Path(args.save_state).resolve()));report['saved_state']=str(Path(args.save_state).resolve())
 report.update(status='complete',elapsed_seconds=time.time()-start);write()
except Exception as error:
 report.update(status='failed',error=str(error),elapsed_seconds=time.time()-start);write();raise
