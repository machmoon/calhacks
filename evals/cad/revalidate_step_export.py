"""Regenerate a separate strictly checked STEP from a saved native selection.

Usage: run.sh evals/cad/revalidate_step_export.py <result.json> <output.step>
The report must name exports.native and physical (or physical_parts). Source
files are never overwritten; this does not upgrade old validation sidecars.
"""
import hashlib,json,sys
from pathlib import Path
import FreeCAD as App
import native
from exchange import export_step

report_path=Path(sys.argv[1]).resolve();output=Path(sys.argv[2]).resolve()
report=json.loads(report_path.read_text())
source=Path(report['exports']['native']).resolve()
names=report.get('physical',report.get('physical_parts'))
assert isinstance(names,list) and names and len(names)==len(set(names))
assert output.suffix.lower()=='.step' and not output.exists(), 'Use a new output path'
assert output!=source
source_hash=hashlib.sha256(source.read_bytes()).hexdigest()
doc=App.openDocument(str(source))
objects=[doc.getObject(name) for name in names]
assert all(obj is not None for obj in objects)
for obj in objects:native.describe(obj)
output.parent.mkdir(parents=True,exist_ok=True)
export_step(objects,output)
App.closeDocument(doc.Name)
assert hashlib.sha256(source.read_bytes()).hexdigest()==source_hash
validation=output.with_suffix('.step.validation.json')
data=json.loads(validation.read_text())
data.update(source_native=str(source),source_native_sha256=source_hash,
    source_selection_report=str(report_path),source_selection_report_sha256=hashlib.sha256(report_path.read_bytes()).hexdigest(),
    source_preserved=True,physical_names=names)
validation.write_text(json.dumps(data,indent=2))
print(json.dumps(dict(output=str(output),solids=data['solid_count'],method=data['method'],source_preserved=True)))
