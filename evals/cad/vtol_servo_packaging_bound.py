"""Necessary vertical-space test, not sufficient component-fit verification.

Uses the current cavity generator's declared section transformation. A bounding
height that is too small proves rejection; enough height never proves full fit.
"""
import hashlib,json,math
from pathlib import Path
ROOT=Path(__file__).resolve().parents[2]
generator=ROOT/'cad/freecad/tsung_vtol_wing_structure.py'
text=generator.read_text()
assert '(.02+(z-.02)*.55 if inner else z)*chord' in text, 'Cavity strategy changed; update this bound from new evidence'
reference=ROOT/'cad/projects/tsung_vtol/references'
servo=json.loads((reference/'mg90s-dimensions.json').read_text())
foil=[tuple(map(float,l.split())) for l in (reference/'s3024.dat').read_text().splitlines()[1:] if l.strip()]
foil=[((foil[0][0]+foil[-1][0])/2,0)]+foil[1:-1]
z_range=max(z for x,z in foil)-min(z for x,z in foil)
# Vertical thickness at the same chord station is a stronger bound than the
# global z-range: the upper maximum and lower minimum occur at different x.
thickness=[]
for x in sorted({x for x,z in foil}):
    crossings=[]
    for (a,z),(b,w) in zip(foil,foil[1:]+foil[:1]):
        if min(a,b)-1e-12<=x<=max(a,b)+1e-12:
            if abs(a-b)<1e-12:crossings.extend([z,w])
            else:crossings.append(z+(x-a)/(b-a)*(w-z))
    if len(crossings)>1:thickness.append((max(crossings)-min(crossings),x))
maximum_thickness,thickness_x=max(thickness)
smallest_dimension=min(servo['provisional_conservative_static_envelope_LWH_mm'])
per_side_clearance=.5
required=smallest_dimension+2*per_side_clearance
stations=[]
for eta in [.005,.12,.25,.4,.55,.7,.82,.9]:
    maximum_height=240*math.sqrt(1-eta*eta)*.55*maximum_thickness
    stations.append(dict(span_mm=600*eta,maximum_section_height_mm=maximum_height,
        body_height_fits_necessary_bound=maximum_height>=smallest_dimension,
        body_plus_clearance_fits_necessary_bound=maximum_height>=required))
# Corresponding vertices in this ruled loft share an affine foil transform.
# Between stations, chord is linearly interpolated, so maximum vertical
# thickness cannot exceed the largest endpoint's scaled foil thickness.
upper=max(x['maximum_section_height_mm'] for x in stations)
out=ROOT/'out/tsung-vtol-servo-packaging';out.mkdir(parents=True,exist_ok=True)
report=dict(status='Current conservative cavity fails desired vertical servo clearance throughout',
    source_generator_sha256=hashlib.sha256(generator.read_bytes()).hexdigest(),
    servo_reference=str(reference/'mg90s-dimensions.json'),
    smallest_servo_box_axis_mm=smallest_dimension,
    engineering_clearance_each_side_mm=per_side_clearance,required_vertical_space_mm=required,
    cavity_maximum_vertical_thickness_upper_bound_mm=upper,stations=stations,
    looser_global_z_extent_upper_bound_mm=240*math.sqrt(1-.005**2)*.55*z_range,
    source_polygon_maximum_thickness_fraction=maximum_thickness,maximum_thickness_chord_station=thickness_x,
    all_positions_fail_desired_clearance=upper<required,
    rejected_station_span_mm=[s['span_mm'] for s in stations if not s['body_plus_clearance_fits_necessary_bound']],
    not_ruled_out_station_span_mm=[s['span_mm'] for s in stations if s['body_plus_clearance_fits_necessary_bound']],
    scope='Axis-aligned packaging orientations only; an outer dimensional bounding box is conservative and mounting/horn/cable volumes are absent',
    limitations=['Not an exact B-rep servo fit test','Diagonal or externally protruding installations not evaluated',
        'Passing this necessary bound would not prove a rectangular body fits the curved cavity',
        '0.5mm per-side allowance is a provisional engineering choice, not a manufacturer requirement'])
(out/'result.json').write_text(json.dumps(report,indent=2))
print(json.dumps(dict(required_mm=required,cavity_upper_bound_mm=upper,rejected_stations_mm=report['rejected_station_span_mm'],actual_fit_proven=False)))
