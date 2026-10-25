"""Native hollow wing study with linked ribs; not a flight-ready wing."""
import json, hashlib, os
from pathlib import Path
import FreeCAD as App
import native, worker

root=Path(__file__).resolve().parents[2]
source=root/'out/cad-seated-mounts/plan.json'
plan=json.loads(source.read_text());items={i['name']:i for i in plan['instances']}
out=Path(os.environ['WRIGHT_CAD_OUT']).resolve();out.mkdir(parents=True,exist_ok=True)
worker.document('HollowWingStudy');doc=worker.active()
profile=items['part_wing_R']['params']['rings'][0]
native.make_part(doc,dict(name='Skin',kind='extruded_skin',params=dict(profile=profile,direction=[0,600,0],wall=.45)))
physical=['Skin'];contacts=[]
for short,source_name in [('MainSpar','part_main_spar_R'),('Joiner','part_joiner_R'),('RearSpar','part_rear_spar_R')]:
    p=items[source_name]['params'];start,end=native.vector(p['start']),native.vector(p['end']);axis=end-start
    params=dict(radius=p['radius'],height=axis.Length);kind='cylinder'
    if p['bore_radius']>0:kind='tube';params['bore_radius']=p['bore_radius']
    native.make_part(doc,dict(name=short,kind=kind,params=params))
    doc.getObject(short).Placement=App.Placement(start,App.Rotation(App.Vector(0,0,1),axis));physical.append(short)
for i,y in enumerate([1,120,240,360,470]):
    name=f'Rib{i+1}';radius=6.35 if y<150 else 5
    bores=[[[58.8,-1,8.518373982766182],[58.8,3,8.518373982766182],radius],[[130.2,-1,7.227442696259945],[130.2,3,7.227442696259945],1.5]]
    native.make_part(doc,dict(name=name,kind='profile_plate',params=dict(profile_object='Skin_InnerProfile',direction=[0,1,0],thickness=2,bores=bores)))
    doc.getObject(name).Placement.Base=App.Vector(0,y,0);physical.append(name)
    contacts.extend([[name,'Skin'],[name,'Joiner' if y<150 else 'MainSpar'],[name,'RearSpar']])
contacts.append(['Joiner','MainSpar'])
doc.recompute()
fit=worker.check(physical,contacts)
exports=worker.save(str(out),physical)
result=dict(scope='Native hollow wing structural geometry study; provisional rib layout, not flight or manufacturing approval',source_sha256=hashlib.sha256(source.read_bytes()).hexdigest(),physical_parts=[native.describe(doc.getObject(n)) for n in physical],fit=fit,exports=exports,skin_volume_mm3=doc.Skin.Shape.Volume,envelope_volume_mm3=doc.Skin_Outer.Shape.Volume,provisional_inputs=dict(rib_starts_y_mm=[1,120,240,360,470],rib_thickness_mm=2,skin_wall_mm=.45),wing_complete=False,missing=['Load and buckling validation','Print segmentation and joining','Adhesive and fastener design','Control surfaces and actuators','Root attachment and tip closure','Material and tolerance validation'],known_source_gap_mm=.105)
(out/'result.json').write_text(json.dumps(result,indent=2,allow_nan=False))
print(json.dumps(dict(solids=len(physical),skin_volume_mm3=result['skin_volume_mm3'],clashes=sum(p['clash'] for p in fit['pairs']),floating=sum(p['floating'] for p in fit['pairs']))),flush=True)
App.closeDocument(doc.Name)
