"""Curved S3024 internal structure study; inferred design, not author's internals.

Retains the current reference outer loft exactly. The inset cavity is deliberately
variable-thickness; minimum separation is measured in 3D, not inferred from scale.
"""
import hashlib
import json
import math
import os
from pathlib import Path
import FreeCAD as App
import Part
import layout
import native
import profiles
import worker

ROOT=Path(__file__).resolve().parents[2]
OUT=Path(os.environ.get('WRIGHT_CAD_OUT',ROOT/'out/tsung-vtol-wing-structure')).resolve()
OUT.mkdir(parents=True,exist_ok=True)
SOURCE=ROOT/'cad/projects/tsung_vtol/references/s3024.dat'
raw=[]
for line in SOURCE.read_text().splitlines()[1:]:
    try:raw.append(tuple(map(float,line.split())))
    except ValueError:pass
foil=[((raw[0][0]+raw[-1][0])/2,0)]+raw[1:-1]
outer_eta=[0,.12,.25,.4,.55,.7,.82,.9,.95,.98,1]
inner_eta=[.005,.12,.25,.4,.55,.7,.82,.9]
rib_eta=[.1,.25,.4,.55,.7,.82]
worker.document('TsungCurvedWingStructure');doc=worker.active()
layout.create(doc,'WingControls',{'HalfSpan':dict(value=600,min=600,max=610),
    'RibThickness':dict(value=2,min=1.5,max=3)})

def ring(eta,inner=False):
    chord=max(2,240*math.sqrt(max(0,1-eta*eta)))
    trailing=480-12*eta*eta
    return [[trailing-chord+(.5+(x-.5)*.8 if inner else x)*chord,eta*600,
             20+(.02+(z-.02)*.55 if inner else z)*chord] for x,z in foil]

def wing_loft(name,etas,inner=False):
    obj=profiles.loft(doc,name,[ring(e,inner) for e in etas],native.vector)
    for s,e in zip(obj.Sections,etas):s.setExpression('Placement.Base.y',f'WingControls.HalfSpan * {e}')
    return obj

outer=wing_loft('RightOuterContour',outer_eta)
cavity=wing_loft('RightCavity',inner_eta,True)
skin=doc.addObject('Part::Cut','RightSkin');skin.Base=outer;skin.Tool=cavity
# A straight internal tube and ribs are explicit provisional engineering choices.
# Root end is at the cavity end cap; outer diameter bores follow the tube datum.
bore=doc.addObject('Part::Cylinder','SparBoreTool');bore.Radius=2;bore.Height=610
bore.Placement=App.Placement(App.Vector(400,0,24.5),App.Rotation(App.Vector(0,0,1),App.Vector(0,1,0)))
bore.setExpression('Height','WingControls.HalfSpan')
native.make_part(doc,dict(name='RightSpar',kind='tube',params=dict(radius=2,bore_radius=1.5,height=497)))
spar=doc.RightSpar
spar.Placement=App.Placement(App.Vector(400,3,24.5),bore.Placement.Rotation)
spar.setExpression('Placement.Base.y','WingControls.HalfSpan * 0.005')
# make_part tube is a cut with native Base and Tool, not a custom Python proxy.
spar.Base.setExpression('Height','WingControls.HalfSpan * (5 / 6 - 0.005)')
physical=['RightSkin','RightSpar'];contacts=[]
for i,e in enumerate(rib_eta):
    slab=doc.addObject('Part::Box',f'RightRibSlab{i}');slab.Length=300;slab.Height=100
    slab.Placement.Base=App.Vector(200,600*e-1,-20)
    slab.setExpression('Width','WingControls.RibThickness')
    slab.setExpression('Placement.Base.y',f'WingControls.HalfSpan * {e} - WingControls.RibThickness / 2')
    blank=doc.addObject('Part::Common',f'RightRibBlank{i}');blank.Base=cavity;blank.Tool=slab
    rib=doc.addObject('Part::Cut',f'RightRib{i}');rib.Base=blank;rib.Tool=bore
    physical.append(rib.Name);contacts.extend([[skin.Name,rib.Name],[spar.Name,rib.Name]])
doc.recompute()
for name in list(physical):
    mirror=doc.addObject('Part::Mirroring',name.replace('Right','Left'))
    mirror.Source=doc.getObject(name);mirror.Normal=App.Vector(0,1,0);mirror.Base=App.Vector(0,0,0)
    physical.append(mirror.Name)
contacts += [[a.replace('Right','Left'),b.replace('Right','Left')] for a,b in contacts]
for name in physical:
    obj=doc.getObject(name)
    obj.addProperty('App::PropertyString','DesignEvidence','Wright')
    obj.DesignEvidence='Provisional engineered internals within inferred reference exterior; not original author CAD'
doc.recompute();native.validate_document(doc)
doc.saveAs(str(OUT/'unverified-build.FCStd'))
print('Curved wing native features built; unverified native checkpoint saved',flush=True)

def measure(full=False):
    current=worker.active()
    print('Phase: cavity containment',flush=True)
    outside=current.RightCavity.Shape.cut(current.RightOuterContour.Shape).Volume
    assert outside<=1e-6
    print('Phase: true minimum wall distance',flush=True)
    gap=current.RightCavity.Shape.distToShape(current.RightOuterContour.Shape.Shells[0])[0]
    assert gap>=.8,(gap,'minimum wall failed')
    removed=current.RightOuterContour.Shape.Volume-current.RightSkin.Shape.Volume
    assert removed>1 and abs(removed-current.RightCavity.Shape.Volume)<.01
    print('Phase: spar containment',flush=True)
    spar_outside=current.RightSpar.Shape.cut(current.RightCavity.Shape).Volume
    assert spar_outside<=1e-6,('spar outside cavity',spar_outside)
    result=dict(minimum_wall_mm=gap,minimum_required_wall_mm=.8,cavity_outside_outer_mm3=outside,
        outer_volume_mm3=current.RightOuterContour.Shape.Volume,cavity_volume_mm3=current.RightCavity.Shape.Volume,
        skin_volume_mm3=current.RightSkin.Shape.Volume,spar_outside_cavity_mm3=spar_outside)
    if full:
        # Boolean reconstruction checks both directions, not only matching bounds.
        print('Phase: exact outer boolean equivalence',flush=True)
        restored=current.RightSkin.Shape.fuse(current.RightCavity.Shape)
        extra=restored.cut(current.RightOuterContour.Shape).Volume
        missing=current.RightOuterContour.Shape.cut(restored).Volume
        assert max(extra,missing)<=1e-6
        result['outer_contour_equivalence']=dict(extra_mm3=extra,missing_mm3=missing,
            method='Skin union its cavity equals original source outer solid in both boolean differences')
    print('Wall, cavity and spar containment checks passed',flush=True)
    print('Phase: all-pair fit/contact audit with tight conservative bounds',flush=True)
    fit=worker.check(physical,contacts)
    print('Fit audit complete',fit['overlap_count'],fit['unmet_contact_count'],flush=True)
    assert fit['passed'],(fit['overlap_count'],fit['unmet_contact_count'])
    result['fit']=fit
    return result

baseline=measure(True)
exports=worker.save(str(OUT),physical)
print('Default native, AP242 and GLB saved; reopening and editing span',flush=True)
saved=Path(exports['native']);source_hash=hashlib.sha256(saved.read_bytes()).hexdigest()
App.closeDocument(doc.Name);worker._doc=None
worker.restore(str(saved));doc=worker.active()
native.set_parameter(doc,'WingControls','HalfSpan',610)
assert abs(doc.RightSkin.Shape.BoundBox.YMax-610)<1e-6
assert abs(doc.LeftSkin.Shape.BoundBox.YMin+610)<1e-6
assert abs(native.shape_bounds(doc.RightRib0.Shape).YLength-2)<1e-6
edited=measure(False)
edit_checkpoint=worker.checkpoint(str(OUT/'edited-610mm'))
App.closeDocument(doc.Name);worker._doc=None
worker.restore(edit_checkpoint['checkpoint']);doc=worker.active()
assert abs(doc.RightSkin.Shape.BoundBox.YMax-610)<1e-6
assert abs(doc.LeftSkin.Shape.BoundBox.YMin+610)<1e-6
assert hashlib.sha256(saved.read_bytes()).hexdigest()==source_hash
report=dict(scope='Curved S3024 wing structural prototype inside current inferred reference outline',
    exact_original_author_internals=False,manufacturing_complete=False,
    source_airfoil=str(SOURCE),source_airfoil_sha256=hashlib.sha256(SOURCE.read_bytes()).hexdigest(),
    source_reference_generator='cad/freecad/tsung_vtol_reference.py',
    outer_geometry=dict(stations_eta=outer_eta,chord='max(2,240*sqrt(1-eta^2))',trailing_x='480-12*eta^2',z_offset_mm=20,half_span_mm=600,
        note='Same inferred section construction as current reference; publication identifies airfoil/span, not these stations'),
    engineering_assumptions=dict(cavity_stations_eta=inner_eta,chord_fraction_scale=.8,thickness_fraction_scale=.55,
        contraction_center_normalized_xz=[.5,.02],wall_type='Variable thickness, deliberately conservative; not constant-wall offset',
        closed_tip_span_mm=60,root_cap_span_mm=3,rib_stations_eta=rib_eta,rib_thickness_mm=2,
        spar=dict(axis='Y',x_mm=400,z_mm=24.5,root_y_mm=3,tip_y_mm=500,outer_diameter_mm=4,inner_diameter_mm=3),
        spar_bores='Exact geometric OD, no manufacturing clearance; not validated fit/tolerance'),
    implementation_restart_reason='Old process stopped after verified trimmed-shape bounding-box implementation correction and correction of its invalid raw-bbox rib-thickness assertion; not a time-only retry',
    rejected_experiment=dict(method='Native Part::Offset2D sections with -2 mm inset, followed by one ruled loft',
        result='NULL cavity shape after recompute; offset edge counts changed from 61 to 47 toward tip; not accepted as a skin'),
    physical_parts=physical,baseline=baseline,edited_610mm=edited,exports=exports,
    edit_checkpoint=edit_checkpoint,reopened_span_edit_passed=True,reopened_edited_checkpoint_passed=True,
    limitations=['Variable skin is heavy/conservative; no material or mass claim, structural sizing or flight validation.',
        'Original hidden internals are unknown; all cavity/rib/spar values are new engineering assumptions.',
        'Root/pod/boom joints, controls, ailerons, servo access, wiring, fasteners and fabrication segmentation absent.',
        'This isolated bilateral wing study does not resolve full-aircraft integration clashes.',
        'Zero-clearance spar bores demonstrate geometric seating, not assembly tolerance or manufacturing fit.'])
(OUT/'result.json').write_text(json.dumps(report,indent=2))
App.closeDocument(doc.Name);worker._doc=None
print('Curved bilateral wing study passed: native skins/ribs/spars, exact outline, measured minimum wall, edit/reopen.',flush=True)
