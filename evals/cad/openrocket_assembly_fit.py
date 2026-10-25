"""Screen every exported material solid in the pinned OpenRocket fixtures.

This exercises Wright's production interference checker. It does not certify
missing components, required connections, structural sizing or flight readiness.
"""
import hashlib
import json
from pathlib import Path
import FreeCAD as App
import Part
import worker

ROOT = Path(__file__).resolve().parents[2]
FIXTURES = ROOT/'out/freecad-openrocket-verified'
OUT = ROOT/'out/openrocket-assembly-fit'
OUT.mkdir(parents=True, exist_ok=True)
source_report_path = FIXTURES/'result.json'
source_report = json.loads(source_report_path.read_text())
report = dict(source_report=str(source_report_path),
    source_report_sha256=hashlib.sha256(source_report_path.read_bytes()).hexdigest(),
    checker='cad.freecad.worker.check (production all-solid-pair interference)',
    checker_sha256=hashlib.sha256((ROOT/'cad/freecad/worker.py').read_bytes()).hexdigest(),
    complete_rocket_design_verified=False, cases=[])

for index, case in enumerate(source_report['imports']):
    directory = FIXTURES/Path(case['source']['file']).stem
    source = directory/'partial.step'
    source_hash = hashlib.sha256(source.read_bytes()).hexdigest()
    shape = Part.read(str(source))
    solids = shape.Solids
    assert 1 <= len(solids) <= 128
    assert len(solids) == case['exchange']['physical_solids']
    worker.document('RocketFit'+str(index)); doc = worker.active()
    names = []
    for i, solid in enumerate(solids):
        assert solid.isValid()
        obj = doc.addObject('Part::Feature', 'MaterialSolid'+str(i))
        obj.Shape = solid
        names.append(obj.Name)
    doc.recompute()
    before = worker.geometry_fingerprint([doc.getObject(n) for n in names])
    print(case['source']['file'], len(solids), 'solids; checking every pair', flush=True)
    fit = worker.check(names, tolerance_mm=0.001, require_connected=False)
    assert len(fit['pairs']) == len(solids)*(len(solids)-1)//2
    assert before == worker.geometry_fingerprint([doc.getObject(n) for n in names])
    overlaps = [pair for pair in fit['pairs'] if pair['clash']]
    # A copied solid at the exact same placement must not receive a false clear.
    duplicate = doc.addObject('Part::Feature', 'DeliberateOverlap')
    duplicate.Shape = solids[0].copy(); doc.recompute()
    control = worker.check([names[0], duplicate.Name], tolerance_mm=0.001)
    assert not control['passed'] and control['pairs'][0]['clash']
    doc.removeObject(duplicate.Name); doc.recompute()
    assert before == worker.geometry_fingerprint([doc.getObject(n) for n in names])
    assert hashlib.sha256(source.read_bytes()).hexdigest() == source_hash
    missing = [dict(tag=c['tag'], name=c['name'], status=c['status'])
        for c in case['component_coverage'] if c['status'] != 'created']
    row = dict(source_design=case['source'], source_step=str(source),
        source_step_sha256=source_hash, source_unchanged=True,
        material_solids=len(solids), checked_pairs=len(fit['pairs']),
        overlap_pairs=overlaps, overlap_pair_count=len(overlaps),
        no_detected_material_interference=not overlaps,
        coincident_solid_negative_control_passed=True,
        omitted_components=missing, complete_source_coverage=not missing,
        required_connections='not specified or verified',
        solid_identity='STEP traversal indices; do not infer manufacturer or source component identity',
        scope='Current exported native solid geometry only. No exemptions for intentional overlaps; '
              'each detected overlap requires interpretation or correction. Omitted recovery/mass '
              'components are not modeled. Clearance, connectivity, structural and flight checks incomplete.',
        all_pairs=fit['pairs'])
    report['cases'].append(row)
    (OUT/'progress.json').write_text(json.dumps(report, indent=2))
    print('screened',len(fit['pairs']),'pairs;',len(overlaps),'overlaps;',len(missing),'omitted components',flush=True)
    App.closeDocument(doc.Name); worker._doc = None; worker._last_check = None

report['status'] = 'screening_complete'
(OUT/'result.json').write_text(json.dumps(report, indent=2))
print('Assembly screening complete; full rocket design is not verified', flush=True)
