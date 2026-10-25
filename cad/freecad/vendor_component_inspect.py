"""Inspect actual supplier STEP geometry without substituting envelopes."""
import json,hashlib,base64
from pathlib import Path
import FreeCAD as App
import Part
import native,render,worker
ROOT=Path(__file__).resolve().parents[2];OUT=ROOT/'out/vendor-components';OUT.mkdir(exist_ok=True)
for name in ['mn2806evo','p12x4']:
 source=ROOT/'cad/components/vendor/tmotor'/(name+'.step')
 if not source.exists():continue
 shape=Part.read(str(source));print(name,'loaded',len(shape.Solids),'solids',len(shape.Faces),'faces',flush=True)
 b=native.shape_bounds(shape)
 record={'model':name,'source':str(source),'sha256':hashlib.sha256(source.read_bytes()).hexdigest(),'native_units':'mm (OpenCascade STEP normalization)','valid':shape.isValid(),'solids':len(shape.Solids),'faces':len(shape.Faces),'bounds_mm':[[b.XMin,b.YMin,b.ZMin],[b.XMax,b.YMax,b.ZMax]],'scope':'Actual manufacturer STEP geometry; import demonstration, not reference-VTOL substitution'}
 assert record['valid'] and record['solids']>0
 doc=App.newDocument('Vendor_'+name);obj=doc.addObject('Part::Feature','SupplierGeometry');obj.Shape=shape;doc.recompute()
 doc.saveAs(str(OUT/(name+'.FCStd')))
 try:
  result=render.selected([obj],worker.geometry_fingerprint,width=640,height=480,azimuth_degrees=-45,elevation_degrees=25,deflection_mm=1,max_triangles=100000)
  (OUT/(name+'.png')).write_bytes(base64.b64decode(result['content_blocks'][0]['source']['data']))
  record['render_triangles']=result['triangles'];record['preview']=str(OUT/(name+'.png'))
 except ValueError as e:record['render_error']=str(e)
 (OUT/(name+'.json')).write_text(json.dumps(record,indent=2));App.closeDocument(doc.Name)
 print(record,flush=True)
