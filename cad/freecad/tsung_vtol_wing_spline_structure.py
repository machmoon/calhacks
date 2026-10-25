"""Smooth S3024 internal structure study; inferred design, not author's internals.

Interpolates supplied S3024 coordinates with two native spline branches. This
changes the faceted reference between points; no exact polygon equality is claimed.
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
OUT=Path(os.environ.get('WRIGHT_CAD_OUT',ROOT/'out/tsung-vtol-wing-spline-structure')).resolve()
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
worker.document('TsungSmoothWingStructure');doc=worker.active()
layout.create(doc,'WingControls',{'HalfSpan':dict(value=600,min=600,max=610),
    'RibThickness':dict(value=2,min=1.5,max=3),
    'SparOuterRadius':dict(value=2,min=2,max=6),
    'SparInnerRadius':dict(value=1.5,min=1,max=5)})

SPLIT=min(range(len(foil)),key=lambda i:foil[i][0])
INSET_MM=1.2
inset_fit_evidence=[]

def ring(eta):
    chord=max(2,240*math.sqrt(max(0,1-eta*eta)))
    trailing=480-12*eta*eta
    return [[trailing-chord+x*chord,eta*600,20+z*chord] for x,z in foil]

def fitted_inset(eta):
    # This is an explicitly recorded construction snapshot. No live wall-inset
    # control is exposed: the editable native result uses these fitted sections.
    import bisect
    curves,placement,_,_=profiles._spline_data(ring(eta),SPLIT,native.vector,.2)
    source_wire=Part.Wire([c.toShape() for c in curves]);source_wire.Placement=placement
    offset=source_wire.makeOffset2D(-INSET_MM,join=0,fill=False)
    if offset.isNull() or not offset.isValid() or len(offset.Wires)!=1:
        raise ValueError('inset did not produce one valid wire')
    offset_wire=offset.Wires[0]
    if not offset_wire.isClosed() or Part.Face(offset_wire).Area>=Part.Face(source_wire).Area:
        raise ValueError('inset failed to remove section area')
    dense=offset_wire.discretize(Number=4001)
    if (dense[-1]-dense[0]).Length<1e-7:dense=dense[:-1]
    start=max(range(len(dense)),key=lambda i:dense[i].x)
    dense=dense[start:]+dense[:start]
    leading=min(range(len(dense)),key=lambda i:dense[i].x)
    if max(p.z for p in dense[:leading+1])<max(p.z for p in dense[leading:]):
        dense=dense[:1]+list(reversed(dense[1:]))
        leading=min(range(len(dense)),key=lambda i:dense[i].x)
    def sample(branch):
        distances=[0.0]
        for a,b in zip(branch,branch[1:]):distances.append(distances[-1]+(b-a).Length)
        points=[]
        for j in range(129):
            target=distances[-1]*(1-math.cos(math.pi*j/128))/2
            index=min(len(branch)-2,max(0,bisect.bisect_right(distances,target)-1))
            fraction=(target-distances[index])/(distances[index+1]-distances[index])
            points.append(branch[index]+(branch[index+1]-branch[index])*fraction)
        return points
    upper=sample(dense[:leading+1]);lower=sample(dense[leading:]+dense[:1])
    points=upper+lower[1:-1]
    coords=[[p.x,p.y,p.z] for p in points]
    fitted,frame,_,quality=profiles._spline_data(coords,128,native.vector,.15)
    fitted_wire=Part.Wire([c.toShape() for c in fitted]);fitted_wire.Placement=frame
    errors=[Part.Vertex(frame.multVec(p)).distToShape(offset_wire)[0]
            for c in fitted for p in c.discretize(Number=301)]
    errors.extend(Part.Vertex(p).distToShape(fitted_wire)[0] for p in dense[::10])
    maximum=max(errors)
    if maximum>.08:raise ValueError('sampled fitted inset deviation exceeds .08 mm')
    inset_fit_evidence.append(dict(eta=eta,native_offset_mm=INSET_MM,
        offset_edges=len(offset_wire.Edges),fitted_edges=2,source_points_per_branch=129,
        sampled_bidirectional_offset_fit_max_mm=maximum,certified_hausdorff_bound=False,
        method='Two curves interpolating cosine-spaced samples of a dense native offset wire; sampled distances in both directions to that wire; actual 3D minimum wall checked independently.',
        profile_quality=quality))
    return coords

def wing_loft(name,etas,inner=False):
    rings=[fitted_inset(e) for e in etas] if inner else [ring(e) for e in etas]
    native.make_part(doc,dict(name=name,kind='spline_loft',params=dict(rings=rings,
        split_index=128 if inner else SPLIT,sampled_tolerance_mm=.15 if inner else .2)))
    obj=doc.getObject(name)
    for section,e in zip(obj.Sections,etas):section.setExpression('Placement.Base.y',f'WingControls.HalfSpan * {e}')
    print('Built',name,'with',len(obj.Shape.Faces),'faces',flush=True)
    return obj

outer=wing_loft('RightOuterContour',outer_eta)
cavity=wing_loft('RightCavity',inner_eta,True)
skin=doc.addObject('Part::Cut','RightSkin');skin.Base=outer;skin.Tool=cavity
# A straight internal tube and ribs are explicit provisional engineering choices.
# Root end is at the cavity end cap; outer diameter bores follow the tube datum.
bore=doc.addObject('Part::Cylinder','SparBoreTool');bore.Radius=2;bore.Height=610
bore.Placement=App.Placement(App.Vector(400,0,24.5),App.Rotation(App.Vector(0,0,1),App.Vector(0,1,0)))
bore.setExpression('Height','WingControls.HalfSpan')
bore.setExpression('Radius','WingControls.SparOuterRadius')
native.make_part(doc,dict(name='RightSpar',kind='tube',params=dict(radius=2,bore_radius=1.5,height=497)))
spar=doc.RightSpar
spar.Placement=App.Placement(App.Vector(400,3,24.5),bore.Placement.Rotation)
spar.setExpression('Placement.Base.y','WingControls.HalfSpan * 0.005')
# make_part tube is a cut with native Base and Tool, not a custom Python proxy.
spar.Base.setExpression('Height','WingControls.HalfSpan * (5 / 6 - 0.005)')
spar.Base.setExpression('Radius','WingControls.SparOuterRadius')
spar.Tool.setExpression('Radius','WingControls.SparInnerRadius')
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
(OUT/'inset-fit.json').write_text(json.dumps(inset_fit_evidence,indent=2))
doc.saveAs(str(OUT/'unverified-build.FCStd'))
print('Smooth wing native features built; unverified native checkpoint saved',flush=True)

def measure(full=False):
    current=worker.active()
    print('Phase: cavity containment',flush=True)
    outside=current.RightCavity.Shape.cut(current.RightOuterContour.Shape).Volume
    assert outside<=1e-6
    print('Phase: true minimum wall distance',flush=True)
    gap=current.RightCavity.Shape.distToShape(current.RightOuterContour.Shape.Shells[0])[0]
    assert gap>=.8,(gap,'minimum wall failed')
    removed=current.RightOuterContour.Shape.Volume-current.RightSkin.Shape.Volume
    assert removed>1
    integral_residual=removed-current.RightCavity.Shape.Volume
    print('Phase: exact removed-material decomposition (integrals retained separately)',flush=True)
    removed_shape=current.RightOuterContour.Shape.cut(current.RightSkin.Shape)
    removed_extra=removed_shape.cut(current.RightCavity.Shape).Volume
    removed_missing=current.RightCavity.Shape.cut(removed_shape).Volume
    material_overlap=current.RightSkin.Shape.common(current.RightCavity.Shape).Volume
    assert removed_shape.isValid() and len(removed_shape.Solids)==1
    assert max(removed_extra,removed_missing,material_overlap)<=1e-6
    material_evidence=dict(method='Bidirectional boolean differences between actual removed material and cavity, plus skin/cavity disjointness',
        removed_extra_mm3=removed_extra,removed_missing_mm3=removed_missing,
        skin_cavity_overlap_mm3=material_overlap,volume_integral_residual_mm3=integral_residual,
        volume_integral_identity_passed=abs(integral_residual)<.01,mass_validated=False)
    print('Phase: spar containment',flush=True)
    spar_outside=current.RightSpar.Shape.cut(current.RightCavity.Shape).Volume
    assert spar_outside<=1e-6,('spar outside cavity',spar_outside)
    result=dict(minimum_wall_mm=gap,minimum_required_wall_mm=.8,cavity_outside_outer_mm3=outside,
        outer_volume_mm3=current.RightOuterContour.Shape.Volume,cavity_volume_mm3=current.RightCavity.Shape.Volume,
        skin_volume_mm3=current.RightSkin.Shape.Volume,spar_outside_cavity_mm3=spar_outside,
        material_decomposition=material_evidence)
    if full:
        # Boolean reconstruction checks both directions, not only matching bounds.
        print('Phase: exact outer boolean equivalence',flush=True)
        restored=current.RightSkin.Shape.fuse(current.RightCavity.Shape)
        extra=restored.cut(current.RightOuterContour.Shape).Volume
        missing=current.RightOuterContour.Shape.cut(restored).Volume
        assert max(extra,missing)<=1e-6
        result['outer_contour_equivalence']=dict(extra_mm3=extra,missing_mm3=missing,
            method='Skin union cavity equals this smooth outer solid in both boolean differences; not the faceted reference')
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
native.set_parameter(doc,'WingControls','SparOuterRadius',2.2)
assert abs(doc.SparBoreTool.Radius.Value-2.2)<1e-9
assert abs(doc.RightSpar.Base.Radius.Value-2.2)<1e-9
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
report=dict(scope='Smooth S3024 wing structural prototype; source-coordinate faithful, not exactly equal to faceted reference',
    exact_original_author_internals=False,manufacturing_complete=False,
    source_airfoil=str(SOURCE),source_airfoil_sha256=hashlib.sha256(SOURCE.read_bytes()).hexdigest(),
    source_reference_generator='cad/freecad/tsung_vtol_reference.py',
    exact_faceted_reference_equivalence=False,inset_section_fit=inset_fit_evidence,
    physical_face_counts={name:len(doc.getObject(name).Shape.Faces) for name in physical},
    outer_section_evidence=[json.loads(s.WrightSplineEvidence) for s in doc.RightOuterContour.Sections],
    outer_geometry=dict(stations_eta=outer_eta,chord='max(2,240*sqrt(1-eta^2))',trailing_x='480-12*eta^2',z_offset_mm=20,half_span_mm=600,
        note='Same normalized source coordinates and inferred stations; two-curve smoothing differs between supplied points from faceted reference'),
    engineering_assumptions=dict(cavity_stations_eta=inner_eta,nominal_section_inset_mm=INSET_MM,
        wall_type='Native section offsets fitted into two-curve snapshots, then lofted; actual 3D minimum measured, not assumed constant',
        inset_edit_policy='No live inset control; regenerate fitted profiles for a different inset. Span and spar radii are native linked controls.',
        closed_tip_span_mm=60,root_cap_span_mm=3,rib_stations_eta=rib_eta,rib_thickness_mm=2,
        spar=dict(axis='Y',x_mm=400,z_mm=24.5,root_y_mm=3,tip_y_mm=500,outer_diameter_mm=4,inner_diameter_mm=3),
        spar_bores='Exact geometric OD, no manufacturing clearance; not validated fit/tolerance'),
    physical_parts=physical,baseline=baseline,edited_610mm=edited,exports=exports,
    edit_checkpoint=edit_checkpoint,reopened_span_edit_passed=True,reopened_edited_checkpoint_passed=True,linked_spar_bore_radius_edit_passed=True,
    limitations=['Raw curved-surface volume integrals disagree; material decomposition is verified by set differences, not trusted as a mass estimate.',
        'The OD4/ID3 default spar remains structurally unsized; no material allowable, mass claim or flight validation.',
        'Spline exterior matches source points but differs between samples from faceted reference; approximation evidence is sampled, not a certified maximum.',
        'Section inset fitting is recorded construction data, not a live constant-wall feature.',
        'Original hidden internals are unknown; all cavity/rib/spar values are new engineering assumptions.',
        'Root/pod/boom joints, controls, ailerons, servo access, wiring, fasteners and fabrication segmentation absent.',
        'This isolated bilateral wing study does not resolve full-aircraft integration clashes.',
        'Zero-clearance spar bores demonstrate geometric seating, not assembly tolerance or manufacturing fit.'])
(OUT/'result.json').write_text(json.dumps(report,indent=2))
App.closeDocument(doc.Name);worker._doc=None
print('Smooth bilateral wing study passed: native skins/ribs/spars, smooth-outline reconstruction, measured minimum wall, edit/reopen.',flush=True)
