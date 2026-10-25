"""Native split-clamp study on the reconstructed right tail, via typed actions.
This is a proposed joint, not a reconstruction of unpublished author internals.
"""
import hashlib, json, os, shutil
from pathlib import Path
import FreeCAD as App
import worker

ROOT=Path(__file__).resolve().parents[2]
SOURCE=Path(os.environ.get('WRIGHT_VTOL_NATIVE', ROOT/'out/tsung-vtol-reference/assembly.FCStd'))
OUT=ROOT/'out/tsung-vtol-tail-joint'; OUT.mkdir(parents=True,exist_ok=True)
shutil.copy2(SOURCE,OUT/'input.FCStd'); SOURCE=OUT/'input.FCStd'
source_hash=hashlib.sha256(SOURCE.read_bytes()).hexdigest()
worker.open_document(str(SOURCE)); doc=worker.active(); trace=[]
def act(action, **args):
    result=worker.METHODS[action](**args)
    trace.append(dict(action=action,args=args,result=result))
    print(action,args.get('name',args.get('object_name','')),flush=True)
    return result
def part(name,kind,params,position,axis=(0,0,1),angle=0):
    act('make_part',spec=dict(name=name,kind=kind,params=params))
    act('place',object_name=name,position=position,axis=axis,angle_degrees=angle)
    return name
def boolean(name,operation,base,tool,retain_tool_as_part=False):
    act('boolean',name=name,operation=operation,base=base,tool=tool,retain_tool_as_part=retain_tool_as_part)
    return name

# Coordinates come from the explicitly inferred reference layout. Clearances,
# ear dimensions and hole diameter below are new design assumptions.
part('JointCollarOuter','cylinder',dict(radius=9,height=20),[665,220,-10],[0,1,0],90)
part('JointEars','box',dict(x=20,y=40,z=8),[665,200,-14])
boolean('JointBlank','fuse','JointCollarOuter','JointEars')
part('JointUpperHalfspace','box',dict(x=25,y=45,z=12),[662.5,197.5,-10])
part('JointLowerHalfspace','box',dict(x=25,y=45,z=12),[662.5,197.5,-22])
boolean('JointUpperBlank','common','JointBlank','JointUpperHalfspace')
boolean('JointLowerBlank','common','JointBlank','JointLowerHalfspace')
boolean('FinTopSeat','cut','FinRight','TopStabilizer',retain_tool_as_part=True)
boolean('FinCollarBlank','fuse','FinTopSeat','JointUpperBlank')
# Extend beyond both boom ends, avoiding an unintended contact at the cutter's
# end cap while claiming a radial assembly clearance.
part('JointBoomClearance','cylinder',dict(radius=5.2,height=800),[0,220,-10],[0,1,0],90)
upper=boolean('FinBoomSeat','cut','FinCollarBlank','JointBoomClearance')
lower=boolean('JointLowerSeat','cut','JointLowerBlank','JointBoomClearance')
for index,y in enumerate([206,234]):
    hole=part(f'JointBoltTool{index}','cylinder',dict(radius=1.7,height=16),[675,y,-20])
    upper=boolean(f'FinClampDrill{index}','cut',upper,hole)
    lower=boolean(f'LowerClampDrill{index}','cut',lower,hole)
doc.getObject(upper).Label='Right fin with split clamp — proposed, unvalidated'
doc.getObject(lower).Label='Right tail clamp lower half — proposed, unvalidated'
physical=['BoomRight',upper,lower,'TopStabilizer']
contacts=[[upper,lower],[upper,'TopStabilizer']]
fit=act('check',objects=physical,contacts=contacts,require_connected=False)
act('checkpoint',directory=str(OUT/'construction'))
assert fit['overlap_count']==0 and fit['unmet_contact_count']==0
gaps={n:doc.getObject(n).Shape.distToShape(doc.BoomRight.Shape)[0] for n in [upper,lower]}
assert all(abs(v-.2)<1e-5 for v in gaps.values()),gaps
removal=act('check_translation',moving=[lower],obstacles=[upper,'BoomRight','TopStabilizer'],translation=[0,0,-30])
# Swept AABBs may be inconclusive on the enclosing joint; never force a PASS.
exports=act('save',directory=str(OUT),objects=physical)
saved_hash=hashlib.sha256(Path(exports['native']).read_bytes()).hexdigest()
App.closeDocument(doc.Name)
worker._doc=None  # Same fresh-session boundary as a restarted JSON-RPC worker.
act('restore',path=exports['native'])
# Changing the common clearance tool must recompute both clamp halves.
act('set_parameter',object_name='JointBoomClearance',parameter='Radius',value=5.3)
edited_gaps={n:worker.active().getObject(n).Shape.distToShape(worker.active().BoomRight.Shape)[0] for n in [upper,lower]}
assert all(abs(v-.3)<1e-5 for v in edited_gaps.values()),edited_gaps
assert hashlib.sha256(SOURCE.read_bytes()).hexdigest()==source_hash
assert hashlib.sha256(Path(exports['native']).read_bytes()).hexdigest()==saved_hash
report=dict(scope='Proposed native tail root split clamp and top seat on reference geometry',
    source=str(SOURCE),source_sha256=source_hash,physical=physical,fit=fit,
    nominal_radial_clearance_mm=gaps,edited_clearance_mm=edited_gaps,
    removal=removal,exports=exports,source_preserved=True,
    action_count=len(trace),uses_typed_api=True,original_author_joint=False,
    assumptions=dict(collar_length_mm=20,collar_outer_radius_mm=9,
        boom_radius_mm=5,clearance_radius_mm=5.2,ear_width_mm=40,
        ear_total_thickness_mm=8,bolt_clearance_diameter_mm=3.4),
    mechanical_review='Revision required: split faces already meet while radial clearance remains; no rigid-body clamping travel is available',
    limitations=['Split faces meet before radial gap closes; this geometry does not yet establish boom retention',
        'No fasteners or preload modeled; clamp compliance and grip unverified',
        '0.2mm radial clearance is a design assumption, not a validated clamping fit',
        'Fin and stabilizer remain exterior solid envelopes pending internal structure',
        'This isolated joint does not establish full-aircraft connectivity or strength',
        'Top stabilizer seat has nominal contact but retention hardware remains missing'])
(OUT/'result.json').write_text(json.dumps(report,indent=2))
(OUT/'action-trace.json').write_text(json.dumps(trace,indent=2))
print(json.dumps(dict(clashes=fit['overlap_count'],gaps=gaps,reopened_edit=edited_gaps,removal=removal['status'])))
