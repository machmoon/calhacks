"""Read-only local checks and actual BRep views of R8; not full qualification."""
import FreeCAD as A,Part,json,sys,hashlib,importlib.util
from pathlib import Path
import surface_preview
out=Path(sys.argv[1]).resolve();m=json.loads((out/'artifact-manifest.json').read_text());mp=Path(m['proxy_module']);sp=importlib.util.spec_from_file_location(mp.stem,mp);mod=importlib.util.module_from_spec(sp);sys.modules[mp.stem]=mod;sp.loader.exec_module(mod)
p=out/'assembly.FCStd';before=hashlib.sha256(p.read_bytes()).hexdigest();d=A.openDocument(str(p));objects=[d.getObject(n) for n in m['collision_objects']];r={'source_sha256':before,'checks':{},'scope':'New appearance geometry against existing collision objects; full independent qualification pending'}
r['renders']={}
for name,az in [('rear-three-quarter',135),('assembled',-45)]:
 r['renders'][name]=surface_preview.selected(objects,out/(name+'.png'),width=960,height=640,azimuth=az,elevation=24)
 r['source_unchanged']=before==hashlib.sha256(p.read_bytes()).hexdigest();(out/'appearance-check.json').write_text(json.dumps(r,indent=2));print(name,flush=True)
for n in ['TailSurfaceRight','TailSurfaceLeft','RuddervatorRight','RuddervatorLeft','WingletRight','WingletLeft','CruiseMotor','CruiseProp','CruiseFirewallCut3']:
 o=d.getObject(n);s=o.Shape;r['checks'][n]={'valid':s.isValid(),'solids':len(s.Solids),'volume_mm3':s.Volume,'interferences':[]}
 for other in objects:
  if other.Name==n:continue
  if not s.BoundBox.intersect(other.Shape.BoundBox):continue
  common=s.common(other.Shape).Volume
  if common>1e-5:r['checks'][n]['interferences'].append({'other':other.Name,'volume_mm3':common})
 print(n,r['checks'][n],flush=True)

(out/'appearance-check.json').write_text(json.dumps(r,indent=2))
