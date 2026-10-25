"""Capture orbit source media from saved native B-reps, without changing models."""
import hashlib,json,subprocess
from pathlib import Path
import FreeCAD as App
import render
ROOT=Path(__file__).resolve().parents[2]
OUT=ROOT/'demo/video-sources/2026-10-24'
for tag,folder in [('current-vtol-orbit','tsung-vtol-top-retention'),('hardpoint-orbit','tsung-vtol-wing-hardpoint')]:
 source=ROOT/'out'/folder/'assembly.FCStd'
 before=hashlib.sha256(source.read_bytes()).hexdigest()
 report=json.loads((source.parent/'result.json').read_text())
 print(tag, list(report),flush=True)
 names=report.get('physical') or report.get('physical_names')
 if not names:raise ValueError('Missing explicit physical selection')
 doc=App.openDocument(str(source));faces=[];colors=[]
 for name in sorted(names):
  shape=doc.getObject(name).Shape.copy().cleaned();verts,tris=shape.tessellate(.5)
  for tri in tris:
   faces.append([[verts[j].x,verts[j].y,verts[j].z] for j in tri]);colors.append(render.part_color(name))
 dest=OUT/'assets'/(tag+'.mp4')
 args=['/opt/homebrew/bin/ffmpeg','-v','error','-y','-f','rawvideo','-pix_fmt','rgb24','-s','640x480','-r','12','-i','-','-an','-c:v','libx264','-crf','18','-pix_fmt','yuv420p','-movflags','+faststart',str(dest)]
 proc=subprocess.Popen(args,stdin=subprocess.PIPE)
 for i in range(96):
  rgb,stats=render.rasterize(faces,colors,640,480,-52+360*i/96,24)
  proc.stdin.write(rgb.tobytes())
  if i==0:(OUT/'assets'/(tag+'.png')).write_bytes(render.png_rgb(rgb))
  if i%24==0:print(tag,i,flush=True)
 proc.stdin.close()
 if proc.wait():raise RuntimeError('Encoder failed')
 assert hashlib.sha256(source.read_bytes()).hexdigest()==before
 metadata={'id':tag,'title':'Current VTOL native CAD orbit' if tag.startswith('current') else 'Internal spar-to-boom hardpoint orbit','kind':'native_geometry_rendered_video','source':str(source),'source_sha256':before,'file':'assets/'+dest.name,'poster':'assets/'+tag+'.png','sha256':hashlib.sha256(dest.read_bytes()).hexdigest(),'duration_seconds':8,'width':640,'height':480,'fps':12,'physical_objects':names,'triangles':len(faces),'caption':'Native CAD render; proposed reconstruction','qualification':'Rendered from saved B-reps, not a live UI recording. Full aircraft attachments and structural sizing remain unfinished.','source_unchanged':True}
 (OUT/(tag+'.json')).write_text(json.dumps(metadata,indent=2))
 App.closeDocument(doc.Name)
 print('completed',tag,flush=True)
