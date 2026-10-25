"""Bounded independent SO101 CAD evaluator; never calls candidate helpers."""
import argparse,hashlib,json,math,subprocess,sys,time,xml.etree.ElementTree as ET
from pathlib import Path
from reference import fk
HERE=Path(__file__).resolve().parent;ROOT=HERE.parents[2]
def sha(p):return hashlib.sha256(Path(p).read_bytes()).hexdigest()
def valid_matrix(m):
 if not isinstance(m,list) or len(m)!=4 or any(not isinstance(row,list) or len(row)!=4 for row in m):raise ValueError('matrix must be4x4')
 if any(isinstance(v,bool) or not isinstance(v,(int,float)) or not math.isfinite(v) for row in m for v in row):raise ValueError('nonfinite/non-numeric matrix')
 if max(abs(m[3][i]-[0,0,0,1][i]) for i in range(4))>1e-7:raise ValueError('invalid affine last row')
 if max(abs(sum(m[k][i]*m[k][j] for k in range(3))-float(i==j)) for i in range(3) for j in range(3))>1e-6:raise ValueError('non-orthonormal rotation')
 det=m[0][0]*(m[1][1]*m[2][2]-m[1][2]*m[2][1])-m[0][1]*(m[1][0]*m[2][2]-m[1][2]*m[2][0])+m[0][2]*(m[1][0]*m[2][1]-m[1][1]*m[2][0])
 if abs(det-1)>1e-6:raise ValueError('rotation determinant is not+1')
def error(a,b):
 valid_matrix(a);valid_matrix(b)
 translation=math.sqrt(sum((a[i][3]-b[i][3])**2 for i in range(3)))
 trace=sum(sum(a[k][i]*b[k][i] for k in range(3)) for i in range(3))
 rotation=math.degrees(math.acos(max(-1,min(1,(trace-1)/2))))
 return translation,rotation
def mm(m):return [[v*1000 if j==3 and i<3 else v for j,v in enumerate(row)] for i,row in enumerate(m)]
def expected(q,urdf):
 ref=fk({k:math.radians(v) for k,v in q.items()},urdf)
 return {'links':{n:mm(m) for n,m in ref['link_world_transforms_metres'].items()},'visuals':{'Visual_'+v['link']+'_'+str(v['index']):mm(v['world_transform_metres']) for v in ref['visual_instances']},'axes':ref['joint_axes_world']}
def compare_state(state,q,urdf,cases):
 target=expected(q,urdf);rows=[]
 for group in ['links','visuals']:
  for name,want in target[group].items():
   actual=state[group].get(name)
   if actual is None:rows.append(dict(kind=group,name=name,passed=False,error='missing'));continue
   t,r=error(actual,want);rows.append(dict(kind=group,name=name,translation_error_mm=t,rotation_error_deg=r,passed=t<=cases['translation_tolerance_mm'] and r<=cases['rotation_tolerance_deg']))
 angles=all(abs(state['angles_degrees'].get(k,float('inf'))-v)<1e-7 for k,v in q.items())
 return dict(id=state['id'],passed=angles and all(r['passed'] for r in rows),angles_match=angles,measurements=rows)
def axis_check(before,after,joint,q,reference_axis,child):
 a=before['links'][child];b=after['links'][child];r=[[sum(b[i][k]*a[j][k] for k in range(3)) for j in range(3)] for i in range(3)]
 angle=math.radians(q);v=[(r[2][1]-r[1][2])/(2*math.sin(angle)),(r[0][2]-r[2][0])/(2*math.sin(angle)),(r[1][0]-r[0][1])/(2*math.sin(angle))];norm=math.sqrt(sum(x*x for x in v));v=[x/norm for x in v] if norm>1e-12 else [0,0,0]
 axis_error=math.degrees(math.acos(max(-1,min(1,sum(x*y for x,y in zip(v,reference_axis['direction']))))))
 t=[b[i][3]-sum(r[i][k]*a[k][3] for k in range(3)) for i in range(3)];p=reference_axis['origin_mm']
 displacement=math.sqrt(sum((sum(r[i][k]*p[k] for k in range(3))+t[i]-p[i])**2 for i in range(3)))
 return dict(joint=joint,axis_direction_error_deg=axis_error,axis_line_error_mm=displacement/(2*abs(math.sin(angle/2))))
def evaluate(first,second,urdf,cases):
 expected_ids=['loaded','zero']+['individual_'+n for n in cases['joint_order']]+['combined']
 for probe,ids in [(first,expected_ids),(second,['loaded','reopened_edit'])]:
  actual=[s.get('id') for s in probe.get('states',[])]
  if probe.get('status')!='complete' or len(actual)!=len(ids) or set(actual)!=set(ids):raise ValueError('incomplete/duplicate/unexpected probe states')
 root=ET.parse(urdf).getroot();joints={j.get('name'):j for j in root.findall('joint') if j.get('type')=='revolute'};names=cases['joint_order'];zero=dict.fromkeys(names,0.)
 expected_names=set(expected(zero,urdf)['visuals']);coverage=all(set(probe.get('visual_names',[]))==expected_names and len(probe.get('visual_names',[]))==17 and not probe.get('unexpected_geometry') for probe in [first,second])
 controls=[]
 for stage,probe in [('initial',first),('reopened',second)]:
  for n,j in joints.items():
   data=probe.get('controls',{}).get(n,{})
   for suffix,key in [('_lower','lower'),('_upper','upper')]:
    entry=data.get(n+suffix,{});v=entry.get('value_degrees',float('inf'));delta=abs(math.radians(v)-float(j.find('limit').get(key)));controls.append(dict(stage=stage,property=n+suffix,error_rad=delta,passed=entry.get('type')=='App::PropertyAngle' and delta<=cases['limit_tolerance_rad']))
   controls.append(dict(stage=stage,property=n,passed=data.get(n,{}).get('type')=='App::PropertyAngle'))
 poses={'loaded':zero,'zero':zero};poses.update({'individual_'+n:{**zero,n:v} for n,v in zip(names,cases['individual_degrees'])});combined=dict(zip(names,cases['combined_degrees']));poses['combined']=combined
 rows=[compare_state(s,poses[s['id']],urdf,cases) for s in first['states']];byid={s['id']:s for s in first['states']};axes=[];ref=expected(zero,urdf)
 for n,v in zip(names,cases['individual_degrees']):axes.append(axis_check(byid['zero'],byid['individual_'+n],n,v,ref['axes'][n],joints[n].find('child').get('link')))
 fresh=dict(zip(names,cases['reopened_degrees']));reopen_rows=[compare_state(s,combined if s['id']=='loaded' else fresh,urdf,cases) for s in second['states']]
 continuity=[]
 for group in ['links','visuals']:
  for n,m in byid['combined'][group].items():
   t,r=error(second['states'][0][group][n],m);continuity.append(dict(kind=group,name=n,translation_error_mm=t,rotation_error_deg=r,passed=t<=cases['reopen_translation_tolerance_mm'] and r<=cases['reopen_rotation_tolerance_deg']))
 geometry={stage:all(v['passed'] for v in p.get('geometry',{}).values()) and set(p.get('geometry',{}))==expected_names for stage,p in [('initial',first),('reopened',second)]}
 gates=dict(coverage=coverage,source_mesh_geometry=all(geometry.values()),native_controls_and_limits=all(c['passed'] for c in controls),independent_sampled_articulation=all(r['passed'] for r in rows),joint_axes=all(a['axis_direction_error_deg']<=.1 and a['axis_line_error_mm']<=.1 for a in axes),fresh_reopen_and_edit=all(r['passed'] for r in reopen_rows) and all(r['passed'] for r in continuity),native_dependencies_without_custom_proxies=not first.get('custom_proxies') and not second.get('custom_proxies'))
 return dict(passed=all(gates.values()),scope='Mesh-source CAD assembly and sampled articulation only; not detailed BRep assembly or collision-clearance certification',gates=gates,controls=controls,states=rows,joint_axes=axes,reopened_states=reopen_rows,reopen_continuity=continuity,geometry=geometry,mesh_solid_flags={n:v['is_solid_mesh'] for n,v in first['geometry'].items()},mesh_watertightness_passed=all(v['is_solid_mesh'] for v in first['geometry'].values()),mesh_not_solid_instances=[n for n,v in first['geometry'].items() if not v['is_solid_mesh']],qualification='Mesh assembly/articulation only. A passed contract does not certify watertight geometry, BRep solids, manufacturing suitability or collision clearance.',brep_solid_validity='not_applicable_mesh_candidate',collision_clearance='not_assessed',out_of_range_native_enforcement='not_assessed')
def main():
 parser=argparse.ArgumentParser();parser.add_argument('--candidate',required=True);parser.add_argument('--out',required=True);parser.add_argument('--urdf',default=str(ROOT/'cad/projects/so-arm101/upstream/Simulation/SO101/so101_new_calib.urdf'));parser.add_argument('--timeout',type=int,default=240);args=parser.parse_args()
 if not 10<=args.timeout<=600:parser.error('timeout must be10..600 seconds per process')
 output=Path(args.out).resolve();output.mkdir(parents=True,exist_ok=False);candidate=Path(args.candidate).resolve()
 if not candidate.is_file():
  result=dict(status='failed',passed=False,candidate=str(candidate),gates={'artifact_present':False},error='Required assembly.FCStd is missing; candidate execution did not deliver an artifact',other_checks='not run because artifact is missing')
  (output/'result.json').write_text(json.dumps(result,indent=2));print(json.dumps(result,indent=2));return 1
 urdf=Path(args.urdf).resolve();cases=json.loads((HERE/'cases.json').read_text());source_hash=sha(candidate);names=cases['joint_order'];zero=dict.fromkeys(names,0.)
 queries=[dict(id='zero',angles_degrees=zero)]+[dict(id='individual_'+n,angles_degrees={**zero,n:v}) for n,v in zip(names,cases['individual_degrees'])]+[dict(id='combined',angles_degrees=dict(zip(names,cases['combined_degrees'])))];fresh=[dict(id='reopened_edit',angles_degrees=dict(zip(names,cases['reopened_degrees'])))];report={'status':'running','candidate':str(candidate),'candidate_sha256':source_hash,'reference_urdf_sha256':sha(urdf),'evaluator_hashes':{p.name:sha(p) for p in HERE.iterdir() if p.suffix in ['.py','.json']},'processes':[]};start=time.time()
 def write(): (output/'result.json').write_text(json.dumps(report,indent=2))
 try:
  frozen=json.loads((HERE/'reference_manifest.json').read_text())
  for name,digest in frozen['files'].items():
   if sha(urdf.parent/name)!=digest:raise ValueError('frozen reference integrity mismatch: '+name)
  for stage,model,query in [('initial',candidate,queries),('reopened',output/'posed.FCStd',fresh)]:
   querypath=output/(stage+'-queries.json');querypath.write_text(json.dumps(query));cmd=[str(ROOT/'cad/freecad/run.sh'),str(HERE/'probe.py'),'--candidate',str(model),'--urdf',str(urdf),'--queries',str(querypath),'--output',str(output/(stage+'.json')),'--check-geometry']
   if stage=='initial':cmd+=['--save-state',str(output/'posed.FCStd')]
   t=time.time()
   with (output/(stage+'.log')).open('w') as log:run=subprocess.run(cmd,stdout=log,stderr=subprocess.STDOUT,timeout=args.timeout)
   report['processes'].append(dict(stage=stage,returncode=run.returncode,elapsed_seconds=time.time()-t));write()
   if run.returncode:raise RuntimeError(stage+' measurement failed; see retained log/JSON')
  first=json.loads((output/'initial.json').read_text());second=json.loads((output/'reopened.json').read_text());report.update(evaluate(first,second,urdf,cases));report['status']='complete'
 except Exception as exc:report.update(status='failed',passed=False,error=str(exc))
 report.update(candidate_unchanged=sha(candidate)==source_hash,elapsed_seconds=time.time()-start);report['passed']=report.get('passed',False) and report['candidate_unchanged'];write();print(json.dumps({k:report[k] for k in ['status','passed','candidate_unchanged','elapsed_seconds']},indent=2));return 0 if report['passed'] else 1
if __name__=='__main__':sys.exit(main())
