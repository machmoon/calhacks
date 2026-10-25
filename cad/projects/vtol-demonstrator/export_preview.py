"""Bounded native tessellation for app preview, never a manufacturing export."""
import FreeCAD
import Mesh,MeshPart,hashlib

def generate(doc,names,out):
 mesh=Mesh.Mesh();rows=[]
 for name in names:
  o=doc.getObject(name);part=MeshPart.meshFromShape(Shape=o.Shape,LinearDeflection=.8,AngularDeflection=.4,Relative=False)
  rows.append(dict(object=name,triangles=part.CountFacets));mesh.addMesh(part)
  if mesh.CountFacets>200000:raise ValueError('Native preview mesh exceeds200000triangle budget')
 path=out/'assembly-preview.stl';mesh.write(str(path))
 return dict(path=str(path),sha256=hashlib.sha256(path.read_bytes()).hexdigest(),triangles=mesh.CountFacets,linear_deflection_mm=.8,angular_deflection_radians=.4,objects=rows,scope='Native physical and candidate COTS display mesh; not a certified printable/manufacturing export. FCStd is the editable source.')
if __name__=='__main__':
 import argparse,json,sys
 from pathlib import Path
 import FreeCAD as A
 p=argparse.ArgumentParser();p.add_argument('--output-dir',required=True);a=p.parse_args();out=Path(a.output_dir).resolve();sys.path.insert(0,str(out));import vtol_features_r3
 m=json.loads((out/'artifact-manifest.json').read_text());d=A.openDocument(str(out/'assembly.FCStd'));r=generate(d,m['collision_objects'],out);(out/'preview-mesh.json').write_text(json.dumps(r,indent=2));print(r['triangles'],flush=True)
