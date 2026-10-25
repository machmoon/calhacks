import FreeCAD as A,json,sys,importlib.util
from pathlib import Path
import surface_preview
out=Path(sys.argv[1]).resolve();m=json.loads((out/'artifact-manifest.json').read_text());p=Path(m['proxy_module']);s=importlib.util.spec_from_file_location(p.stem,p);mod=importlib.util.module_from_spec(s);sys.modules[p.stem]=mod;s.loader.exec_module(mod);d=A.openDocument(str(out/'assembly.FCStd'));r=surface_preview.selected([d.getObject(n) for n in m['collision_objects']],out/'rear.png',width=960,height=640,azimuth=180,elevation=8);(out/'rear-render.json').write_text(json.dumps(r,indent=2))
