"""Real-kernel native render, depth occlusion, PNG and fail-closed budgets."""
import base64,json,os,struct,zlib
from pathlib import Path
import numpy as np
import FreeCAD as App
import native,worker
import render
OUT=Path(os.environ.get('WRIGHT_CAD_OUT',Path(__file__).resolve().parents[2]/'out/freecad-render')).resolve();OUT.mkdir(parents=True,exist_ok=True)
worker.document('RenderTests');doc=worker.active()
for name,z in [('Back',0),('Front',10)]:
 native.make_part(doc,dict(name=name,kind='box',params=dict(x=10,y=10,z=2)))
 doc.getObject(name).Placement.Base=App.Vector(30,-20,z)
doc.recompute();objects=worker.selected_objects(['Back','Front']);before=worker.geometry_fingerprint(objects)
raw_before={o.Name:o.Shape.exportBrepToString() for o in objects};count=len(doc.Objects)
def decode(result):
 blob=base64.b64decode(result['content_blocks'][0]['source']['data'],validate=True)
 assert blob[:8]==b'\x89PNG\r\n\x1a\n';pos=8;compressed=b'';width=height=None
 while pos<len(blob):
  size=struct.unpack_from('>I',blob,pos)[0];kind=blob[pos+4:pos+8];data=blob[pos+8:pos+8+size];crc=struct.unpack_from('>I',blob,pos+8+size)[0]
  assert zlib.crc32(kind+data)&0xffffffff==crc
  if kind==b'IHDR':width,height,depth,color,comp,filtering,interlace=struct.unpack('>IIBBBBB',data);assert (depth,color,comp,filtering,interlace)==(8,2,0,0,0)
  if kind==b'IDAT':compressed+=data
  pos+=12+size
  if kind==b'IEND':break
 assert pos==len(blob)
 raw=zlib.decompress(compressed);assert len(raw)==height*(1+width*3)
 rows=np.frombuffer(raw,dtype=np.uint8).reshape(height,1+width*3);assert not rows[:,0].any()
 return rows[:,1:].reshape(height,width,3),blob
options=dict(width=256,height=192,azimuth_degrees=0,elevation_degrees=90)
both=worker.METHODS['render'](objects=['Back','Front'],**options);pixels,png=decode(both)
front=worker.render(['Front'],**options);front_pixels,_=decode(front)
back=worker.render(['Back'],**options);back_pixels,_=decode(back)
assert pixels.shape==(192,256,3) and np.array_equal(pixels,front_pixels)
assert not np.array_equal(front_pixels[96,128],back_pixels[96,128])
reverse=worker.render(['Front','Back'],**options)
assert np.array_equal(pixels,decode(reverse)[0]),'draw order changed occlusion'
assert both['bounds_mm']==[[30,-20,0],[40,-10,12]],both['bounds_mm']
assert both['triangles']==24 and both['visible_pixels']>1000
assert both['geometry_fingerprint']==before==worker.geometry_fingerprint(objects)
assert len(doc.Objects)==count and all(o.Shape.exportBrepToString()==raw_before[o.Name] for o in objects)
invalid=[dict(angular_deflection_radians=0),dict(angular_deflection_radians=1.01),dict(angular_deflection_radians=True),dict(angular_deflection_radians=float('nan')),dict(width=True),dict(width=63),dict(width=1025),dict(height=1.5),dict(azimuth_degrees=float('inf')),dict(elevation_degrees=91),dict(deflection_mm=0),dict(max_triangles=0),dict(max_triangles=100001),dict(max_triangles=1),dict(objects=[]),dict(objects=['Back','Back']),dict(objects=['missing']),dict(objects=['Back']*129)]
for case in invalid:
 args=dict(objects=['Back'],width=128,height=128);args.update(case)
 try:worker.render(**args)
 except ValueError:pass
 else:raise AssertionError(('invalid render accepted',case))
# Angular control measurably changes a curved display mesh, never its native solid.
native.make_part(doc,dict(name='Round',kind='cylinder',params=dict(radius=10,height=12)))
round_before=doc.Round.Shape.exportBrepToString()
coarse=worker.render(['Round'],width=128,height=128,angular_deflection_radians=.8)
fine=worker.render(['Round'],width=128,height=128,angular_deflection_radians=.2)
assert fine['triangles']>coarse['triangles']
assert doc.Round.Shape.exportBrepToString()==round_before
assert fine['parts'][0]['angular_deflection_radians']==.2
# Large projected workloads reject before raster allocation/iteration.
tri=np.array([[[-1,-1,0],[1,-1,0],[0,1,0]]]*100)
try:render.rasterize(tri,[(255,0,0)]*100,1024,1024,0,90)
except ValueError as e:assert 'budget' in str(e)
else:raise AssertionError('unbounded projected work accepted')
native.set_parameter(doc,'Front','Height',3)
changed=worker.render(['Back','Front'],**options)
assert changed['geometry_fingerprint']!=before
(OUT/'occlusion.png').write_bytes(png)
summary=dict(passed=True,native_png_dimensions=[256,192],png_crc=True,depth_occlusion=True,draw_order_invariant=True,translated_world_bounds=True,source_geometry_unchanged=True,invalid_inputs_rejected=len(invalid),projected_budget=True,edit_fingerprint_changes=True,angular_control_changes_display_only=True)
(OUT/'render-tests.json').write_text(json.dumps(summary,indent=2));print(json.dumps(summary))
App.closeDocument(doc.Name)
