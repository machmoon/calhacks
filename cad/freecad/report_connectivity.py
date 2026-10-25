"""Derive geometric connectivity from a saved exhaustive pair report."""
import json,sys,hashlib
from pathlib import Path
from worker import contact_components
source=Path(sys.argv[1]).resolve();r=json.loads(source.read_text());fit=r['fit']
parts=fit['objects'];pairs=fit['pairs']
expected=len(parts)*(len(parts)-1)//2
assert len(pairs)==expected
assert len({tuple(sorted([p['a'],p['b']])) for p in pairs})==expected
components=contact_components(parts,pairs,fit['tolerance_mm'])
out=dict(source_report_sha256=hashlib.sha256(source.read_bytes()).hexdigest(),scope='Geometric contact connectivity derived from saved exhaustive native measurements; not joint strength',parts=len(parts),pairs=len(pairs),components=components,connected=len(components)==1,tolerance_mm=fit['tolerance_mm'])
(source.parent/'connectivity.json').write_text(json.dumps(out,indent=2))
print(json.dumps(dict(parts=len(parts),components=len(components),connected=out['connected'])))
