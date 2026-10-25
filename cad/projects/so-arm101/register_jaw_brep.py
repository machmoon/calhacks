"""Bounded one-asset rigid registration: official jaw STEP to official URDF STL.

Preserves the exact imported BRep; no mesh-to-solid conversion or assembly-pose
fitting. Surface agreement is sampled, not a continuous equivalence proof.
"""
import hashlib,itertools,json,time
from pathlib import Path
import numpy as np
from scipy.spatial import cKDTree
from scipy.spatial.transform import Rotation
import FreeCAD as App
import Part,Mesh
ROOT=Path(__file__).resolve().parents[3];PROJECT=Path(__file__).resolve().parent;OUT=ROOT/'out/so-arm101-brep-jaw-probe';OUT.mkdir(parents=True,exist_ok=True)
STEP=PROJECT/'upstream/STEP/SO101/Follower_Specific/Moving_Jaw_SO101.step';STL=PROJECT/'upstream/Simulation/SO101/assets/moving_jaw_so101_v1.stl';started=time.time();rng=np.random.default_rng(101)
source=Part.read(str(STEP));assert source.isValid() and len(source.Solids)==1
mesh=Mesh.Mesh(str(STL));scale=App.Matrix();scale.A11=scale.A22=scale.A33=1000;mesh.transform(scale)
def topology(shape):
 vertices,faces=shape.tessellate(.03);return np.array([[p.x,p.y,p.z] for p in vertices])[np.array(faces)]
def sample(tris,count):
 cross=np.cross(tris[:,1]-tris[:,0],tris[:,2]-tris[:,0]);area=np.linalg.norm(cross,axis=1);valid=area>1e-12;tris=tris[valid];cross=cross[valid];area=area[valid]
 chosen=rng.choice(len(tris),count,p=area/area.sum());a=rng.random((count,1));b=rng.random((count,1));u=np.sqrt(a);points=(1-u)*tris[chosen,0]+u*(1-b)*tris[chosen,1]+u*b*tris[chosen,2]
 return points,cross[chosen]/area[chosen,None]
source_tri=topology(source);v,f=mesh.Topology;target_tri=np.array([[p.x,p.y,p.z] for p in v])[np.array(f)]
x,_=sample(source_tri,4000);target,normals=sample(target_tri,40000);tree=cKDTree(target)
def pca(points):
 center=points.mean(axis=0);_,_,vt=np.linalg.svd(points-center,full_matrices=False);return center,vt.T
cx,sx=pca(x);ct,st=pca(target)
def align(a,b):
 ca=a.mean(axis=0);cb=b.mean(axis=0);u,_,vt=np.linalg.svd((a-ca).T@(b-cb));r=vt.T@u.T
 if np.linalg.det(r)<0:vt[-1]*=-1;r=vt.T@u.T
 return r,cb-r@ca
best=None;starts=0
for perm in itertools.permutations(range(3)):
 for signs in itertools.product([-1,1],repeat=3):
  matrix=np.eye(3)[:,perm]@np.diag(signs);r=st@matrix@sx.T
  if np.linalg.det(r)<0:continue
  t=ct-r@cx;starts+=1
  for _ in range(25):
   points=x@r.T+t;dist,index=tree.query(points);keep=dist<=np.quantile(dist,.9);delta,shift=align(points[keep],target[index[keep]]);r=delta@r;t=delta@t+shift
  score=float(np.mean(tree.query(x@r.T+t)[0]**2))
  if best is None or score<best[0]:best=(score,r,t)
score,r,t=best
# Point-to-plane refinement uses actual STL facets, with bounded iterations.
fit,_=sample(source_tri,12000)
for _ in range(50):
 points=fit@r.T+t;dist,index=tree.query(points);keep=dist<=np.quantile(dist,.95);p=points[keep];q=target[index[keep]];n=normals[index[keep]]
 system=np.concatenate([np.cross(p,n),n],axis=1);rhs=np.einsum('ij,ij->i',q-p,n);delta=np.linalg.lstsq(system,rhs,rcond=None)[0];rot=Rotation.from_rotvec(delta[:3]).as_matrix();r=rot@r;t=rot@t+delta[3:]
 if np.linalg.norm(delta)<1e-10:break
matrix=App.Matrix()
for i in range(3):
 for j in range(3):setattr(matrix,f'A{i+1}{j+1}',float(r[i,j]))
for i in range(3):setattr(matrix,f'A{i+1}4',float(t[i]))
aligned=Part.Shape(source);aligned.Placement=App.Placement(matrix).multiply(source.Placement)
assert aligned.isValid() and aligned.isPartner(source)
# isPartner proves shared native TShape geometry despite a different rigid location.
# Native point-to-BRep distance from independently sampled mesh points.
evaluation,_=sample(target_tri,1500);distances=[]
for p in evaluation:distances.append(Part.Vertex(App.Vector(*p)).distToShape(aligned)[0])
metrics=dict(samples=len(distances),max_mm=float(max(distances)),rms_mm=float(np.sqrt(np.mean(np.square(distances)))),p95_mm=float(np.quantile(distances,.95)),median_mm=float(np.median(distances)))
doc=App.newDocument('SO101_Jaw_BRep_Registration');obj=doc.addObject('Part::Feature','RegisteredJawBRep');obj.Label='Official jaw BRep registered to URDF mesh-local frame';obj.Shape=aligned;reference=doc.addObject('Mesh::Feature','OfficialJawMeshReference');reference.Mesh=mesh
if 'Visibility' in reference.PropertiesList:reference.Visibility=False
doc.recompute();native=OUT/'registered-jaw.FCStd';doc.saveAs(str(native))
report=dict(step_source=str(STEP),step_sha256=hashlib.sha256(STEP.read_bytes()).hexdigest(),stl_source=str(STL),stl_sha256=hashlib.sha256(STL.read_bytes()).hexdigest(),native=str(native),source_brep_valid=True,source_brep_solids=len(source.Solids),source_brep_faces=len(source.Faces),source_volume_mm3=source.Volume,aligned_volume_mm3=aligned.Volume,underlying_brep_exactly_preserved=True,shared_native_tshape_during_transform=aligned.isPartner(source),rigid_transform_source_step_to_urdf_mesh_local_mm=list(matrix.A),rotation_determinant=float(np.linalg.det(r)),pca_icp_starts=starts,mesh_points_to_native_brep=metrics,elapsed_seconds=time.time()-started,scope='One independent source-asset registration only; original BRep retained. Sampled distance is not a continuous surface equivalence bound, and no arm assembly or held-out pose fitting is used.',preassembled_step_used=False,mesh_to_brep_conversion=False)
(OUT/'registration.json').write_text(json.dumps(report,indent=2));print(json.dumps(report),flush=True)
