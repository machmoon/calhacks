"""Explicit native draft state never becomes verified through checkpoint/reopen."""
import json
from pathlib import Path
import FreeCAD as App
import Part
import worker,native,booleans,cut_proofs
OUT=Path(__file__).resolve().parents[2]/'out/freecad-deferred-cut-proofs';OUT.mkdir(parents=True,exist_ok=True)
checks={}
def rejects(label,fn):
 try:fn()
 except ValueError as e:checks[label]=str(e)
 else:raise AssertionError(label)
def close():
 d=worker.active();cut_proofs.clear(d);App.closeDocument(d.Name);worker._doc=None
# Source contains a genuinely no-effect cut: strict loading must reject it.
d=App.newDocument('DeferredFixture');d.UndoMode=1
b=d.addObject('Part::Box','Base');t=d.addObject('Part::Box','Tool');t.Placement.Base=App.Vector(100,0,0)
c=d.addObject('Part::Cut','SourceCut');c.Base=b;c.Tool=t;d.recompute();source=OUT/'source.FCStd';d.saveAs(str(source));App.closeDocument(d.Name)
rejects('strict_default_rejects',lambda:worker.open_document(str(source)))
r=worker.open_document(str(source),defer_source_cut_proofs=True);d=worker.active();assert r['cut_proofs']['pending_count']==1
checks['explicit_open_pending']=True
fit=worker.check(['SourceCut']);assert fit['pair_checks_passed'] and not fit['passed'] and not fit['geometry_verified'];checks['pending_check']=True
render=worker.render(['SourceCut'],width=64,height=64);assert not render['geometry_verified'];checks['pending_render']=True
rejects('checked_save_blocked',lambda:worker.save(str(OUT/'blocked'),['SourceCut']))
rejects('new_noop_strict',lambda:booleans.create(d,'BadNew','cut','Base','Tool'));assert d.getObject('BadNew') is None
# A changed source cut is no longer deferrable, even if it retains its name.
saved=d.SourceCut.Shape.copy();d.SourceCut.Shape=Part.makeBox(5,10,10,App.Vector(8,0,0))
rejects('changed_source_corruption_strict',lambda:native.validate_document(d))
d.SourceCut.Shape=saved
assert cut_proofs.status(d)['pending_count']==1;checks['revert_still_pending']=True
preview=worker.save_preview(str(OUT/'preview'),['SourceCut']);assert not preview['geometry_verified'];checks['preview_pending']=True
checkpoint=worker.checkpoint(str(OUT/'checkpoint'));assert checkpoint['cut_proofs']['pending_count']==1
close();rejects('marked_default_requires_explicit_mode',lambda:worker.open_document(checkpoint['checkpoint']))
r=worker.restore(checkpoint['checkpoint']);assert r['cut_proofs']['pending_count']==1;checks['restore_pending']=True
rejects('restored_checked_save_blocked',lambda:worker.save(str(OUT/'restored-blocked'),['SourceCut']))
# A real changed cut receives a strict proof and may clear that exact pending cut.
worker.place('Tool',[5,0,0]);assert cut_proofs.status(worker.active())['geometry_verified'];checks['changed_valid_cut_proved']=True
worker.checkpoint(str(OUT/'proved-checkpoint'))
(OUT/'result.json').write_text(json.dumps(checks,indent=2));print(json.dumps(checks),flush=True)
