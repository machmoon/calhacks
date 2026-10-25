"""Requirement-level aggregation; missing evidence never becomes a VTOL pass."""
import hashlib,json
from pathlib import Path
GATES={
 'frozen_hardware_package':'Exact selected hardware and compatibility evidence frozen before detailed mounts',
 'bom_geometry_coverage':'Every BOM instance accounted for with honest geometry/reference scope',
 'electrical_and_wiring':'Connector-to-connector power/signal/wire/protection schedule verified',
 'native_geometry':'Current physical native geometry validates',
 'all_pair_collisions':'Every physical pair checked; declared contacts never excuse penetration',
 'attachment_contacts':'Declared mating contacts measured on current geometry',
 'propeller_sweeps':'Lift/cruise propeller swept volumes clear',
 'control_and_linkage_motion':'Full control-surface/servo/linkage travel checked',
 'service_access':'Battery/board/hatch/fastener/tool insertion and removal checked',
 'connector_thermal_routing':'Connector access, wiring retention and thermal/airflow provisions reviewed',
 'load_paths':'Stated loads and joint/structural sizing independently justified',
 'span_chord_boom_servo_battery_edits':'All five requested parameter cases update expected geometry',
 'native_reopen':'Saved native model reopens and retains editable dependencies',
 'exports':'Required exports pass stated equivalence checks for this revision',
 'current_wing_analysis':'Current wing geometry analyzed with required solvers and honest wing-only scope',
 'mass_propulsion_performance':'Mass/CG, propulsion, power and performance reconciled',
 'views_and_assembly_evidence':'Assembled/exploded/cutaway views and assembly sequence tied to revision'}

MISSING_EVIDENCE={
 'bom_geometry_coverage':'Exact/proxy/missing classification and placed geometry for every purchased BOM instance.',
 'electrical_and_wiring':'Verified connector pinouts, net-to-net schedule, voltage/current/protection and conductor ratings for selected variants.',
 'control_and_linkage_motion':'Purchased servo/horn interfaces, hinge axes, linkage geometry and continuous travel/clearance checks.',
 'connector_thermal_routing':'Connector insertion envelopes, cable bend/retention clearances, cooling loads and airflow evidence.',
 'load_paths':'Current-revision material/process properties, loads, fastener preload/engagement and independent structural sizing.',
 'views_and_assembly_evidence':'Current-revision assembled/exploded/cutaway images plus a checked assembly/removal sequence.',
 'mass_propulsion_performance':'Revision-bound native mass/material/CG reconciliation and selected catalog hardware mapped into Wright-air design/spec.',
}
GATE_PROBES={'native_geometry':['geometry'],'all_pair_collisions':['pairs'],'attachment_contacts':['pairs'],
 'span_chord_boom_servo_battery_edits':['edits'],'native_reopen':['reopen'],'propeller_sweeps':['sweeps'],
 'service_access':['access'],'current_wing_analysis':['aero'],'exports':['exports'],
 'frozen_hardware_package':['package_inventory'],'bom_geometry_coverage':['engineering-inputs'],
 'mass_propulsion_performance':['wright-air binding']}

def sha(path):return hashlib.sha256(Path(path).read_bytes()).hexdigest()
def aggregate(revision,native_sha256,evidence):
 gates={key:{'status':'pending','requirement':requirement,'reason':MISSING_EVIDENCE.get(key,'No current-revision independent evidence supplied'),'probes':GATE_PROBES.get(key,[])} for key,requirement in GATES.items()}
 seen=set()
 for item in evidence:
  name=item['gate']
  if name in seen:raise ValueError('duplicate gate evidence; do not overwrite failures')
  seen.add(name)
  if name not in gates:raise ValueError('unknown gate')
  if item.get('revision')!=revision or item.get('native_sha256')!=native_sha256:
   gates[name].update(status='pending',reason='Evidence revision or native geometry hash mismatch');continue
  if item.get('status') not in {'passed','failed','pending','inconclusive'}:raise ValueError('invalid evidence status')
  if item['status']=='passed' and not item.get('measurements'):raise ValueError('passing gate requires measurements')
  gates[name].update({k:v for k,v in item.items() if k not in {'gate','revision','native_sha256'}})
 return {'revision':revision,'native_sha256':native_sha256,'passed':all(g['status']=='passed' for g in gates.values()),'gates':gates,'scope':'Requirement-level VTOL demonstrator evidence, not flight testing or one-shot model generalization','pending_or_failed':[k for k,v in gates.items() if v['status']!='passed']}

def package_inventory(path):
 path=Path(path);names=['requirements.json','bom.json','electrical-architecture.json','component-cad-manifest.json','interface-table.json','verification-plan.json']
 return {name:{'present':(path/name).is_file(),'sha256':sha(path/name) if (path/name).is_file() else None} for name in names}

def collision_gate_status(probe):
 """A clear geometric subset cannot waive failed/missing proof or full BOM coverage."""
 check=probe.get('check',{})
 if probe.get('status')!='complete' or check.get('geometry_verified') is not True or check.get('overlap_count',0)>0:return 'failed'
 coverage=probe.get('coverage',{})
 return 'passed' if probe.get('source_unchanged') is True and coverage.get('physical_count',0)>0 and len(coverage.get('objects',[]))>=coverage['physical_count'] else 'pending'
