"""Reopen saved pod iteration and prove its access geometry still recomputes."""
import hashlib,json
from pathlib import Path
import FreeCAD as App
ROOT=Path(__file__).resolve().parents[2];OUT=ROOT/'out/freecad-aircraft-pod'
source=OUT/'assembly.FCStd';before_sha=hashlib.sha256(source.read_bytes()).hexdigest()
doc=App.openDocument(str(source))
before=doc.part_pod_access_hatch.Shape.Volume
doc.PodAccessTool.Length=105;doc.recompute()
after=doc.part_pod_access_hatch.Shape.Volume
assert after>before and doc.part_pod.Shape.isValid() and doc.part_pod_access_hatch.Shape.isValid()
assert len(doc.part_pod.Shape.Solids)==1 and len(doc.part_pod_access_hatch.Shape.Solids)==1
App.closeDocument(doc.Name)
assert hashlib.sha256(source.read_bytes()).hexdigest()==before_sha
report=dict(passed=True,source_sha256=before_sha,edit='PodAccessTool.Length 100 -> 105 mm',hatch_volume_before_mm3=before,hatch_volume_after_mm3=after,source_file_unchanged=True)
(OUT/'reopen-check.json').write_text(json.dumps(report,indent=2));print(json.dumps(report))
