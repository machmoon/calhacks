"""Proposed spar-to-boom hardpoint, not recovered author hardware.

Uses the actual provisional curved wing checkpoint. Local geometry checks do not
size the spar, printed web, clamp preload or skin reinforcement for flight loads.
"""
import base64,hashlib,json,math
from pathlib import Path
import FreeCAD as App
import Part
import native,worker,layout,booleans

ROOT=Path(__file__).resolve().parents[2]
OUT=ROOT/'out/tsung-vtol-wing-hardpoint';OUT.mkdir(parents=True,exist_ok=True)
SOURCE=ROOT/'out/tsung-vtol-wing-structure/unverified-build.FCStd'
source_hash=hashlib.sha256(SOURCE.read_bytes()).hexdigest()
worker.open_document(str(SOURCE));doc=worker.active()
layout.create(doc,'HardpointControls',{'BoomY':dict(value=220,min=210,max=230),
    'SplitGap':dict(value=.5,min=.2,max=.8)})

def make(name,kind,params,xyz=(0,0,0),axis=None,follow=True):
    native.make_part(doc,dict(name=name,kind=kind,params=params))
    obj=doc.getObject(name);obj.Placement.Base=App.Vector(*xyz)
    if axis:obj.Placement.Rotation=App.Rotation(App.Vector(0,0,1),App.Vector(*axis))
    if follow:obj.setExpression('Placement.Base.y',f'HardpointControls.BoomY + ({xyz[1]-220} mm)')
    doc.recompute();return obj

def op(name,operation,a,b):
    obj=doc.addObject('Part::'+operation,name);obj.Base=a;obj.Tool=b
    doc.recompute()
    assert obj.Shape.isValid() and len(obj.Shape.Solids)==1,name
    return obj

def hexagon(name,y,z,af,height):
    radius=af/math.sqrt(3)
    obj=make(name,'loft',dict(rings=[[[400+radius*math.cos(t*math.tau/6),
        y+radius*math.sin(t*math.tau/6),zz] for t in range(6)] for zz in [z,z+height]]),follow=False)
    for section in obj.Sections:
        offset=section.Placement.Base.y-220
        section.setExpression('Placement.Base.y',f'HardpointControls.BoomY + ({offset} mm)')
    doc.recompute();return obj

boom=make('AttachmentBoom','tube',dict(radius=5,bore_radius=4,height=700),(40,220,-10),(1,0,0))
collar=make('BoomCollar','cylinder',dict(radius=9,height=20),(390,220,-10),(1,0,0))
ears=make('BoomEars','box',dict(x=20,y=40,z=12),(390,200,-16))
blank=op('BoomClampBlank','Fuse',collar,ears)
up=make('UpperSpace','box',dict(x=30,y=50,z=60),(385,195,-9.75))
up.setExpression('Placement.Base.z','-10 mm + HardpointControls.SplitGap / 2')
down=make('LowerSpace','box',dict(x=30,y=50,z=11.75),(385,195,-22))
down.setExpression('Height','12 mm - HardpointControls.SplitGap / 2');doc.recompute()
upper=op('UpperBoomBlank','Common',blank,up)
lower=op('LowerBoomBlank','Common',blank,down)
web=make('SparWeb','box',dict(x=8,y=20,z=29.5),(396,210,-5))
upper=op('WebAndSaddle','Fuse',upper,web)
ring=make('SparCollarBlank','cylinder',dict(radius=4.5,height=20),(400,210,24.5),(0,1,0))
upper=op('HardpointBlank','Fuse',upper,ring)
boom_bore=make('BoomBore','cylinder',dict(radius=5,height=800),(0,220,-10),(1,0,0))
upper=op('HardpointBoomSeat','Cut',upper,boom_bore)
lower=op('LowerBoomSeat','Cut',lower,boom_bore)
spar_bore=make('HardpointSparBore','cylinder',dict(radius=2,height=40),(400,200,24.5),(0,1,0))
upper=op('HardpointSparSeat','Cut',upper,spar_bore)
bolts=[];nuts=[];keys=[]
for i,y in enumerate([206,234]):
    hole=make(f'HPDrill{i}','cylinder',dict(radius=1.7,height=30),(400,y,-25))
    upper=op(f'HPUpperDrill{i}','Cut',upper,hole)
    lower=op(f'HPLowerDrill{i}','Cut',lower,hole)
    pocket=hexagon(f'HPNutPocket{i}',y,-6.4,5.7,2.5)
    upper=op(f'HPNutSeat{i}','Cut',upper,pocket)
    nut_blank=hexagon(f'HPNutBlank{i}',y,-6.4,5.5,2.4)
    nut_bore=make(f'HPNutBore{i}','cylinder',dict(radius=1.5,height=4.4),(400,y,-7.4))
    nuts.append(op(f'HPNut{i}','Cut',nut_blank,nut_bore))
    shaft=make(f'HPShank{i}','cylinder',dict(radius=1.5,height=16),(400,y,-16))
    head=make(f'HPHead{i}','cylinder',dict(radius=2.75,height=3),(400,y,-19))
    screw=op(f'HPScrewBlank{i}','Fuse',shaft,head)
    bolts.append(op(f'HPScrew{i}','Cut',screw,hexagon(f'HPSocket{i}',y,-19.1,2.5,1.4)))
    keys.append(hexagon(f'HPHexKey{i}',y,-59,2.4,41.3))
print('Spar-to-boom hardpoint built; checking nominal load-path contacts',flush=True)
physical=[boom.Name,'RightSpar',upper.Name,lower.Name]+[o.Name for o in bolts+nuts]
contacts=[[boom.Name,upper.Name],[boom.Name,lower.Name],['RightSpar',upper.Name]]
for bolt,nut in zip(bolts,nuts):contacts.extend([[bolt.Name,lower.Name],[nut.Name,upper.Name],[bolt.Name,nut.Name]])
fit=worker.check(physical,contacts,tolerance_mm=.001,require_connected=True)
assert fit['passed'],[(p['a'],p['b'],p['overlap_mm3'],p['distance_mm']) for p in fit['pairs'] if p['clash'] or p['floating']]
assert abs(upper.Shape.distToShape(lower.Shape)[0]-.5)<1e-6
key_checks=[]
for key in keys:
    for name in physical:
        overlap=key.Shape.common(doc.getObject(name).Shape).Volume
        assert overlap<1e-6,(key.Name,name,overlap)
        key_checks.append(dict(tool=key.Name,part=name,overlap_mm3=overlap))
# Adjacent original ribs are included in the clearance audit, not assumed clear
# from their station centers alone.
rib_clearance=[]
for i in range(6):
    rib=doc.getObject(f'RightRib{i}')
    overlap=upper.Shape.common(rib.Shape).Volume
    gap=upper.Shape.distToShape(rib.Shape)[0]
    assert overlap<1e-6,(rib.Name,overlap)
    rib_clearance.append(dict(rib=rib.Name,overlap_mm3=overlap,distance_mm=gap))
print('Local fit passed; cutting explicit curved-skin passage',flush=True)
before_skin=doc.RightSkin.Shape.copy()
booleans.create(doc,'RightSkinWithHardpointPassage','cut','RightSkin',upper.Name,True)
cut_skin=doc.RightSkinWithHardpointPassage
removed=before_skin.Volume-cut_skin.Shape.Volume
assert removed>1e-6
assert cut_skin.Shape.common(upper.Shape).Volume<1e-6
assert cut_skin.Shape.distToShape(upper.Shape)[0]<1e-6
# Every removed bit lies in the local hardpoint; prove no unrelated skin change.
assert cut_skin.Shape.cut(before_skin).Volume<1e-6
outside=before_skin.cut(upper.Shape).cut(cut_skin.Shape).Volume
assert outside<1e-6
print('Skin passage verified; exporting local assembly',flush=True)
exports=worker.save(str(OUT),physical)
render=worker.render([upper.Name,lower.Name]+[o.Name for o in bolts+nuts],azimuth_degrees=-50,elevation_degrees=15)
(OUT/'hardpoint.png').write_bytes(base64.b64decode(render.pop('content_blocks')[0]['source']['data']))
saved_hash=hashlib.sha256(Path(exports['native']).read_bytes()).hexdigest()
native.set_parameter(doc,'HardpointControls','BoomY',225)
edited=worker.check(physical,contacts,tolerance_mm=.001,require_connected=True)
assert edited['passed']
native.set_parameter(doc,'HardpointControls','BoomY',220)
assert hashlib.sha256(SOURCE.read_bytes()).hexdigest()==source_hash
assert hashlib.sha256(Path(exports['native']).read_bytes()).hexdigest()==saved_hash
report=dict(scope='Proposed local spar-to-boom load-path geometry; engineering assumptions, not author CAD',
    source=str(SOURCE),source_sha256=source_hash,source_wing_checkpoint_unverified=True,
    physical=physical,fit=fit,edited_225mm_fit=edited,skin_removed_mm3=removed,
    preserved_skin_outside_hardpoint_mm3=outside,skin_output=cut_skin.Name,
    local_hex_key_checks=key_checks,rib_clearances=rib_clearance,exports=exports,
    fastener_dimension_evidence='out/tsung-vtol-tail-joint-v2/result.json:fastener_sources',
    load_path=['Wing spar bears in integral hardpoint collar','Integral printed web joins spar collar to boom saddle',
        'M3 screw/nut pair joins lower saddle to upper hardpoint','Radial boom seats contact before split faces bottom'],
    limitations=['Spar collar requires insertion of spar through the ring; axial retention/bonding not yet designed.',
        'No structural sizing: 4 mm spar, printed web, bearing stress, torsion and CFRP crushing remain unverified.',
        'Skin passage is zero-clearance geometry; adhesive/seal allowance and local reinforcement remain unresolved.',
        'Skin stays in the native document but is not included in the local physical STEP/GLB export.',
        'Local hardware envelopes omit thread helix, preload and manufacturing tolerance.',
        'Parent wing checkpoint remains provisional; full wing-pod-aircraft assembly is not complete.'])
(OUT/'result.json').write_text(json.dumps(report,indent=2))
print(json.dumps(dict(passed=True,parts=len(physical),skin_removed_mm3=removed,exports=exports)),flush=True)
