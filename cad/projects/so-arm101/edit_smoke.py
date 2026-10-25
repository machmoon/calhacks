"""Reversible native placement edit; not a physically valid joint-motion test."""
import hashlib,json
from pathlib import Path
import FreeCAD as App
ROOT=Path(__file__).resolve().parents[3];OUT=ROOT/'out/so-arm101-native';source=OUT/'official-assembly.FCStd';digest=hashlib.sha256(source.read_bytes()).hexdigest();doc=App.openDocument(str(source));obj=doc.getObject('Part__Feature009');original=App.Placement(obj.Placement);initial=obj.getGlobalPlacement().Base
changed=App.Placement(original);changed.Base.x+=1;obj.Placement=changed;doc.recompute();after=obj.getGlobalPlacement().Base;assert abs((after-initial).Length-1)<1e-8
out=OUT/'tests';out.mkdir(exist_ok=True);path=out/'placement-edit.FCStd';doc.saveAs(str(path));name=doc.Name;App.closeDocument(name);doc=App.openDocument(str(path));obj=doc.getObject('Part__Feature009');assert abs((obj.getGlobalPlacement().Base-initial).Length-1)<1e-8
obj.Placement=original;doc.recompute();assert (obj.getGlobalPlacement().Base-initial).Length<1e-8
assert hashlib.sha256(source.read_bytes()).hexdigest()==digest
(OUT/'edit-smoke.json').write_text(json.dumps(dict(source_native_sha256=digest,object_name=obj.Name,label=obj.Label,local_x_translation_mm=1,translation_persisted_after_reopen=True,reverted_in_memory=True,source_unchanged=True,edited_test_file=str(path),scope='Isolated placement-edit persistence only; not valid assembly kinematics, clearance or joint constraints'),indent=2));print('Native placement edit/reopen/revert passed; source preserved',flush=True)
