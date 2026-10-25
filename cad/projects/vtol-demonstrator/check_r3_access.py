import FreeCAD as A,Part,json,math
from pathlib import Path
import vtol_features_r3 as features
out=Path(__file__).resolve().parents[3]/'out/vtol-demonstrator-r3';out.mkdir(exist_ok=True)
d=A.newDocument();p=d.addObject('App::FeaturePython','Parameters');p.addProperty('App::PropertyLength','ClampBoltOffset');p.ClampBoltOffset=25
for n in ['Upper','Lower']:
 o=d.addObject('Part::FeaturePython',n);features.LiftMount(o,n);o.Parameters=p
d.recompute();rows=[]
for sign in [-1,1]:
 y=sign*25;tool=Part.makeCylinder(3,200,A.Vector(0,y,56));bolt=Part.makeCylinder(1.5,25,A.Vector(0,y,28)).fuse(Part.makeCylinder(2.75,3,A.Vector(0,y,53)))
 r=5.5/math.sqrt(3);pts=[A.Vector(r*math.cos(i*math.pi/3),y+r*math.sin(i*math.pi/3),28.1) for i in range(6)];nut=Part.Face(Part.makePolygon(pts+pts[:1])).extrude(A.Vector(0,0,2.4)).cut(Part.makeCylinder(1.5,2.4,A.Vector(0,y,28.1)))
 for label,a,b in [('driver_upper',tool,d.Upper.Shape),('bolt_upper',bolt,d.Upper.Shape),('bolt_lower',bolt,d.Lower.Shape),('nut_lower',nut,d.Lower.Shape),('bolt_nut',bolt,nut)]:
  rows.append(dict(side=sign,pair=label,common_mm3=a.common(b).Volume,distance_mm=a.distToShape(b)[0]));print(rows[-1],flush=True)
r=dict(revision='layout-r3-tool-access',upper_valid=d.Upper.Shape.isValid(),lower_valid=d.Lower.Shape.isValid(),split_gap_mm=d.Upper.Shape.distToShape(d.Lower.Shape)[0],tool_diameter_mm=6,tool_path_length_mm=200,checks=rows,limitations=['Local upper/lower clamp and fastener checks only; complete aircraft tool sweep checked separately','Thread forms omitted; no preload, torque, material or strength certification'])
(out/'local-access.json').write_text(json.dumps(r,indent=2))
