"""Preliminary Pi 5 mounting fixture from the sourced hole pattern.

A fit-test fixture, not a complete balloon payload or flight-rated mounting tray.
Thickness, clearance holes and standoffs are explicitly provisional design inputs.
"""
import json
import math
import os
from pathlib import Path
import sys
sys.path.insert(0,str(Path(__file__).resolve().parent))
import FreeCAD as App
import Part
import Mesh
from native import make_part, describe
from exchange import export_step, export_glb

root=Path(__file__).resolve().parents[2]
reference=json.loads((root/'cad/projects/agents_in_space/references/pi5-dimensions.json').read_text())
out=Path(os.environ['WRIGHT_CAD_OUT']).resolve()
out.mkdir(parents=True,exist_ok=True)
doc=App.newDocument('PalloonPiMountFixture')
doc.UndoMode=1
holes=[[x+8,y+8] for x,y in reference['mounting_holes_xy']]
make_part(doc,dict(name='Tray',kind='mount',params=dict(x=101,y=72,z=2.4,bores=[[[x,y,-1],[x,y,3.4],1.6] for x,y in holes])))
selected=[doc.Tray]
for i,(x,y) in enumerate(holes):
    n=f'Standoff{i+1}'
    make_part(doc,dict(name=n,kind='tube',params=dict(radius=3,bore_radius=1.6,height=6)))
    obj=doc.getObject(n)
    obj.Placement.Base=App.Vector(x,y,2.4)
    selected.append(obj)
doc.recompute()
checks=[]
for i,a in enumerate(selected):
    for b in selected[i+1:]:
        overlap=a.Shape.common(b.Shape).Volume
        gap=a.Shape.distToShape(b.Shape)[0]
        checks.append(dict(a=a.Name,b=b.Name,overlap_mm3=overlap,distance_mm=gap,required_contact=a==doc.Tray,passed=overlap<=1e-6 and (a!=doc.Tray or gap<=1e-6)))
assert all(c['passed'] for c in checks)
expected=101*72*2.4-4*math.pi*1.6**2*2.4+4*math.pi*(3**2-1.6**2)*6
assert abs(sum(o.Shape.Volume for o in selected)-expected)<1e-5
for obj in selected:
    describe(obj)
    Mesh.export([obj],str(out/(obj.Name+'.stl')))
doc.saveAs(str(out/'pi_mount_fixture.FCStd'))
export_step(selected,out/'pi_mount_fixture.step')
export_glb(selected,out/'pi_mount_fixture.glb')
reimport=Part.read(str(out/'pi_mount_fixture.step'))
assert reimport.isValid() and len(reimport.Solids)==5
assert abs(reimport.Volume-expected)<1e-5
report=dict(project='Palloon Systems',scope='Preliminary Pi 5 mounting-pattern fixture; no board/component or full payload fit claim',geometry_passed=True,payload_complete=False,source=reference,provisional_design_inputs=dict(tray_xyz_mm=[101,72,2.4],board_xy_offset_mm=[8,8],clearance_hole_diameter_mm=3.2,standoff_od_mm=6,standoff_height_mm=6),physical_parts=[describe(o) for o in selected],pair_checks=checks,step_roundtrip_solids=5,volume_mm3=reimport.Volume,missing=['Fasteners and print tolerances','PCB underside component clearance','Cooler and plugged cable envelopes','Battery, radio, suspension and enclosure','Flight loads and thermal validation'])
(out/'result.json').write_text(json.dumps(report,indent=2))
App.closeDocument(doc.Name)
print('Pi fixture: 5 solids, 10 pair checks, native tree and STEP/STL saved; payload incomplete')
