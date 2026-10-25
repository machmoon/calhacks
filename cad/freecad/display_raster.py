"""Bounded Rust display rasterizer; never changes or validates CAD geometry."""
import hashlib,os,shutil,struct,subprocess
from pathlib import Path
import numpy as np
ROOT=Path(__file__).resolve().parents[2]
def rasterize(q,normals,colors,width,height):
 if os.environ.get('WRIGHT_RASTER_BACKEND')=='python':return None
 source=ROOT/'crates/wright-guide/src/bin/wright-raster.rs'
 compiler=shutil.which('rustc') or str(Path.home()/'.cargo/bin/rustc')
 if not Path(compiler).is_file():return None
 digest=hashlib.sha256(source.read_bytes()).hexdigest()[:20]
 cache=ROOT/'out/display-raster';cache.mkdir(parents=True,exist_ok=True)
 binary=cache/('wright-raster-'+digest)
 if not binary.is_file():
  temporary=cache/(binary.name+f'.{os.getpid()}.tmp')
  try:
   subprocess.run([compiler,'-O','--edition=2021',str(source),'-o',str(temporary)],check=True,timeout=60,stdout=subprocess.PIPE,stderr=subprocess.PIPE)
   temporary.replace(binary)
  finally:temporary.unlink(missing_ok=True)
 triangles=len(q)
 if not 0<triangles<=500000:raise ValueError('Raster triangle budget exceeded')
 payload=np.concatenate((q.reshape(triangles,9),normals.reshape(triangles,9),colors.reshape(triangles,3)),axis=1).astype('<f8')
 if not np.isfinite(payload).all():raise ValueError('Nonfinite raster payload')
 result=subprocess.run([str(binary)],input=struct.pack('<III',width,height,triangles)+payload.tobytes(),stdout=subprocess.PIPE,stderr=subprocess.PIPE,timeout=90)
 if result.returncode or len(result.stdout)!=width*height*3:raise ValueError('Rust rasterizer failed: '+result.stderr.decode(errors='replace')[:500])
 return np.frombuffer(result.stdout,dtype=np.uint8).reshape(height,width,3)
