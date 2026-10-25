"""Make evaluator negatives from baseline copies; preserve source and AI artifacts."""
import hashlib,json
from pathlib import Path
import FreeCAD as App
ROOT=Path(__file__).resolve().parents[3];source=ROOT/'out/so-arm101-candidate/assembly.FCStd';out=ROOT/'out/so-arm101-evaluation/native-negatives';out.mkdir(exist_ok=True)
before=hashlib.sha256(source.read_bytes()).hexdigest();doc=App.openDocument(str(source));removed=[]
for obj in doc.Objects:
 for prop,expression in list(obj.ExpressionEngine):
  if 'JointParameters.' in expression:
   obj.setExpression(prop,None);removed.append([obj.Name,prop,expression])
assert len(removed)==6;doc.recompute();doc.saveAs(str(out/'frozen-controls.FCStd'));App.closeDocument(doc.Name)
doc=App.openDocument(str(source));doc.removeObject('Visual_base_link_0');doc.recompute();doc.saveAs(str(out/'omitted-instance.FCStd'));App.closeDocument(doc.Name)
doc=App.openDocument(str(source));extra=doc.addObject('Part::Box','UnexpectedPhysicalBox');extra.Length=10;extra.Width=10;extra.Height=10;doc.recompute();doc.saveAs(str(out/'extra-box.FCStd'));App.closeDocument(doc.Name)
assert hashlib.sha256(source.read_bytes()).hexdigest()==before
(out/'manifest.json').write_text(json.dumps(dict(source=str(source),source_sha256=before,source_unchanged=True,removed_expressions=removed),indent=2));print('Native negative fixtures saved; source unchanged')
