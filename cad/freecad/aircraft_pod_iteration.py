"""Native pod interface/access study on the saved structural aircraft baseline.
The baseline pod is already hollow. This preserves that native cavity and adds
wing-envelope root seats, a mount seat and a removable underside access patch.
All dimensions remain provisional; zero-clearance seams are not fabrication fits.
"""
import json,hashlib,time,os
from pathlib import Path
import FreeCAD as App
import Part
import native,worker
ROOT=Path(__file__).resolve().parents[2]
OUT=Path(os.environ.get('WRIGHT_CAD_OUT',ROOT/'out/freecad-aircraft-pod')).resolve();OUT.mkdir(parents=True,exist_ok=True)
source=ROOT/'out/freecad-aircraft-structural/assembly.FCStd'
baseline=json.loads((ROOT/'out/freecad-aircraft-structural/result.json').read_text())
plan=json.loads((ROOT/'out/cad-seated-mounts/plan.json').read_text())
worker.restore(str(source));doc=worker.active();started=time.monotonic()
physical=baseline['fit']['objects'];contacts=[[p['a'],p['b']] for p in baseline['fit']['pairs'] if p['required_contact']]
original={n:doc.getObject(n).Shape.copy() for n in physical}
pod=doc.part_pod;outer=pod.Base;inner=pod.Tool
section_checks=[]
for label,loft in [('outer',outer),('inner',inner)]:
 for section in loft.Sections:
  face=Part.Face(section.Shape.Wires[0]);section_checks.append(dict(kind=label,name=section.Name,edges=len(section.Shape.Edges),valid=face.isValid(),area_mm2=face.Area,fully_constrained=section.FullyConstrained))
assert all(c['valid'] and c['fully_constrained'] for c in section_checks)
containment=inner.Shape.cut(outer.Shape).Volume
assert containment<1e-5
# Material thickness is measured by distance between the complete cavity and
# outer boundary surfaces, rather than assuming radial reductions are normal offsets.
wall_min=Part.makeCompound(inner.Shape.Faces).distToShape(Part.makeCompound(outer.Shape.Faces))[0]
assert wall_min>0
print('Validated constrained sections, cavity containment and minimum wall',wall_min,flush=True)
old_volume=pod.Shape.Volume
doc.removeObject('part_pod');doc.recompute()
def boolean(kind,name,base,tool):
 o=doc.addObject(kind,name);o.Base=base;o.Tool=tool;doc.recompute()
 assert not o.Shape.isNull() and o.Shape.isValid(),name
 return o
shell=boolean('Part::Cut','PodOriginalShell',outer,inner)
# Shared outer wing construction retains live section/thickness dependency.
# Cutting filled root envelopes creates seats through shell; the physical skins
# and ribs remain untouched. This is a mating aperture, not collision exemption.
right=boolean('Part::Cut','PodRightRootSeat',shell,doc.part_wing_R_Outer)
left=boolean('Part::Cut','PodBothRootSeats',right,doc.part_wing_L_Outer)
# Separate tool with expressions follows the existing mount's box dimensions.
# The physical mount is not selected as a boolean construction child.
seat=doc.addObject('Part::Box','PodMountSeatTool')
for prop in ['Length','Width','Height']:
 seat.setExpression(prop,'part_pusher_mount.'+prop)
for axis in ['x','y','z']:seat.setExpression('Placement.Base.'+axis,'part_pusher_mount.Placement.Base.'+axis)
doc.recompute()
seated=boolean('Part::Cut','PodSeatedShell',left,seat)
access=doc.addObject('Part::Box','PodAccessTool')
access.Length=100;access.Width=60;access.Height=30;access.Placement.Base=App.Vector(-90,-30,-120);doc.recompute()
hatch=boolean('Part::Common','part_pod_access_hatch',seated,access)
final=boolean('Part::Cut','part_pod',seated,access)
assert len(final.Shape.Solids)==1 and len(hatch.Shape.Solids)==1
physical.append(hatch.Name);contacts.append(['part_pod',hatch.Name])
unchanged=[]
for name,shape in original.items():
 if name=='part_pod':continue
 current=doc.getObject(name).Shape
 assert abs(current.Volume-shape.Volume)<1e-6 and current.cut(shape).Volume<1e-6 and shape.cut(current).Volume<1e-6
 unchanged.append(name)
print('Verified 59 unchanged physical parts',flush=True)
# Union of shell and hatch equals explicitly seated geometry, with no added material.
closed=final.Shape.fuse(hatch.Shape)
assert closed.cut(seated.Shape).Volume<1e-6 and seated.Shape.cut(closed).Volume<1e-6
assert closed.cut(original['part_pod']).Volume<1e-6
stages=dict(original_shell_mm3=old_volume,outer_envelope_mm3=outer.Shape.Volume,cavity_mm3=inner.Shape.Volume,right_root_removed_mm3=shell.Shape.Volume-right.Shape.Volume,left_root_removed_mm3=right.Shape.Volume-left.Shape.Volume,mount_seat_removed_mm3=left.Shape.Volume-seated.Shape.Volume,final_shell_mm3=final.Shape.Volume,hatch_mm3=hatch.Shape.Volume)
# Verify an access-size edit propagates through both pieces while preserving union.
volume_before=hatch.Shape.Volume;access.Length=105;doc.recompute()
assert hatch.Shape.Volume>volume_before
edited_closed=final.Shape.fuse(hatch.Shape)
assert edited_closed.cut(seated.Shape).Volume<1e-5 and seated.Shape.cut(edited_closed).Volume<1e-5
access.Length=100;doc.recompute()
print('Pod seats and access geometry valid; checking full aircraft',flush=True)
fit=worker.check(physical,contacts,require_connected=True)
exports=worker.save(str(OUT),physical)
# Actual geometry close-up, cut through y=0 to reveal cavity and root interfaces.
cut=Part.makeBox(600,250,240,App.Vector(-250,-250,-150))
faces=[];colors=[]
for name in ['part_pod','part_pod_access_hatch','part_pusher_mount','part_rib_R1','part_rib_L1','part_joiner_R','part_joiner_L']:
 shape=doc.getObject(name).Shape.cut(cut)
 if shape.isNull():continue
 vertices,triangles=shape.tessellate(.5)
 for tri in triangles:
  faces.append([[vertices[j].x,vertices[j].y,vertices[j].z] for j in tri]);colors.append('#d8a63c' if name=='part_pod_access_hatch' else '#4d9e91' if 'rib' in name else '#73889e' if name=='part_pod' else '#ac7959')
(OUT/'cutaway-mesh.json').write_text(json.dumps(dict(faces=faces,colors=colors)))
result=dict(scope='Actual aircraft hollow pod interface and underside access study; baseline was already hollow',source_sha256=hashlib.sha256(source.read_bytes()).hexdigest(),section_checks=section_checks,cavity_outside_outer_mm3=containment,minimum_original_wall_distance_mm=wall_min,volumes=stages,unchanged_physical_parts=unchanged,physical_parts=len(physical),fit=fit,exports=exports,access_edit_propagation_passed=True,provisional_inputs=dict(access_tool_xyz_mm=[100,60,30],access_origin_mm=[-90,-30,-120],interface_clearance_mm=0,hatch_seam_clearance_mm=0),missing=['Mount fastening and retention','Wing/root joint reinforcement and fastening','Hatch flange, retention, seal and manufacturing clearance','Remaining full aircraft clashes and disconnected contacts','Structural, thermal, material and manufacturing validation'],aircraft_complete=False,elapsed_s=time.monotonic()-started)
(OUT/'result.json').write_text(json.dumps(result,indent=2))
print(json.dumps(dict(parts=len(physical),clashes=fit['overlap_count'],unmet_contacts=fit['unmet_contact_count'],groups=len(fit['components']),volumes=stages,wall_min=wall_min)),flush=True)
App.closeDocument(doc.Name)
