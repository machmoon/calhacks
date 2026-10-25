"""Native recursive-validity proof and exact-key cache negative controls."""
import hashlib,json,tempfile
from pathlib import Path
import FreeCAD as App
import Part
import native,step_import
key=lambda s:hashlib.sha256(s.cleaned().exportBrepToString().encode()).digest()
a=Part.makeBox(2,3,4);b=Part.makeCylinder(1,5)
inner=Part.makeCompound([a,b]);inner.Placement=App.Placement(App.Vector(8,2,3),App.Rotation(App.Vector(0,1,0),37))
outer=Part.makeCompound([inner]);outer.Placement=App.Placement(App.Vector(4,5,6),App.Rotation(App.Vector(1,0,0),19))
native._validity_cache.clear();leaves=native.validate_aggregate_leaves(outer)
assert len(leaves)==2 and all(key(s) in native._validity_cache for s in leaves)
assert all(s.isValid() and native.shape_is_valid(s.copy()) for s in leaves)
foreign=Part.makeBox(7,8,9);assert key(foreign) not in native._validity_cache
mutated=leaves[0].copy();mutated.translate(App.Vector(0,0,1));assert key(mutated) not in native._validity_cache
# Open shell masquerading as a solid: invalid child makes entire compound fail.
bad=Part.makeSolid(Part.makeShell(a.Faces[:-1]));assert not bad.isValid()
invalid=Part.makeCompound([Part.makeCompound([a,bad])]);before=dict(native._validity_cache)
try:native.validate_aggregate_leaves(invalid);raise AssertionError('accepted invalid child')
except ValueError:pass
assert dict(native._validity_cache)==before and key(bad) not in native._validity_cache
with tempfile.TemporaryDirectory() as directory:
 path=Path(directory)/'source.step';outer.exportStep(str(path));doc=App.newDocument('AggregateImport');doc.UndoMode=1
 original_labels=step_import._labels
 def forbidden(*args):raise AssertionError('labels read unexpectedly requested')
 step_import._labels=forbidden
 result=step_import.import_step(doc,str(path),'Imported',position=[3,4,5],axis=[1,1,0],angle_degrees=23,preserve_labels=False,summary=True)
 assert result['provenance']['labels_status']=='not_requested'
 shapes=[doc.getObject(n).Shape for n in result['object_names']]
 assert all(s.isValid() for s in shapes)
 cache_hits=sum(key(s) in native._validity_cache for s in shapes);assert cache_hits==2,cache_hits
 for value in [0,1,None,'false']:
  try:step_import.import_step(doc,str(path),'Bad',preserve_labels=value);raise AssertionError('bad option')
  except ValueError:pass
 step_import._labels=original_labels
 default=step_import.import_step(doc,str(path),'WithLabels',summary=True);assert default['provenance']['preserve_labels'] is True
 obj=doc.getObject(result['object_names'][0]);saved=obj.Shape.copy();obj.Shape=bad.copy();doc.recompute()
 try:native.validate_document(doc);raise AssertionError('invalid replacement trusted prior import proof')
 except ValueError:pass
 obj.Shape=saved;doc.recompute()
 file=Path(directory)/'reopen.FCStd';doc.saveAs(str(file));App.closeDocument(doc.Name);doc=App.openDocument(str(file));assert len(native.validate_document(doc))==4
 App.closeDocument(doc.Name)
previous_limit=native._VALIDITY_CACHE_LIMIT
try:
 native._VALIDITY_CACHE_LIMIT=2;native._validity_cache.clear()
 native.validate_aggregate_leaves(Part.makeCompound([Part.makeBox(1,1,1),Part.makeBox(2,2,2),Part.makeBox(3,3,3)]))
 assert len(native._validity_cache)==2
finally:native._VALIDITY_CACHE_LIMIT=previous_limit
print(json.dumps({'passed':True,'actual_placed_import_cache_hits':cache_hits,'recursive_invalid_child_rejected':True,'foreign_and_mutated_geometry_not_seeded':True,'labels_false_skips_second_read':True,'default_labels_preserved':True,'reopen_validated':True}))
