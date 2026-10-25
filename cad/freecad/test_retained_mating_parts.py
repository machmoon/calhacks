"""Cut a mating seat while retaining the original mating component as output."""
import json
from pathlib import Path
import FreeCAD as App
import worker
ROOT=Path(__file__).resolve().parents[2]; OUT=ROOT/'out/freecad-retained-mating-parts'
OUT.mkdir(parents=True,exist_ok=True)
worker.document('RetainedMatingParts'); call=worker.METHODS; checks=[]
def reject(label,fn):
    try:fn()
    except ValueError:checks.append(label)
    else:raise AssertionError(label)
for name,dims in [('Blank',(20,20,10)),('Mate',(10,10,20))]:
    call['make_part'](spec=dict(name=name,kind='box',params=dict(zip(['x','y','z'],dims))))
call['place'](object_name='Mate',position=[5,5,-5])
call['boolean'](name='Seat',operation='cut',base='Blank',tool='Mate',retain_tool_as_part=True)
fit=call['check'](objects=['Seat','Mate'],contacts=[['Seat','Mate']],require_connected=True)
assert fit['passed']; checks.append('retained_seat_and_mate_pass')
reject('base_still_rejected',lambda:worker.selected_objects(['Seat','Blank']))
call['boolean'](name='DefaultSeat',operation='cut',base='Blank',tool='Mate')
reject('default_tool_still_rejected',lambda:worker.selected_objects(['DefaultSeat','Mate']))
reject('retain_union_rejected',lambda:call['boolean'](name='Bad',operation='fuse',base='Blank',tool='Mate',retain_tool_as_part=True))
call['boolean'](name='Refilled',operation='fuse',base='Seat',tool='Mate')
reject('nested_retention_cannot_hide_overlap',lambda:worker.selected_objects(['Refilled','Mate']))
checkpoint=worker.checkpoint(str(OUT))['checkpoint']
App.closeDocument(worker.active().Name);worker._doc=None;worker.restore(checkpoint)
assert worker.active().Seat.WrightRetainToolAsPart
call['set_parameter'](object_name='Mate',parameter='Width',value=12)
fit=call['check'](objects=['Seat','Mate'],contacts=[['Seat','Mate']],require_connected=True)
assert fit['passed'];checks.append('reopened_mate_edit_recomputes_seat')
exports=worker.save(str(OUT),['Seat','Mate'])
assert exports['preview']['physical_nodes']==2 and exports['roundtrip_valid']
checks.append('both_mating_parts_exported')
(OUT/'result.json').write_text(json.dumps(dict(passed=True,checks=checks,fit=fit,exports=exports),indent=2))
print('Retained mating-part checks passed:',len(checks))
