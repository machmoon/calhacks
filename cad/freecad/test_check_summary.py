"""Compact responses preserve full computation, failures and export evidence."""
import json
from pathlib import Path
import FreeCAD as App
import native,worker
OUT=Path(__file__).resolve().parents[2]/'out/freecad-check-summary'
OUT.mkdir(parents=True,exist_ok=True)
worker.document('CompactChecks');doc=worker.active()
for name,x in [('A',0),('B',5),('C',15),('D',40)]:
    native.make_part(doc,dict(name=name,kind='box',params=dict(x=10,y=10,z=10)))
    worker.place(name,[x,0,0])
names=['A','B','C','D'];contacts=[['A','D'],['B','C']]
full=worker.check(names,contacts,require_connected=True)
compact=worker.check(names,contacts,require_connected=True,summary=True)
for key in ['passed','connected','components','pairs_total','overlap_count','unmet_contact_count']:
    assert compact[key]==full[key],key
assert compact['pairs_total']==6 and len(compact['pairs'])==3
assert compact['omitted_pair_count']==3 and compact['contact_pair_count']==1
assert compact['pairs']==[p for p in full['pairs'] if p['clash'] or p['required_contact']]
assert compact['overlap_count']==1 and compact['unmet_contact_count']==1
compact['pairs'].clear()
selected=worker.selected_objects(names)
assert worker.current_check(selected)['pairs']==full['pairs']
assert 'omitted_pair_count' not in worker.current_check(selected)
exports=worker.save(str(OUT),names)
persisted=json.loads((OUT/'geometry-check.json').read_text())
assert persisted['pairs']==full['pairs'] and len(persisted['pairs'])==6
worker.place('D',[50,0,0])
assert worker.current_check(selected)=={'status':'not_checked'}
try:worker.check(names,summary=1)
except ValueError:pass
else:raise AssertionError('nonboolean summary accepted')
(OUT/'result.json').write_text(json.dumps(dict(passed=True,full_pair_count=6,
    compact_pair_count=3,all_failures_preserved=True,complete_saved_evidence=True,
    source_mutation_invalidates_evidence=True,exports=exports),indent=2))
print('Compact checks: all pairs computed, failures retained, full export and freshness passed')
