"""Replace filled aircraft wing envelopes with native skins and linked ribs.

The original interface requirements remain in the report, including contacts
that need redesigned attachments. No collision exemptions or tolerance changes.
"""
import json,os,hashlib,time
from pathlib import Path
import FreeCAD as App
import native,worker
root=Path(__file__).resolve().parents[2]
source=root/'out/cad-seated-mounts/plan.json'
plan=json.loads(source.read_text())
baseline=root/'out/freecad-aircraft/assembly.FCStd'
out=Path(os.environ['WRIGHT_CAD_OUT']).resolve();out.mkdir(parents=True,exist_ok=True)
worker.restore(str(baseline));doc=worker.active()
physical=[i['name'] for i in plan['instances']]
original={n:native.describe(doc.getObject(n)) for n in physical}
contacts={tuple(sorted([i['name'],other])) for i in plan['instances'] for other in i.get('touches',[])}
started=time.monotonic()
for side,sign in [('R',1),('L',-1)]:
    wing='part_wing_'+side
    old=doc.getObject(wing);sections=[o.Name for o in old.Sections]
    doc.removeObject(wing)
    for name in sections:doc.removeObject(name)
    doc.recompute()
    profile=next(i for i in plan['instances'] if i['name']==wing)['params']['rings'][0]
    native.make_part(doc,dict(name=wing,kind='extruded_skin',params=dict(profile=profile,direction=[0,sign*600,0],wall=.45)))
    for i,y in enumerate([1,120,240,360,470]):
        name=f'part_rib_{side}{i+1}';radius=6.35 if y<150 else 5
        bores=[[[58.8,-sign,8.518373982766182],[58.8,sign*3,8.518373982766182],radius],[[130.2,-sign,7.227442696259945],[130.2,sign*3,7.227442696259945],1.5]]
        native.make_part(doc,dict(name=name,kind='profile_plate',params=dict(profile_object=wing+'_InnerProfile',direction=[0,sign,0],thickness=2,bores=bores)))
        doc.getObject(name).Placement.Base=App.Vector(0,sign*y,0)
        physical.append(name)
        for other in [wing,'part_joiner_'+side if y<150 else 'part_main_spar_'+side,'part_rear_spar_'+side]:contacts.add(tuple(sorted([name,other])))
    print(f'{side} native skin and five linked ribs created',flush=True)
doc.recompute()
unchanged=[]
for name,expected in original.items():
    if name in ['part_wing_R','part_wing_L']:continue
    actual=native.describe(doc.getObject(name))
    passed=abs(actual['volume_mm3']-expected['volume_mm3'])<1e-5 and max(abs(a-b) for row,target in zip(actual['bbox_mm'],expected['bbox_mm']) for a,b in zip(row,target))<1e-5
    unchanged.append(dict(name=name,passed=passed))
assert all(c['passed'] for c in unchanged)
print('Checking all assembly pairs, retaining original contact requirements',flush=True)
fit=worker.check(physical,[list(p) for p in sorted(contacts)],require_connected=True)
exports=worker.save(str(out),physical)
result=dict(scope='Full aircraft structural geometry iteration: native wing skins and provisional linked ribs; original interface requirements preserved',source_plan_sha256=hashlib.sha256(source.read_bytes()).hexdigest(),baseline_sha256=hashlib.sha256(baseline.read_bytes()).hexdigest(),unchanged_parts=unchanged,physical_parts=[native.describe(doc.getObject(n)) for n in physical],fit=fit,exports=exports,elapsed_s=time.monotonic()-started,aircraft_complete=False,provisional_rib_stations_mm=[1,120,240,360,470],skin_wall_mm=.45,rib_thickness_mm=2)
(out/'result.json').write_text(json.dumps(result,indent=2,allow_nan=False))
print(json.dumps(dict(parts=len(physical),clashes=sum(p['clash'] for p in fit['pairs']),floating=sum(p['floating'] for p in fit['pairs']),elapsed_s=result['elapsed_s'])),flush=True)
App.closeDocument(doc.Name)
