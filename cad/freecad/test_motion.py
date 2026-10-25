"""Motion checks must never call unsampled obstacles clear."""
import json,os
from pathlib import Path
import FreeCAD as App
import native,worker
worker.document('MotionTest');doc=worker.active()
for name,x in [('Moving',0),('Wall',5)]:
    native.make_part(doc,dict(name=name,kind='box',params=dict(x=1,y=1,z=1)))
    doc.getObject(name).Placement.Base=App.Vector(x,0,0)
doc.recompute()
before=worker.geometry_fingerprint([doc.Moving,doc.Wall])
call=worker.METHODS['check_translation']
clear=call(moving=['Moving'],obstacles=['Wall'],translation=[0,10,0])
assert clear['status']=='clear' and clear['continuous_envelope_clear']
blocked=call(moving=['Moving'],obstacles=['Wall'],translation=[10,0,0])
assert blocked['status']=='blocked' and blocked['collisions']
# Only endpoint samples miss the wall; conservative envelope must prevent PASS.
unknown=call(moving=['Moving'],obstacles=['Wall'],translation=[10,0,0],samples=2)
assert unknown['status']=='inconclusive' and not unknown['collisions']
assert worker.geometry_fingerprint([doc.Moving,doc.Wall])==before
for kwargs in [dict(samples=True),dict(samples=102),dict(translation=[0,0,0]),dict(obstacles=['Moving'])]:
    args=dict(moving=['Moving'],obstacles=['Wall'],translation=[10,0,0]);args.update(kwargs)
    try:call(**args)
    except ValueError:pass
    else:raise AssertionError('invalid motion request accepted')
out=Path(os.environ.get('WRIGHT_CAD_OUT','out/freecad-motion'));out.mkdir(parents=True,exist_ok=True)
(out/'result.json').write_text(json.dumps(dict(passed=True,clear=clear,blocked=blocked,inconclusive=unknown,source_geometry_preserved=True),indent=2))
print('Native motion screening passed clear, blocked and unsampled-obstacle cases')
