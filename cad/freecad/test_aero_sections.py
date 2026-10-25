"""Native geometry edits and placements must reach the aerodynamic section data."""
import hashlib,json,math
from pathlib import Path
import FreeCAD as App
import native,worker,aero_sections

worker.document('AeroSectionTest');doc=worker.active()
def ring(chord,y):
    return [[chord*(1+math.cos(i*math.pi/16))/2,y,.06*chord*math.sin(i*math.pi/16)] for i in range(32)]
rings=[ring(100,0),ring(80,100),ring(50,200)]
native.make_part(doc,dict(name='Wing',kind='spline_loft',params=dict(rings=rings,split_index=16,sampled_tolerance_mm=.5,ruled=False)))
before=worker.geometry_fingerprint([doc.Wing])
data=aero_sections.extract(doc,'Wing')
assert [round(s['chord_m'],8) for s in data['sections']]==[.1,.08,.05]
assert [round(s['xyz_le_m'][1],8) for s in data['sections']]==[0,.1,.2]
assert all(abs(s['twist_deg'])<1e-9 for s in data['sections'])
assert before==worker.geometry_fingerprint([doc.Wing])
worker.place('Wing',[5,0,2])
placed=aero_sections.extract(doc,'Wing')
assert abs(placed['sections'][0]['xyz_le_m'][0]-.005)<1e-9
assert abs(placed['sections'][0]['xyz_le_m'][2]-.002)<1e-9
assert placed['source_geometry_sha256']!=data['source_geometry_sha256']
worker.set_profile(doc.Wing.Sections[1].Name,ring(90,100))
edited=aero_sections.extract(doc,'Wing')
assert abs(edited['sections'][1]['chord_m']-.09)<1e-9
for args in [dict(samples_per_branch=True),dict(samples_per_branch=130),dict(symmetric=1),dict(object_name='Missing')]:
    request=dict(object_name='Wing');request.update(args)
    try:aero_sections.extract(doc,**request)
    except ValueError:pass
    else:raise AssertionError('invalid extraction accepted')
out=Path(__file__).resolve().parents[2]/'out/freecad-aero-sections';out.mkdir(exist_ok=True)
doc.saveAs(str(out/'fixture.FCStd'));App.closeDocument(doc.Name);worker._doc=None
worker.open_document(str(out/'fixture.FCStd'));doc=worker.active()
reopened=aero_sections.extract(doc,'Wing')
assert max(abs(a-b) for s,t in zip(edited['sections'],reopened['sections']) for a,b in zip(s['xyz_le_m'],t['xyz_le_m']))<1e-9
assert abs(reopened['sections'][1]['chord_m']-.09)<1e-9
(out/'result.json').write_text(json.dumps(dict(passed=True,units_verified=True,source_unchanged_by_extraction=True,
    rigid_placement_propagated=True,profile_edit_propagated=True,reopen_passed=True,invalid_inputs_rejected=True),indent=2))
App.closeDocument(doc.Name)
print('Native aerodynamic section units, placement, profile edits, rejection and reopen passed')
