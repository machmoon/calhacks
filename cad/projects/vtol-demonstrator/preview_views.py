"""Actual native-BRep diagnostic cutaway/exploded views; source is immutable."""
import FreeCAD as A,Part,surface_preview

def generate(doc,names,out,linear_deflection=.8,angular_deflection=.4):
 report={}
 for view in ['cutaway','exploded']:
  objects=[];clips={};translations={}
  for n in names:
   source=doc.getObject(n)
   if source is None or source.Shape.isNull():continue
   if view=='cutaway':
    if 'WingSkin' in n:clips[n]=(2,88)
    elif n=='Pod':clips[n]=(2,24)
    elif n=='Hatch':continue
   else:
    shift=(0,0,0)
    if any(k in n for k in ['WingSkin','Aileron','Rib','Spar']):shift=(0,200 if 'Right' in n else -200,70)
    elif n=='Hatch':shift=(0,0,180)
    elif n=='Pod' or 'Tray' in n:shift=(0,0,-100)
    elif n.startswith(('LiftMotor','LiftProp','ClampBolt')):shift=(0,0,100)
    translations[n]=shift
   objects.append(source)
  report[view]=surface_preview.selected(objects,out/(view+'.png'),width=640,height=480,azimuth=-45,elevation=30,linear_deflection=linear_deflection,angular_deflection=angular_deflection,clip_planes=clips,translations=translations)
  report[view]['scope']='Native tessellation with display-only clipping and translations; uncapped sections, not modified CAD or service-path proof'
 return report
if __name__=='__main__':
 import argparse,json,hashlib
 from pathlib import Path
 import vtol_features
 p=argparse.ArgumentParser();p.add_argument('--output-dir',required=True);a=p.parse_args();out=Path(a.output_dir).resolve();m=json.loads((out/'artifact-manifest.json').read_text());f=out/'assembly.FCStd';before=hashlib.sha256(f.read_bytes()).hexdigest();d=A.openDocument(str(f));r=generate(d,m['collision_objects'],out);r['source_unchanged']=before==hashlib.sha256(f.read_bytes()).hexdigest();r['source_sha256']=before;(out/'diagnostic-views.json').write_text(json.dumps(r,indent=2))
