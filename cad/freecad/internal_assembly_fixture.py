"""Editable two-deck instrument enclosure geometry benchmark, all dimensions provisional.
Run: cad/freecad/run.sh cad/freecad/internal_assembly_fixture.py
No electronics fidelity, thread engagement, structural or manufacturing claim.
"""
import json, os, sys
from pathlib import Path
import FreeCAD as App
import Part
import native, worker, layout

ROOT=Path(__file__).resolve().parents[2]
OUT=Path(os.environ.get('WRIGHT_CAD_OUT',ROOT/'out/freecad-internal-assembly')).resolve()
OUT.mkdir(parents=True,exist_ok=True)
worker.document('InternalInstrumentAssembly'); doc=worker.active()
layout.create(doc,'Dimensions',{'LowerClearance':{'value':12,'min':4,'max':20},'DeckSpacing':{'value':18,'min':12,'max':24}})
p=doc.Dimensions
physical=[];contacts=[]
def make(name,kind,params,xyz=(0,0,0),zexpr=None):
    native.make_part(doc,dict(name=name,kind=kind,params=params))
    o=doc.getObject(name);o.Placement.Base=App.Vector(*xyz)
    if zexpr:o.setExpression('Placement.Base.z',zexpr)
    physical.append(name);return o

def box(name,size,xyz=(0,0,0),zexpr=None):return make(name,'box',dict(zip(['x','y','z'],size)),xyz,zexpr)
def mount(name,size,holes,xyz=(0,0,0),zexpr=None):
    return make(name,'mount',dict(zip(['x','y','z'],size),bores=[[[x,y,-1],[x,y,size[2]+1],1.6] for x,y in holes]),xyz,zexpr)
pattern=[(8,8),(112,8),(8,62),(112,62)]
world=[(x+20,y+20) for x,y in pattern]
mount('Base',(160,110,3),world)
for name,size,pos in [('LeftWall',(3,110,57),(0,0,3)),('RightWall',(3,110,57),(157,0,3)),('BackWall',(154,3,57),(3,107,3)),('FrontWall',(154,3,57),(3,0,3))]:
    box(name,size,pos);contacts.extend([[name,'Base'],[name,'Lid']])
box('Lid',(160,110,3),(0,0,60))
low='3 mm + Dimensions.LowerClearance'
upper=low+' + 2 mm + Dimensions.DeckSpacing'
mount('LowerDeck',(120,70,2),pattern,(20,20,15),low)
mount('UpperDeck',(120,70,2),pattern,(20,20,35),upper)
for i,(x,y) in enumerate(world):
    a=f'LowerSpacer{i}';b=f'UpperSpacer{i}'
    make(a,'tube',dict(radius=3,bore_radius=1.6,height=12),(x,y,3))
    doc.getObject(a+'_Outer').setExpression('Height','Dimensions.LowerClearance')
    make(b,'tube',dict(radius=3,bore_radius=1.6,height=18),(x,y,17),low+' + 2 mm')
    doc.getObject(b+'_Outer').setExpression('Height','Dimensions.DeckSpacing')
    # Simplified clearance bolt: linked native shaft/head union; no threads implied.
    shaft=doc.addObject('Part::Cylinder',f'Bolt{i}Shaft');shaft.Radius=1.4
    shaft.Placement.Base=App.Vector(x,y,0);shaft.setExpression('Height',upper+' + 2 mm')
    head=doc.addObject('Part::Cylinder',f'Bolt{i}Head');head.Radius=2.8;head.Height=3
    head.Placement.Base=App.Vector(x,y,37);head.setExpression('Placement.Base.z',upper+' + 2 mm')
    bolt=doc.addObject('Part::Fuse',f'Bolt{i}');bolt.Base=shaft;bolt.Tool=head;physical.append(bolt.Name)
    contacts.extend([['Base',a],[a,'LowerDeck'],['LowerDeck',b],[b,'UpperDeck'],['UpperDeck',bolt.Name]])
for deck,z in [('LowerDeck',low),('UpperDeck',upper)]:
    for i,(x,y,sx,sy,h) in enumerate([(40,40,25,25,8),(80,40,40,20,6),(45,72,25,10,5),(90,72,20,10,7)]):
        name=deck+f'Component{i}';box(name,(sx,sy,h),(x,y,0),z+' + 2 mm');contacts.append([name,deck])
doc.recompute()
for n in physical:assert native.describe(doc.getObject(n))['solids']==1
# Native expression propagation: a real input edit moves every upper dependency.
before={n:doc.getObject(n).Shape.BoundBox.ZMin for n in ['LowerDeck','UpperDeck','UpperSpacer0','UpperDeckComponent0']}
p.LowerClearance=14;doc.recompute()
propagation={n:doc.getObject(n).Shape.BoundBox.ZMin-before[n] for n in before}
assert all(abs(delta-2)<1e-7 for delta in propagation.values())
assert abs(doc.LowerSpacer0_Outer.Height.Value-14)<1e-7
p.LowerClearance=12;doc.recompute()
# Deliberate collision and separation controls ensure a passing fixture isn't vacuous.
doc.UpperDeckComponent0.setExpression('Placement.Base.z',None)
doc.UpperDeckComponent0.Placement.Base.z=36;doc.recompute()
negative_overlap=doc.UpperDeckComponent0.Shape.common(doc.UpperDeck.Shape).Volume
assert negative_overlap>1
negative_clash=worker.check(['UpperDeck','UpperDeckComponent0'])
assert not negative_clash['passed'] and negative_clash['overlap_count']==1
doc.UpperDeckComponent0.Placement.Base.z=40;doc.recompute()
negative_separation=worker.check(['UpperDeck','UpperDeckComponent0'],[['UpperDeck','UpperDeckComponent0']],require_connected=True)
assert not negative_separation['passed'] and negative_separation['unmet_contact_count']==1
# Return to original dependency before the full fit check and exports.
doc.UpperDeckComponent0.setExpression('Placement.Base.z',upper+' + 2 mm');doc.recompute()
clearance_pairs=[('UpperDeckComponent0','Lid',15),('LowerDeckComponent0','UpperDeck',10),('LowerDeck','LeftWall',17),('Bolt0','LowerSpacer0',0.2)]
clearances=[]
for a,b,expected in clearance_pairs:
    measured=doc.getObject(a).Shape.distToShape(doc.getObject(b).Shape)[0]
    assert abs(measured-expected)<1e-6,(a,b,measured,expected)
    clearances.append(dict(a=a,b=b,expected_mm=expected,measured_mm=measured))
fit=worker.check(physical,contacts,tolerance_mm=0.01,require_connected=True)
assert fit['passed'],fit
exports=worker.save(str(OUT),physical)
# Cutaway is derived from actual physical B-reps; front half of housing removed only.
cut=Part.makeBox(180,60,80,App.Vector(-10,-5,-5))
triangles=[];colors=[]
for n in physical:
    shape=doc.getObject(n).Shape
    if n in ['FrontWall','Lid','LeftWall','RightWall']:shape=shape.cut(cut)
    if shape.isNull():continue
    vertices,faces=shape.tessellate(.3)
    color='#708294' if n in ['Base','Lid','FrontWall','BackWall','LeftWall','RightWall'] else '#339889' if 'Deck' in n and 'Component' not in n else '#d69c42' if 'Spacer' in n or 'Bolt' in n else '#434c63'
    for face in faces:triangles.append([[vertices[j].x,vertices[j].y,vertices[j].z] for j in face]);colors.append(color)
(OUT/'cutaway-mesh.json').write_text(json.dumps(dict(faces=triangles,colors=colors)))
report=dict(scope='Provisional two-deck instrument enclosure; solid fit and dependency benchmark only',physical_parts=len(physical),native_objects=len(doc.Objects),fit=fit,clearances=clearances,dependency_edit_delta_mm=propagation,negative_control_overlap_mm3=negative_overlap,negative_clash_check=negative_clash,negative_separation_check=negative_separation,exports=exports,missing=['Threads and lower bolt retention','Lid fastening','Real board/component/cable geometry','Tolerance, thermal and load validation'],complete_product=False)
(OUT/'result.json').write_text(json.dumps(report,indent=2))
App.closeDocument(doc.Name)
# Saved-document reload is independent of the in-memory parameter edit.
doc=App.openDocument(str(OUT/'assembly.FCStd'));doc.Dimensions.LowerClearance=13;doc.recompute()
assert abs(doc.UpperDeck.Shape.BoundBox.ZMin-36)<1e-7
report['saved_reopen_expression_edit_passed']=True
(OUT/'result.json').write_text(json.dumps(report,indent=2))
App.closeDocument(doc.Name)
print(json.dumps(dict(parts=report['physical_parts'],pairs=fit['pairs_total'],passed=fit['passed'],clearances=clearances)))
