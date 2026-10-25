"""Same native geometry and warm tessellation: compare Python/Rust display only."""
import argparse,hashlib,importlib.util,json,os,sys,time
from pathlib import Path
import FreeCAD as App
import surface_preview
import numpy as np
from PIL import Image
p=argparse.ArgumentParser();p.add_argument('native');p.add_argument('manifest');p.add_argument('output');a=p.parse_args()
source=Path(a.native).resolve();out=Path(a.output).resolve();out.mkdir(parents=True,exist_ok=False)
sha=lambda p:hashlib.sha256(Path(p).read_bytes()).hexdigest()
before=sha(source);manifest=json.loads(Path(a.manifest).read_text());module=Path(manifest['proxy_module'])
assert before==manifest['native_sha256']
assert {v for k,v in manifest['generator_sources'].items() if Path(k).name==module.name}=={sha(module)}
spec=importlib.util.spec_from_file_location(module.stem,module);loaded=importlib.util.module_from_spec(spec);sys.modules[module.stem]=loaded;spec.loader.exec_module(loaded)
doc=App.openDocument(str(source));doc.recompute();objects=[doc.getObject(n) for n in manifest['collision_objects']];rows=[]
for name,backend in [('warmup','rust'),('python-reference','python'),('rust-cached','rust')]:
 os.environ['WRIGHT_RASTER_BACKEND']=backend
 start=time.monotonic();cpu=time.process_time()
 report=surface_preview.selected(objects,out/(name+'.png'),width=640,height=480,azimuth=-45,elevation=28,linear_deflection=.8,angular_deflection=.4)
 rows.append({'view':name,'wall_s':time.monotonic()-start,'parent_cpu_s':time.process_time()-cpu,'render':report})
 (out/'timings.json').write_text(json.dumps(rows,indent=2))
x=np.asarray(Image.open(out/'python-reference.png')).astype(int);y=np.asarray(Image.open(out/'rust-cached.png')).astype(int)
diff=int(abs(x-y).max())
result={'native_sha256':before,'source_unchanged':sha(source)==before,'max_pixel_channel_difference':diff,'passed':diff<=1 and sha(source)==before,'runs':rows,'scope':'Same native model, camera, resolution and tessellation. Parent CPU excludes Rust subprocess CPU; wall times include it. Warmup excluded from speed ratio.'}
result['warm_wall_speedup']=rows[1]['wall_s']/rows[2]['wall_s']
(out/'result.json').write_text(json.dumps(result,indent=2));print(json.dumps({k:v for k,v in result.items() if k!='runs'}))
assert result['passed']
