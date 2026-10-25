"""Actual curved skin regression and adversarial cut material checks."""
import json
from pathlib import Path
import FreeCAD as App
import Part
import booleans
import native
ROOT=Path(__file__).resolve().parents[2]
OUT=ROOT/'out/freecad-curved-cut-validation';OUT.mkdir(parents=True,exist_ok=True)
checks={}
def reject(label,fn):
    try:fn()
    except ValueError as e:checks[label]=str(e)
    else:raise AssertionError(label+' accepted')
base=Part.makeBox(10,10,10)
reject('no_op',lambda:booleans.validate_cut_material(base,base.copy()))
# Smaller net volume still adds material outside the base.
corrupt=Part.makeBox(5,10,10,App.Vector(8,0,0))
assert base.Volume>corrupt.Volume
reject('additive_positive_scalar_delta',lambda:booleans.validate_cut_material(base,corrupt))
reject('disconnected',lambda:booleans.validate_cut_material(base,Part.makeCompound([Part.makeBox(1,1,1),Part.makeBox(1,1,1,App.Vector(5,5,5))])))
reject('null',lambda:booleans.validate_cut_material(base,Part.Shape()))
invalid=Part.Face(Part.makePolygon([App.Vector(0,0,0),App.Vector(10,10,0),App.Vector(0,10,0),App.Vector(10,0,0),App.Vector(0,0,0)]))
assert not invalid.isValid()
reject('invalid',lambda:booleans.validate_cut_material(base,invalid))
# Successful proof cache is geometry keyed, bounded, and rejects a moved result.
old_limit=booleans._CUT_PROOF_CACHE_LIMIT
booleans._CUT_PROOF_CACHE_LIMIT=2
try:
    for width in [4,5,6]:booleans.validate_cut_material(base,Part.makeBox(width,10,10))
    assert len(booleans._cut_proof_cache)==2
    moved=Part.makeBox(6,10,10);moved.translate(App.Vector(8,0,0))
    reject('changed_placement_not_cached',lambda:booleans.validate_cut_material(base,moved))
    checks['bounded_cache']=True
finally:booleans._CUT_PROOF_CACHE_LIMIT=old_limit
# Native Cut placement is separate from its operand-coordinate construction.
placed=App.newDocument('PlacedCutProof');placed.UndoMode=1
pb=placed.addObject('Part::Box','Base');pb.Placement.Base=App.Vector(10,20,30)
pt=placed.addObject('Part::Box','Tool');pt.Placement.Base=App.Vector(15,20,30)
pc=placed.addObject('Part::Cut','Cut');pc.Base=pb;pc.Tool=pt;placed.recompute()
pc.Placement=App.Placement(App.Vector(100,200,300),App.Rotation(App.Vector(0,1,0),90));placed.recompute()
native.validate_document(placed);checks['translated_rotated_cut']=True
App.closeDocument(placed.Name)
print('Negative controls passed; opening actual curved fixture',flush=True)
a=App.openDocument(str(ROOT/'out/tsung-vtol-smooth-aircraft-final/assembly.FCStd'))
b=App.openDocument(str(ROOT/'out/tsung-vtol-wing-hardpoint-linked/assembly.FCStd'))
b.SparWeb.Width=16;b.SparWeb.setExpression('Placement.Base.y','HardpointControls.BoomY - 8 mm')
b.SparCollarBlank.Height=16;b.SparCollarBlank.setExpression('Placement.Base.y','HardpointControls.BoomY - 8 mm');b.recompute()
doc=App.newDocument('CurvedCutValidation');doc.UndoMode=1
for name,shape in [('Base',a.RightSkin.Shape),('Tool',b.HPNutSeat1.Shape)]:
    obj=doc.addObject('Part::Feature',name);obj.Shape=shape.copy()
doc.recompute()
print('Creating actual16mm typed cut',flush=True)
booleans.create(doc,'Cut','cut','Base','Tool')
cut=doc.Cut
assert doc.Base.Shape.Volume-cut.Shape.Volume<0
removed=doc.Base.Shape.cut(cut.Shape);added=cut.Shape.cut(doc.Base.Shape)
assert removed.Volume>150 and not added.Solids
checks['actual_16mm']={'scalar_delta_mm3':doc.Base.Shape.Volume-cut.Shape.Volume,'removed_mm3':removed.Volume,'added_solids':len(added.Solids)}
# Both typed and legacy gates must use the same geometric predicate.
cut.WrightTypedBoolean=False;native.validate_document(doc)
checks['legacy_actual_16mm']=True
saved=cut.Shape.copy();cut.Shape=corrupt
reject('legacy_corrupted',lambda:native.validate_document(doc))
cut.Shape=saved;cut.WrightTypedBoolean=True
path=OUT/'assembly.FCStd';doc.recompute();doc.saveAs(str(path));name=doc.Name;App.closeDocument(name)
doc=App.openDocument(str(path));doc.UndoMode=1;native.validate_document(doc);checks['reopen']=True
far=doc.addObject('Part::Feature','Distant');far.Shape=Part.makeBox(1,1,1,App.Vector(10000,0,0));doc.recompute()
reject('typed_no_op_rollback',lambda:booleans.create(doc,'NoOp','cut','Base','Distant'))
assert doc.getObject('NoOp') is None
(OUT/'result.json').write_text(json.dumps(checks,indent=2));print(json.dumps(checks),flush=True)
