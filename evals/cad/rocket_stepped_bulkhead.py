"""Explicit derived joint proposal; never rewrites the pinned OpenRocket source."""
import hashlib,json,math
from pathlib import Path
import FreeCAD as App
import worker
ROOT=Path(__file__).resolve().parents[2];OUT=ROOT/'out/rocket-stepped-bulkhead';OUT.mkdir(parents=True,exist_ok=True)
evidence=ROOT/'out/openrocket-assembly-fit/source-overlap-analysis.json';source=json.loads(evidence.read_text());dims=source['dimensions_mm']
source_path=ROOT/'evals/cad/references'/source['source']['file'];digest=hashlib.sha256(source_path.read_bytes()).hexdigest();assert digest==source['source']['sha256']
call=worker.METHODS;worker.document('DerivedBulkheadSeat');doc=worker.active();trace=[]
def action(action_name,**args):
 result=call[action_name](**args);trace.append(dict(action=action_name,args=args));return result
r=dims['coupler_radius'];bore=dims['coupler_bore_radius'];length=dims['coupler_length'];cap_r=dims['bulkhead_radius'];depth=3.;gap=.15;cap_t=dims['bulkhead_length']-depth
assert cap_t>0
# Source pair reproduced independently; retain the source conflict as a negative control.
action('make_part',spec=dict(name='Coupler',kind='tube',params=dict(radius=r,bore_radius=bore,height=length)))
action('make_part',spec=dict(name='OriginalBulkhead',kind='cylinder',params=dict(radius=cap_r,height=dims['bulkhead_length'])))
action('place',object_name='OriginalBulkhead',position=[0,0,dims['bulkhead_start_from_parent']])
original=action('check',objects=['Coupler','OriginalBulkhead'],summary=False)
assert not original['passed'] and abs(original['pairs'][0]['overlap_mm3']-source['analytic_source_overlap_mm3'])<1e-5
# Derived stepped proposal: unchanged total thickness/rim radius; pilot set back into bore.
action('make_parameters',name='SeatDimensions',parameters={'BoreRadius':dict(value=bore,min=48,max=49.1),'CouplerLength':dict(value=length,min=122,max=135)})
for name,rad,height,z in [('Pilot',bore-gap,depth,length-depth),('Flange',cap_r,cap_t,length)]:
 action('make_part',spec=dict(name=name,kind='cylinder',params=dict(radius=rad,height=height)))
 action('place',object_name=name,position=[0,0,z])
action('boolean',name='SteppedBulkhead',operation='fuse',base='Pilot',tool='Flange')
for obj,param,source_param,offset in [('Coupler_Bore','Radius','BoreRadius',0),('Pilot','Radius','BoreRadius',-gap),('Coupler_Outer','Height','CouplerLength',0),('Coupler_Bore','Height','CouplerLength',0),('Pilot','Placement.Base.z','CouplerLength',-depth),('Flange','Placement.Base.z','CouplerLength',0)]:
 action('bind_parameter',object_name=obj,parameter=param,source_object='SeatDimensions',source_parameter=source_param,offset_mm=offset)
states=[]
def verify():
 check=action('check',objects=['Coupler','SteppedBulkhead'],contacts=[['Coupler','SteppedBulkhead']],tolerance_mm=1e-6,require_connected=True)
 assert check['passed'] and check['pairs'][0]['overlap_mm3']<1e-6
 actual_gap=float(doc.Coupler_Bore.Radius)-float(doc.Pilot.Radius);assert abs(actual_gap-gap)<1e-8
 expected=math.pi*(float(doc.Pilot.Radius)**2*depth+cap_r**2*cap_t)
 assert len(doc.SteppedBulkhead.Shape.Solids)==1 and abs(doc.SteppedBulkhead.Shape.Volume-expected)<1e-5
 return dict(bore_radius_mm=float(doc.SeatDimensions.BoreRadius),coupler_length_mm=float(doc.SeatDimensions.CouplerLength),radial_pilot_clearance_mm=actual_gap,check=check)
states.append(verify())
action('set_parameter',object_name='SeatDimensions',parameter='BoreRadius',value=bore+.5,summary=True)
action('set_parameter',object_name='SeatDimensions',parameter='CouplerLength',value=132,summary=True);states.append(verify())
action('set_parameter',object_name='SeatDimensions',parameter='BoreRadius',value=bore,summary=True)
action('set_parameter',object_name='SeatDimensions',parameter='CouplerLength',value=length,summary=True);states.append(verify())
doc.OriginalBulkhead.Visibility=False;doc.saveAs(str(OUT/'derived-seat.FCStd'));App.closeDocument(doc.Name);worker._doc=None;worker.open_document(str(OUT/'derived-seat.FCStd'));doc=worker.active();states.append(verify())
assert hashlib.sha256(source_path.read_bytes()).hexdigest()==digest
(OUT/'typed-actions.json').write_text(json.dumps(trace,indent=2))
(OUT/'result.json').write_text(json.dumps(dict(geometry_checks_passed=True,complete_rocket_design_verified=False,source=source['source'],source_unchanged=True,original_interference=original,derived_changes=dict(pilot_depth_mm=depth,radial_clearance_mm=gap,flange_thickness_mm=cap_t,original_total_thickness_preserved=True,original_rim_radius_preserved=True),states=states,reopened=True,scope='Explicit derived stepped-seat proposal. Face bearing and geometric clearance verified only. No positive axial retention, fasteners, adhesive, pressure/load strength, recovery attachment or manufacturing-tolerance validation.'),indent=2))
print('Derived stepped seat: source conflict retained, clearance/contact, linked edits and reopen pass')
