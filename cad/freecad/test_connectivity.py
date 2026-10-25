"""Check geometric assembly connectivity independently of collision freedom."""
import json,os
from pathlib import Path
import FreeCAD as App
import native,worker
worker.document('ConnectivityTest');doc=worker.active()
for i,name in enumerate(['A','B','C']):
    native.make_part(doc,dict(name=name,kind='box',params=dict(x=10,y=10,z=10)))
    worker.place(name,[i*10,0,0])
joined=worker.check(['A','B','C'],require_connected=True)
assert joined['passed'] and joined['components']==[['A','B','C']]
worker.place('C',[21,0,0])
loose=worker.check(['A','B','C'])
assert loose['passed'] and not loose['connected'] and loose['components']==[['A','B'],['C']]
assert not worker.check(['A','B','C'],require_connected=True)['passed']
worker.place('C',[19,0,0])
collision=worker.check(['A','B','C'],require_connected=True)
assert not collision['passed'] and not collision['connected']
worker.place('C',[20.05,0,0])
assert worker.check(['A','B','C'],tolerance_mm=.1,require_connected=True)['passed']
assert not worker.check(['A','B','C'],tolerance_mm=.01,require_connected=True)['passed']
try:worker.check(['A','B','C'],require_connected='yes')
except ValueError:pass
else:raise AssertionError('invalid connectivity flag accepted')
native.make_part(doc,dict(name='Tube',kind='tube',params=dict(radius=5,bore_radius=4,height=10)))
try:worker.place('Tube_Bore',[20,0,0])
except ValueError:pass
else:raise AssertionError('placement invalidated a dependent cut without rejection')
assert doc.Tube_Bore.Placement.Base.Length<1e-8
assert abs(doc.Tube.Shape.Volume-90*__import__('math').pi)<1e-6
out=Path(os.environ['WRIGHT_CAD_OUT']);out.mkdir(parents=True,exist_ok=True)
worker.save(str(out/'visibility'),['Tube'])
assert doc.Tube.Visibility and not doc.Tube_Outer.Visibility and not doc.Tube_Bore.Visibility
App.closeDocument(doc.Name)
doc=App.openDocument(str(out/'visibility/assembly.FCStd'))
assert doc.Tube.Visibility and not doc.Tube_Outer.Visibility and not doc.Tube_Bore.Visibility
(out/'connectivity-tests.json').write_text(json.dumps(dict(passed=True,cases=['connected chain','separated part','collision is not attachment','contact tolerance','strict option type','dependent placement rollback','saved native visibility'])))
App.closeDocument(doc.Name)
print('Native geometric connectivity tests passed',flush=True)
