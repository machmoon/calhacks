"""Independent, partial reference-fidelity checks on native exported geometry.

Run with cad/freecad/run.sh. Passing these gates is NOT exact reconstruction,
internal engineering validation, or evidence of flight performance.
"""
import hashlib
import json
import os
from pathlib import Path
import FreeCAD as App
import Part

ROOT = Path(__file__).resolve().parents[2]
SOURCE = Path(os.environ.get('WRIGHT_VTOL_NATIVE', ROOT / 'out/tsung-vtol-reference/assembly.FCStd'))
OUT = ROOT / 'out/tsung-vtol-reference-eval'
NAMES = ['WingRight', 'WingLeft', 'BoomRight', 'BoomLeft', 'FinRight',
         'FinLeft', 'TopStabilizer', 'Pod', 'LiftMotorFR', 'LiftMotorFL',
         'LiftMotorRR', 'LiftMotorRL', 'PusherMotor']
FOIL = ROOT / 'cad/projects/tsung_vtol/references/s3024.dat'
assert hashlib.sha256(FOIL.read_bytes()).hexdigest() == '1091d856c28a23b447a9c4622693e19086ca279fe4086d57a0aa6f9afeded61b'
COORDS = [tuple(map(float, line.split())) for line in FOIL.read_text().splitlines()[1:] if line.strip()]

def slab(shape, axis, coordinate, thickness=0.02):
    origin = [-2000., -2000., -2000.]
    size = [4000., 4000., 4000.]
    origin[axis] = coordinate - thickness / 2
    size[axis] = thickness
    cut = shape.common(Part.makeBox(*size, App.Vector(*origin)))
    if cut.isNull() or not cut.Solids:
        raise ValueError(f'No material at axis {axis}, station {coordinate}')
    # OCC's generic BoundBox can include an entire underlying loft surface
    # after trimming. These reconstruction sections are polygonal ruled
    # faces; their actual trim vertices give the section extents instead.
    box = App.BoundBox()
    for vertex in cut.Vertexes:
        box.add(vertex.Point)
    return box

def assess(shapes):
    rows = []
    def check(name, passed, observed, criterion, evidence='published'):
        rows.append(dict(name=name, passed=bool(passed), observed=observed,
                         criterion=criterion, evidence=evidence))
    missing = [n for n in NAMES if n not in shapes]
    check('reference_components_present', not missing, missing, 'All named reference features represented', 'visual topology')
    if missing:
        return rows
    for n in NAMES:
        s = shapes[n]
        check('valid_' + n, s.isValid() and len(s.Solids) > 0, len(s.Solids), 'Valid solid geometry', 'kernel')
    bb = Part.makeCompound([shapes[n] for n in NAMES]).BoundBox
    check('published_span', abs(bb.YLength - 1200) <= 1,
          bb.YLength, '1200 mm +/-1 mm numerical reconstruction tolerance')
    check('published_length', abs(bb.XLength - 770) <= 1,
          bb.XLength, '770 mm +/-1 mm numerical reconstruction tolerance; propeller sweep convention unresolved')
    for side, sign in [('Right', 1), ('Left', -1)]:
        wing = shapes['Wing' + side]
        stations = [slab(wing, 1, sign * y).XLength for y in (60, 300, 540)]
        check('wing_tapers_' + side, stations[0] > stations[1] > stations[2], stations,
              'Chord decreases from 10% to 50% to 90% semispan; does not prove exact near-elliptical contour', 'visual topology')
        # Compare actual section edges, not a claimed airfoil-name property.
        # Root incidence/twist of the real aircraft remain unknown; this gate
        # verifies this reconstruction's untwisted S3024 section only.
        y = sign * .5
        section = wing.section(Part.makeBox(4000, .02, 4000, App.Vector(-2000, y - .01, -2000)))
        if section.isNull() or not section.Vertexes:
            check('s3024_root_' + side, False, None, 'Recoverable root section edges', 'source-coordinate geometry')
        else:
            sb = section.BoundBox
            trailing = max(section.Vertexes, key=lambda v: v.Point.x).Point
            xmin = min(p[0] for p in COORDS); xmax = max(p[0] for p in COORDS)
            chord = sb.XLength / (xmax - xmin)
            errors = [Part.Vertex(App.Vector(sb.XMin + (x - xmin) * chord, y, trailing.z + z * chord)).distToShape(section)[0] / chord for x, z in COORDS]
            check('s3024_root_' + side, max(errors) < .002, max(errors),
                  'UIUC source points within0.2% chord of native root section; source hash pinned', 'source-coordinate geometry')
        fin = shapes['Fin' + side]
        b = fin.BoundBox
        low = slab(fin, 2, b.ZMin + b.ZLength * .1)
        high = slab(fin, 2, b.ZMin + b.ZLength * .9)
        centers = [(low.YMin + low.YMax) / 2, (high.YMin + high.YMax) / 2]
        check('inward_cant_' + side, abs(centers[1]) < abs(centers[0]) - 1,
              centers, 'Fin upper section is closer to centerline than lower section', 'visual topology')
        top = shapes['TopStabilizer'].BoundBox
        check('raised_stabilizer_' + side,
              top.Center.z > b.ZMin + .75 * b.ZLength,
              dict(stabilizer_z=top.Center.z, fin_z=[b.ZMin, b.ZMax]),
              'Horizontal stabilizer lies at fin tops, not boom bases', 'visual topology')
        distance = fin.distToShape(shapes['TopStabilizer'])[0]
        check('tail_meets_' + side, distance < .05, distance,
              'Nominal fin/stabilizer geometric contact within0.05 mm; no fastening or load claim', 'geometric assembly')
    return rows

before = hashlib.sha256(SOURCE.read_bytes()).hexdigest()
doc = App.openDocument(str(SOURCE))
shapes = {n: doc.getObject(n).Shape.copy() for n in NAMES if doc.getObject(n) is not None}
checks = assess(shapes)
# Inject material changes only in memory: these must fail the relevant gates.
controls = []
if all(n in shapes for n in NAMES):
    stretched = {n: s.copy() for n, s in shapes.items()}
    matrix = App.Matrix(); matrix.A11 = 1.2
    stretched = {n: s.transformGeometry(matrix) for n, s in stretched.items()}
    result = assess(stretched)
    controls.append(dict(fault='20% longitudinal stretch', detected=any(r['name'] == 'published_length' and not r['passed'] for r in result)))
    lowered = {n: s.copy() for n, s in shapes.items()}
    lowered['TopStabilizer'].translate(App.Vector(0, 0, -200))
    result = assess(lowered)
    controls.append(dict(fault='stabilizer moved below fin tops', detected=all(not r['passed'] for r in result if r['name'].startswith('raised_stabilizer_'))))
App.closeDocument(doc.Name)
assert before == hashlib.sha256(SOURCE.read_bytes()).hexdigest()
OUT.mkdir(parents=True, exist_ok=True)
report = dict(source=str(SOURCE), source_sha256=before, checks=checks,
              partial_reference_gates_passed=all(r['passed'] for r in checks), negative_controls=controls,
              source_preserved=True, exact_reconstruction_proven=False,
              unverified=['Exact surface contours and hidden dimensions', 'Actual component details and placements',
                          'Fasteners, tolerances, wiring and service access', 'Mass, structural and flight performance'])
(OUT / 'result.json').write_text(json.dumps(report, indent=2))
print(json.dumps(dict(passed=report['partial_reference_gates_passed'], failed=[r['name'] for r in checks if not r['passed']], negative_controls=controls)))
assert all(r['detected'] for r in controls), 'Fidelity evaluator missed an injected fault'
