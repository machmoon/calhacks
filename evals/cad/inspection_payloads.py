"""Measure discovery payloads on a real native VTOL; no model-quality claim."""
import hashlib,json,time
from pathlib import Path
import worker
ROOT=Path(__file__).resolve().parents[2]
SOURCE=ROOT/'out/tsung-vtol-tail-joint/assembly.FCStd'
before=hashlib.sha256(SOURCE.read_bytes()).hexdigest()
worker.open_document(str(SOURCE))
results={}
for label,args in [('full',{}),('discovery_page',dict(summary=True,limit=100)),
                   ('selected_joint',dict(objects=['BoomRight','FinClampDrill1','LowerClampDrill1','TopStabilizer']))]:
    started=time.perf_counter();data=worker.inspect_tree(**args)
    results[label]=dict(elapsed_seconds=time.perf_counter()-started,json_bytes=len(json.dumps(data).encode()),
        returned_objects=len(data['objects'] if isinstance(data,dict) else data))
assert results['selected_joint']['returned_objects']==4
assert results['selected_joint']['json_bytes']<results['full']['json_bytes']
assert hashlib.sha256(SOURCE.read_bytes()).hexdigest()==before
out=ROOT/'out/freecad-inspection-payloads';out.mkdir(parents=True,exist_ok=True)
report=dict(source=str(SOURCE),source_sha256=before,measurements=results,source_preserved=True,
    timing_scope='Single local run, warm native document; payload size is deterministic but timing is load-dependent',
    model_usage_reduction_measured=False)
(out/'result.json').write_text(json.dumps(report,indent=2));print(json.dumps(results))
