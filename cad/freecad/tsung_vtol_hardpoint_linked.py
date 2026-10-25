"""Bind mating radii to shared controls in a separate hardpoint revision."""
import hashlib,json
from pathlib import Path
import FreeCAD as App
import native,worker,layout
ROOT=Path(__file__).resolve().parents[2]
SOURCE=ROOT/'out/tsung-vtol-wing-hardpoint/assembly.FCStd'
OUT=ROOT/'out/tsung-vtol-wing-hardpoint-linked';OUT.mkdir(parents=True,exist_ok=True)
original=json.loads((SOURCE.parent/'result.json').read_text())
source_hash=hashlib.sha256(SOURCE.read_bytes()).hexdigest()
worker.open_document(str(SOURCE));doc=worker.active()
physical=original['physical']
contacts=[[p['a'],p['b']] for p in original['fit']['pairs'] if p['required_contact']]
layout.create(doc,'MatingRadii',{'SparRadius':dict(value=2,min=1.6,max=2.8),
    'BoomRadius':dict(value=5,min=4.1,max=5.8)})
bindings={
    'RightSpar_Outer':'MatingRadii.SparRadius',
    'SparBoreTool':'MatingRadii.SparRadius',
    'HardpointSparBore':'MatingRadii.SparRadius',
    'SparCollarBlank':'MatingRadii.SparRadius + 2.5 mm',
    'AttachmentBoom_Outer':'MatingRadii.BoomRadius',
    'BoomBore':'MatingRadii.BoomRadius',
    'BoomCollar':'MatingRadii.BoomRadius + 4 mm',
}
# Apply one atomic expression update, rather than recomputing each dependent
# curved-skin Boolean seven times. Values are unchanged in the nominal revision.
doc.openTransaction('Link mating radii')
try:
    for name,expression in bindings.items():doc.getObject(name).setExpression('Radius',expression)
    doc.recompute();native.validate_document(doc);doc.commitTransaction()
except Exception:
    doc.abortTransaction();doc.recompute();raise
print('Mating-radius dependencies linked; checking nominal assembly',flush=True)
baseline=worker.check(physical,contacts,tolerance_mm=.001,require_connected=True)
assert baseline['passed']
exports=worker.save(str(OUT),physical)
saved_hash=hashlib.sha256(Path(exports['native']).read_bytes()).hexdigest()
App.closeDocument(doc.Name);worker._doc=None;worker.restore(exports['native']);doc=worker.active()
print('Saved native reopened; probing source spar-radius edit',flush=True)
native.set_parameter(doc,'MatingRadii','SparRadius',2.1)
for name in ['RightSpar_Outer','SparBoreTool','HardpointSparBore']:
    assert abs(doc.getObject(name).Radius.Value-2.1)<1e-9
assert abs(doc.SparCollarBlank.Radius.Value-4.6)<1e-9
spar_fit=worker.check(physical,contacts,tolerance_mm=.001,require_connected=True)
assert spar_fit['passed']
print('Spar radius follows source; probing boom-radius edit',flush=True)
native.set_parameter(doc,'MatingRadii','BoomRadius',5.1)
assert abs(doc.AttachmentBoom_Outer.Radius.Value-5.1)<1e-9
assert abs(doc.BoomBore.Radius.Value-5.1)<1e-9
assert abs(doc.BoomCollar.Radius.Value-9.1)<1e-9
boom_fit=worker.check(physical,contacts,tolerance_mm=.001,require_connected=True)
assert boom_fit['passed']
# Check the modified skin passage against the actual new hardpoint as well.
upper=doc.HPNutSeat1.Shape;skin=doc.RightSkinWithHardpointPassage.Shape
assert skin.common(upper).Volume<1e-6 and skin.distToShape(upper)[0]<1e-6
assert hashlib.sha256(SOURCE.read_bytes()).hexdigest()==source_hash
assert hashlib.sha256(Path(exports['native']).read_bytes()).hexdigest()==saved_hash
report=dict(scope='Shared mating dimensions on proposed local hardpoint; not structural sizing',
    source=str(SOURCE),source_sha256=source_hash,physical=physical,bindings=bindings,
    nominal=baseline,reopened_spar_radius_2_1= spar_fit,reopened_boom_radius_5_1=boom_fit,
    source_and_saved_nominal_preserved=True,edited_skin_passage_verified=True,exports=exports,
    limitations=original['limitations']+['Radius ranges are geometric edit bounds, not approved component sizes or tolerances.'])
(OUT/'result.json').write_text(json.dumps(report,indent=2))
print('Reopened source radius edits propagate to spar, ribs, collar, boom seats and skin passage',flush=True)
