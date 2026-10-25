"""Native negative controls for optimized checks and display raster parity."""
import sys,tempfile,os
from pathlib import Path
ROOT=Path(__file__).resolve().parents[3]
sys.path.insert(0,str(ROOT/'cad/freecad'))
import FreeCAD as App,Part
import worker,native,lift_sweep,service_access,surface_preview
import numpy as np
from PIL import Image
# A coarse disk overlaps a motor, but the native inner hub is entirely above it.
prop=Part.makeCylinder(10,2,App.Vector(0,0,10)).cut(Part.makeCylinder(3,2,App.Vector(0,0,10))).fuse(Part.makeBox(20,2,10,App.Vector(20,-1,0)))
motor=Part.makeCylinder(15,9)
assert lift_sweep.refined_motor_envelope(prop,App.Placement(),motor,'fixture')['overlap_mm3']<1e-5
# Same geometry/cache with a moved motor must not reuse its old clearance verdict.
raised=motor.copy();raised.translate(App.Vector(0,0,3))
assert lift_sweep.refined_motor_envelope(prop,App.Placement(),raised,'fixture')['overlap_mm3']>1
# A blade intrusion outside the hub must also remain a collision.
assert lift_sweep.refined_motor_envelope(prop,App.Placement(),Part.makeCylinder(25,9))['overlap_mm3']>1
worker.document('OptimizationControls');doc=worker.active()
hatch=doc.addObject('Part::Feature','Hatch');hatch.Shape=Part.makeBox(10,10,2)
obstacle=doc.addObject('Part::Feature','Obstacle');obstacle.Shape=Part.makeBox(10,10,2,App.Vector(0,0,10));doc.recompute()
assert not service_access.hatch_removal(doc,['Hatch','Obstacle'])['passed']
obstacle.Placement.Base=App.Vector(30,0,0);doc.recompute()
assert service_access.hatch_removal(doc,['Hatch','Obstacle'])['passed']
for n,x in [('A',0),('B',5),('C',40)]:
 native.make_part(doc,dict(name=n,kind='box',params=dict(x=10,y=10,z=10)));worker.place(n,[x,0,0])
full=worker.check(['A','B','C'],contacts=[['A','C']])
fast=worker.check(['A','B','C'],contacts=[['A','C']],exhaustive_distances=False)
assert full['overlap_count']==fast['overlap_count']==1
assert full['unmet_contact_count']==fast['unmet_contact_count']==1
assert fast['connected'] is None and not fast['connectivity_evaluated']
try:worker.check(['A','B'],require_connected=True,exhaustive_distances=False)
except ValueError:pass
else:raise AssertionError('Unmeasured connectivity was certified')
with tempfile.TemporaryDirectory() as folder:
 p=Path(folder);obj=doc.A
 os.environ['WRIGHT_RASTER_BACKEND']='python';surface_preview.selected([obj],p/'reference.png',width=160,height=120)
 os.environ.pop('WRIGHT_RASTER_BACKEND');result=surface_preview.selected([obj],p/'rust.png',width=160,height=120)
 assert result['raster_backend']=='rust'
 x=np.asarray(Image.open(p/'reference.png')).astype(int);y=np.asarray(Image.open(p/'rust.png')).astype(int)
 assert abs(x-y).max()<=1
 before=obj.Shape.exportBrepToString()
 clipped=surface_preview.selected([obj],p/'clipped.png',width=160,height=120,clip_planes={obj.Name:(2,5)})
 assert clipped['bounds_mm'][1][2]<=5.000001
 assert obj.Shape.exportBrepToString()==before
 obj.Placement.Base=App.Vector(17,0,0);doc.recompute()
 moved=surface_preview.selected([obj],p/'moved.png',width=160,height=120)
 assert abs(moved['bounds_mm'][0][0]-17)<1e-5, 'Stale display cache survived a native placement edit'
 # Display translations must equal displaced native geometry without changing CAD.
 original=obj.Shape.exportBrepToString()
 surface_preview.selected([obj,doc.C],p/'display-shift.png',width=160,height=120,translations={obj.Name:(0,20,5)})
 assert obj.Shape.exportBrepToString()==original
 obj.Placement.Base=obj.Placement.Base+App.Vector(0,20,5);doc.recompute()
 surface_preview.selected([obj,doc.C],p/'native-shift.png',width=160,height=120)
 assert np.array_equal(np.asarray(Image.open(p/'display-shift.png')),np.asarray(Image.open(p/'native-shift.png')))
 import display_raster
 try:display_raster.rasterize(np.full((1,3,3),float('nan')),np.zeros((1,3,3)),np.zeros((1,3)),64,64)
 except ValueError:pass
 else:raise AssertionError('Nonfinite display geometry accepted')
print('PASS: refined motor/shaft and blade intrusion, moved-motor cache, blocked hatch removal, collision/contact parity, no false connectivity, native raster parity')
