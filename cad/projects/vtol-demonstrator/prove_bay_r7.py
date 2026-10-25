"""Source-preserving packaging proof; no installation/service/strength claim."""
import FreeCAD as A,Part,json,hashlib,importlib.util,sys,zipfile
from pathlib import Path
import native
ROOT=Path(__file__).resolve().parents[3];out=ROOT/'out/vtol-demonstrator-r7';source=out/'assembly.FCStd';m=json.loads((out/'artifact-manifest.json').read_text());mp=Path(m['proxy_module']);spec=importlib.util.spec_from_file_location(mp.stem,mp);module=importlib.util.module_from_spec(spec);sys.modules[mp.stem]=module;spec.loader.exec_module(module)
before=hashlib.sha256(source.read_bytes()).hexdigest();d=A.openDocument(str(source));proposal_path=ROOT/'cad/projects/vtol-demonstrator/electronics-layout-candidate.json';proposal=json.loads(proposal_path.read_text());physical={n:d.getObject(n).Shape for n in m['physical']};bounds={n:native.shape_bounds(s) for n,s in physical.items()};rows=[];shapes={}
for row in proposal['components']:
 size=row['reservation_size_mm'];xyz=row['reservation_lower_xyz_mm'];shape=Part.makeBox(*size,A.Vector(*xyz));expanded=Part.makeBox(*(s+2 for s in size),A.Vector(*(v-1 for v in xyz)));shapes[row['id']]=shape
 record={'id':row['id'],'size_mm':size,'lower_xyz_mm':xyz,'outside_cavity_mm3':shape.cut(d.PodCavity.Shape).Volume,'one_mm_expanded_outside_cavity_mm3':expanded.cut(d.PodCavity.Shape).Volume,'one_mm_expanded_physical_collisions':[],'pod_gap_mm':shape.distToShape(d.Pod.Shape)[0],'hatch_gap_mm':shape.distToShape(d.Hatch.Shape)[0]}
 for name,other in physical.items():
  if not expanded.BoundBox.intersect(bounds[name]):continue
  volume=expanded.common(other).Volume
  if volume>1e-6:record['one_mm_expanded_physical_collisions'].append({'object':name,'volume_mm3':volume})
 record['pass_']=record['one_mm_expanded_outside_cavity_mm3']<1e-6 and not record['one_mm_expanded_physical_collisions'];rows.append(record);print(record,flush=True)
pairs=[]
for i,(a,sa) in enumerate(shapes.items()):
 for b,sb in list(shapes.items())[i+1:]:pairs.append({'pair':[a,b],'common_mm3':sa.common(sb).Volume,'gap_mm':sa.distToShape(sb)[0]})
changed=[]
for name in ['Pod','Hatch']:
 for other in m['physical']:
  if other==name or (name=='Hatch' and other=='Pod'):continue
  if not bounds[name].intersect(bounds[other]):continue
  volume=physical[name].common(physical[other]).Volume
  if volume>1e-6:changed.append({'pair':[name,other],'volume_mm3':volume})
previous=ROOT/'out/vtol-demonstrator-r6/assembly.FCStd';za=zipfile.ZipFile(previous);zb=zipfile.ZipFile(source);unchanged=[];different=[]
for n in m['physical']:
 key=n+'.Shape.brp'
 if key in za.namelist() and key in zb.namelist() and za.read(key)==zb.read(key):unchanged.append(n)
 else:different.append(n)
result={'native_sha256':before,'source_unchanged':before==hashlib.sha256(source.read_bytes()).hexdigest(),'candidate_layout_sha256':hashlib.sha256(proposal_path.read_bytes()).hexdigest(),'pod_half_height_mm':d.DesignParameters.PodHalfHeight.Value,'components':rows,'component_pairs':pairs,'changed_pod_hatch_collisions':changed,'all_physical_single_valid':all(s.isValid() and len(s.Solids)==1 for s in physical.values()),'exact_brep_unchanged_from_r6':unchanged,'brep_different_from_r6':different,'passed_packaging':all(r['pass_'] for r in rows) and all(p['common_mm3']<1e-6 for p in pairs) and not changed,'scope':'Dimensional reservations expanded1mm, not actual connector/tool/service trajectories. Existing tray is not rear battery support; installation-ready false.'};(out/'bay-proof.json').write_text(json.dumps(result,indent=2));print('packaging',result['passed_packaging'],flush=True)
