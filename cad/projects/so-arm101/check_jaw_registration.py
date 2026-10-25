"""Read-only reverse sampled surface-distance check and native BRep preview."""
import hashlib,json
from pathlib import Path
import numpy as np
from scipy.spatial import cKDTree
import FreeCAD as App
import Part
import render
ROOT=Path(__file__).resolve().parents[3];OUT=ROOT/'out/so-arm101-brep-jaw-probe';path=OUT/'registered-jaw.FCStd';digest=hashlib.sha256(path.read_bytes()).hexdigest();doc=App.openDocument(str(path));shape=doc.RegisteredJawBRep.Shape;mesh=doc.OfficialJawMeshReference.Mesh
v,f=mesh.Topology;tri=np.array([[p.x,p.y,p.z] for p in v])[np.array(f)];cross=np.cross(tri[:,1]-tri[:,0],tri[:,2]-tri[:,0]);tri=tri[np.linalg.norm(cross,axis=1)>1e-12];tree=cKDTree(tri.mean(axis=1))
v,f=shape.tessellate(.03);bt=np.array([[p.x,p.y,p.z] for p in v])[np.array(f)];areas=np.linalg.norm(np.cross(bt[:,1]-bt[:,0],bt[:,2]-bt[:,0]),axis=1);rng=np.random.default_rng(102);chosen=rng.choice(len(bt),1500,p=areas/areas.sum());u=np.sqrt(rng.random((1500,1)));w=rng.random((1500,1));points=(1-u)*bt[chosen,0]+u*(1-w)*bt[chosen,1]+u*w*bt[chosen,2]
# Move tessellation sample points onto the actual native BRep before measuring.
points=np.array([list(Part.Vertex(App.Vector(*p)).distToShape(shape)[1][0][1]) for p in points])
_,indices=tree.query(points,k=32);near=tri[indices];p=points[:,None,:];a,b,c=near[:,:,0],near[:,:,1],near[:,:,2];ab=b-a;ac=c-a;ap=p-a
dot=lambda x,y:np.einsum('...i,...i->...',x,y)
d00=dot(ab,ab);d01=dot(ab,ac);d11=dot(ac,ac);d20=dot(ap,ab);d21=dot(ap,ac);den=d00*d11-d01*d01
valid=den>1e-20;v=np.divide(d11*d20-d01*d21,den,out=np.zeros_like(den),where=valid);w=np.divide(d00*d21-d01*d20,den,out=np.zeros_like(den),where=valid);projected=a+v[:,:,None]*ab+w[:,:,None]*ac
plane=dot(p-projected,p-projected);plane=np.where(valid&(v>=0)&(w>=0)&(v+w<=1),plane,np.inf)
def segment(x,y):
 edge=y-x;t=np.clip(dot(p-x,edge)/dot(edge,edge),0,1);q=x+t[:,:,None]*edge;return dot(p-q,p-q)
d2=np.minimum(plane,np.minimum(segment(a,b),np.minimum(segment(b,c),segment(c,a))));dist=np.sqrt(d2.min(axis=1))
assert np.isfinite(dist).all()
metrics=dict(samples=len(dist),candidate_facets_per_sample=32,max_upper_bound_mm=float(dist.max()),rms_upper_bound_mm=float(np.sqrt(np.mean(dist**2))),p95_upper_bound_mm=float(np.quantile(dist,.95)),median_upper_bound_mm=float(np.median(dist)),scope='Distance to nearest of32candidate STL facets; a conservative upper bound on sampled point-to-entire-mesh distance, not a continuous surface bound')
triangles=bt.tolist();rgb,stats=render.rasterize(triangles,[(60,139,157)]*len(triangles),800,600,38,24);(OUT/'registered-jaw-brep.png').write_bytes(render.png_rgb(rgb))
assert shape.isValid() and hashlib.sha256(path.read_bytes()).hexdigest()==digest
(OUT/'reverse-distance-and-preview.json').write_text(json.dumps(dict(native_sha256=digest,native_unchanged=True,native_reopened_valid=True,brep_points_to_stl=metrics,preview=str(OUT/'registered-jaw-brep.png'),preview_stats=stats,source_geometry_kind='original analytic BRep, tessellated only for display'),indent=2));print(json.dumps(metrics),flush=True)
