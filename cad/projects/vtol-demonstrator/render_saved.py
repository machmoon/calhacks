"""Model-derived views of an immutable saved revision; no geometry authoring."""
import argparse,hashlib,importlib.util,json,sys
from pathlib import Path
import FreeCAD as A
import surface_preview,preview_views
p=argparse.ArgumentParser();p.add_argument('--output-dir',required=True);args=p.parse_args();out=Path(args.output_dir).resolve();m=json.loads((out/'artifact-manifest.json').read_text());module_path=Path(m['proxy_module']);spec=importlib.util.spec_from_file_location(module_path.stem,module_path);module=importlib.util.module_from_spec(spec);sys.modules[module_path.stem]=module;spec.loader.exec_module(module)
source=out/'assembly.FCStd';before=hashlib.sha256(source.read_bytes()).hexdigest();d=A.openDocument(str(source));names=m['collision_objects'];objects=[d.getObject(n) for n in names];r={'source_sha256':before,'script_sha256':hashlib.sha256(Path(__file__).read_bytes()).hexdigest(),'scope':'Actual native-BRep diagnostic views, not qualification of omitted hardware/service/load requirements'}
r['assembled']=surface_preview.selected(objects,out/'assembled.png',width=640,height=480,azimuth=-45,elevation=28)
# Remove only original pod shell material from a copy, preserving the actual
# fused internal frames/bosses/post and final holes rather than raw helper solids.
temp=A.newDocument('InteriorDiagnostic');interior=temp.addObject('Part::Feature','PodInternalMaterial');interior.Shape=d.Pod.Shape.cut(d.PodShell.Shape);temp.recompute()
inner=[o for o in objects if o.Name not in ['RightWingSkin','LeftWingSkin','Pod','Hatch']]+[interior]
r['internals']=surface_preview.selected(inner,out/'internals.png',width=640,height=480,azimuth=-45,elevation=28);A.closeDocument(temp.Name)
r.update(preview_views.generate(d,names,out));r['source_unchanged']=before==hashlib.sha256(source.read_bytes()).hexdigest();assert r['source_unchanged'];(out/'render-results.json').write_text(json.dumps(r,indent=2));print('Saved assembled, internal-material, cutaway and exploded views',flush=True)
