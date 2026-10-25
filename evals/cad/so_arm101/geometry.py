"""Source triangulation check independent of candidate import/assembly code."""
import collections,hashlib,itertools,math,struct
from pathlib import Path
TOLERANCE_MM=1e-4
_cache={}
def source_data(path):
 path=str(Path(path).resolve())
 if path in _cache:return _cache[path]
 data=Path(path).read_bytes();n=struct.unpack_from('<I',data,80)[0]
 if len(data)!=84+50*n:raise ValueError('frozen source must be binary STL')
 points=[];ids={};faces=[];buckets=collections.defaultdict(list)
 for i in range(n):
  face=[]
  for j in range(3):
   p=tuple(v*1000 for v in struct.unpack_from('<fff',data,84+50*i+12+12*j))
   if p not in ids:
    ids[p]=len(points);points.append(p);buckets[tuple(math.floor(x/TOLERANCE_MM) for x in p)].append(ids[p])
   face.append(ids[p])
  faces.append(tuple(sorted(face)))
 result=points,collections.Counter(faces),buckets,hashlib.sha256(data).hexdigest();_cache[path]=result;return result

def check_mesh(mesh,path):
 points,expected,buckets,sha=source_data(path);vertices,faces=mesh.Topology;mapping=[];max_error=0.;missing=0
 for v in vertices:
  p=(float(v.x),float(v.y),float(v.z));cell=tuple(math.floor(x/TOLERANCE_MM) for x in p);best=None;distance=float('inf')
  for delta in itertools.product((-1,0,1),repeat=3):
   for i in buckets.get(tuple(cell[k]+delta[k] for k in range(3)),[]):
    d=sum((p[k]-points[i][k])**2 for k in range(3))
    if d<distance:best=i;distance=d
  if best is None or distance>TOLERANCE_MM**2:missing+=1;mapping.append(-1)
  else:mapping.append(best);max_error=max(max_error,math.sqrt(distance))
 actual=collections.Counter(tuple(sorted(mapping[i] for i in f)) for f in faces)
 return dict(passed=missing==0 and actual==expected,source_sha256=sha,kind='mesh',source_triangles=sum(expected.values()),candidate_triangles=len(faces),unmatched_vertices=missing,max_vertex_error_mm=max_error,triangle_multiset_matches=actual==expected,tolerance_mm=TOLERANCE_MM,brep_credit=False)

def fingerprint(mesh):
 vertices,faces=mesh.Topology;h=hashlib.sha256()
 for v in vertices:h.update(struct.pack('<ddd',v.x,v.y,v.z))
 for f in faces:h.update(struct.pack('<III',*f))
 return h.hexdigest()
