"""Run bounded independent probes against an explicitly frozen native artifact."""
import argparse,json,subprocess,time,os,threading
from concurrent.futures import ThreadPoolExecutor
from functools import wraps
def synchronized(method):
 @wraps(method)
 def wrapped(self,*args,**kwargs):
  with self.lock:return method(self,*args,**kwargs)
 return wrapped
from datetime import datetime,timezone
from pathlib import Path
from report import aggregate,sha,package_inventory,collision_gate_status
ROOT=Path(__file__).resolve().parents[3]
RUNNER_SHA256=sha(Path(__file__))
class ProbeTrace:
 """Incremental execution timings; engineering verdicts remain in probe results."""
 def __init__(self,out,timeout_seconds=300):
  self.timeout_seconds=timeout_seconds
  self.path=Path(out)/'probe-timings.jsonl';self.snapshot=Path(out)/'probe-timings.json';self.rows=[];self.lock=threading.RLock()
 def _emit(self,event,row):
  with self.path.open('a') as stream:
   stream.write(json.dumps(dict(event=event,**row),allow_nan=False)+'\n');stream.flush();os.fsync(stream.fileno())
  temporary=self.snapshot.with_suffix('.json.tmp')
  temporary.write_text(json.dumps({'schema_version':1,'probes':self.rows},indent=2,allow_nan=False));temporary.replace(self.snapshot)
 @synchronized
 def start(self,stage,command):
  wall=time.time();mono=time.monotonic()
  row={'stage':stage,'status':'running','started_at':datetime.fromtimestamp(wall,timezone.utc).isoformat(),'started_unix_seconds':wall,'started_monotonic_seconds':mono,'timeout_seconds':self.timeout_seconds,'command':command}
  self.rows.append(row);self._emit('started',row);return row
 @synchronized
 def finish(self,row,result,error=None):
  wall=time.time();mono=time.monotonic()
  status='timed_out' if isinstance(error,subprocess.TimeoutExpired) else ('failed' if error is not None or result.get('process_returncode',0) or result.get('status')=='failed' else 'complete')
  detail={'type':type(error).__name__,'message':str(error)} if error is not None else ({'type':'ProcessExit','message':f"Probe exited with code {result['process_returncode']}"} if result.get('process_returncode',0) else None)
  if detail is None and result.get('error'):detail={'type':'ProbeResultError','message':str(result['error'])}
  row.update(status=status,finished_at=datetime.fromtimestamp(wall,timezone.utc).isoformat(),finished_unix_seconds=wall,finished_monotonic_seconds=mono,duration_seconds=mono-row['started_monotonic_seconds'],wall_duration_seconds=wall-row['started_unix_seconds'],result_status=result.get('status'),engineering_passed=result.get('passed'),process_returncode=result.get('process_returncode',0 if error is None else None),error=detail)
  self._emit('finished',row)
 @synchronized
 def skip(self,stage,reason):
  row={'stage':stage,'status':'skipped','reason':reason,'at':datetime.now(timezone.utc).isoformat(),'duration_seconds':0}
  self.rows.append(row);self._emit('skipped',row)

def run(native,manifest,out,workers=None,split_edits=True):
 workers=max(1,min(8,int(workers or os.environ.get("WRIGHT_AUDIT_WORKERS","3"))))
 native=Path(native).resolve();manifest=Path(manifest).resolve();out=Path(out).resolve()
 out.mkdir(parents=True,exist_ok=False)
 source_sha=sha(native);m=json.loads(manifest.read_text())
 if m.get('native_sha256')!=source_sha:raise ValueError('Manifest must identify the exact current native artifact')
 contract={'native_sha256':source_sha,'runner_sha256':RUNNER_SHA256,'physical_objects':m['physical'],'reference_objects':m['references'],'contacts':m.get('contact_pairs',[])}
 if m.get('proxy_module'):
  module=Path(m['proxy_module']);module=module if module.is_absolute() else manifest.parent/module
  module_hash=sha(module);declared={digest for path,digest in m.get('generator_sources',{}).items() if Path(path).name==module.name}
  if declared!={module_hash}:raise ValueError('Manifest-declared feature module hash missing, ambiguous or mismatched')
  contract.update(proxy_module=str(module.resolve()),proxy_module_sha256=module_hash)
 contract['collision_objects']=m['physical']+[n for n in m['references'] if n.startswith(('LiftMotor','LiftProp'))]
 cp=out/'contract.json';cp.write_text(json.dumps(contract,indent=2));results={};evidence=[];trace=ProbeTrace(out,timeout_seconds=max(30,min(1200,int(os.environ.get('WRIGHT_PROBE_TIMEOUT','600')))))
 def probe(stage,path):
  command=[str(ROOT/'cad/freecad/run.sh'),str(Path(__file__).with_name('native_probe.py')),stage,str(path),str(cp),str(out)]
  timing=trace.start(stage,command);failure=None
  try:
   with (out/(stage+'.log')).open('w') as log:p=subprocess.run(command,stdout=log,stderr=subprocess.STDOUT,timeout=trace.timeout_seconds,env={**os.environ,"OMP_NUM_THREADS":"1","OPENBLAS_NUM_THREADS":"1","MKL_NUM_THREADS":"1"})
   result=json.loads((out/(stage+'.json')).read_text())
   if p.returncode:result.update(status='failed',passed=False,process_returncode=p.returncode)
  except Exception as error:
   failure=error;result={'status':'failed','passed':False,'error':str(error)}
  trace.finish(timing,result,failure)
  results[stage]=result;return result
 def gate(name,status,measurements,reason=None):
  evidence.append(dict(gate=name,revision=m['design_revision'],native_sha256=source_sha,status=status,measurements=measurements,reason=reason))
 def export_job():
  if m.get('step',{}).get('path'):
   export_path=out/'export-equivalence.json'
   command=[str(ROOT/'cad/freecad/run.sh'),str(Path(__file__).with_name('export_probe.py')),str(native),str(manifest),str(export_path)]
   timing=trace.start('exports',command);failure=None
   try:
    with (out/'export.log').open('w') as log:
     completed=subprocess.run(command,stdout=log,stderr=subprocess.STDOUT,timeout=trace.timeout_seconds,env={**os.environ,"OMP_NUM_THREADS":"1","OPENBLAS_NUM_THREADS":"1","MKL_NUM_THREADS":"1"})
    exported=json.loads(export_path.read_text())
    if completed.returncode:exported.update(passed=False,process_returncode=completed.returncode)
   except Exception as error:
    failure=error;exported={'passed':False,'error':str(error)}
   trace.finish(timing,exported,failure)
   results['exports']=exported
   return exported
  else:
   trace.skip('exports','Manifest has no STEP export path');return None
 pool=ThreadPoolExecutor(max_workers=workers)
 futures={stage:pool.submit(probe,stage,native) for stage in ['pairs','geometry','sweeps','access','aero']}
 if split_edits:
  edit_timing=trace.start('edits',[])
  edit_futures={case:pool.submit(probe,'edit_'+case,native) for case in ['span','chord','boom_spacing','battery','servo_reservation']}
 else:futures['edits']=pool.submit(probe,'edits',native)
 export_future=pool.submit(export_job)
 g=futures['geometry'].result()
 gate('native_geometry','passed' if g.get('passed') and g.get('source_unchanged') else 'failed',g)
 p=futures['pairs'].result();check=p.get('check',{})
 gate('all_pair_collisions',collision_gate_status(p),p,'All declared physical objects and included candidate hardware checked. Missing BOM geometry is tracked separately by bom_geometry_coverage; this is not whole-aircraft clearance certification.')
 gate('attachment_contacts','failed' if check.get('unmet_contact_count',0)>0 else 'pending',{'declared_contacts':contract['contacts'],'unmet_contact_count':check.get('unmet_contact_count')},'Empty or incomplete contact declarations do not establish retention.')
 if split_edits:
  edits={case:future.result() for case,future in edit_futures.items()}
  e=dict(edits['span']);e['cases']=[case for item in edits.values() for case in item.get('cases',[])]
  e['stage']='edits'
  e.update(status='complete' if all(item.get('status')=='complete' for item in edits.values()) else 'failed',passed=all(item.get('passed') for item in edits.values()),source_unchanged=all(item.get('source_unchanged') and item.get('native_sha256')==source_sha for item in edits.values()),isolated_cases=list(edits))
  if any(item.get('status')!='complete' for item in edits.values()):e['error']='One or more isolated edit processes failed'
  results['edits']=e;(out/'edits.json').write_text(json.dumps(e,indent=2));trace.finish(edit_timing,e)
 else:e=futures['edits'].result()
 required_edits={'span','chord','boom_spacing','battery','servo_reservation'}
 cases={case['case']:case for case in e.get('cases',[])}
 edit_failure=e.get('status')!='complete' or not e.get('source_unchanged') or any(not c.get('passed') and c.get('status')!='pending' for c in e.get('cases',[]))
 edit_status='failed' if edit_failure else 'passed' if all(cases.get(name,{}).get('passed') for name in required_edits) else 'pending'
 gate('span_chord_boom_servo_battery_edits',edit_status,e,'Five native parameter transformations checked. Servo/battery reservation changes are not hardware fit evidence; hardware release remains a separate gate.')
 if e.get('edited_native'):
  r=probe('reopen',e['edited_native']);linked=r.get('native_sha256')==e.get('edited_sha256') and e.get('native_sha256')==source_sha
  gate('native_reopen','passed' if linked and r.get('passed') and r.get('source_unchanged') else 'failed',r)
 else:trace.skip('reopen','The edits probe produced no edited_native artifact')
 sweeps=futures['sweeps'].result()
 gate('propeller_sweeps','inconclusive' if not sweeps.get('passed') else 'pending',sweeps,'Conservative full-turn lift envelopes only; cruise source geometry remains unresolved. Envelope clashes require exact sweep investigation.')
 access=futures['access'].result()
 removal=access.get('removal_sequence',[{}])[0]
 access_status='inconclusive' if access.get('status')!='complete' or (access.get('driver_paths_passed') and removal.get('status')=='inconclusive') else ('failed' if not access.get('passed') else 'pending')
 gate('service_access',access_status,access,f"Driver paths passed: {access.get('driver_paths_passed')}; declared hatch removal: {removal.get('status','unassessed')}. Alternate removal sequences, nut tools and complete service coverage remain separate requirements.")
 a=futures['aero'].result()
 gate('current_wing_analysis','passed' if a.get('passed') and a.get('source_unchanged') else 'inconclusive',a,'Current symmetric wing only at 18 m/s; no aircraft stability, stall, structural or flight claim.')
 exported=export_future.result()
 if exported is not None:gate('exports','pending' if exported.get('passed') else 'failed',exported,'Local physical-airframe STEP equivalence passed only if measurements.passed is true. Full export gate still needs candidate COTS and required reference coverage; these are declared omissions.')
 pool.shutdown(wait=True)
 package=Path(m.get('package',ROOT/'cad/projects/vtol-demonstrator/package-v1'))
 package=(package if package.is_absolute() else ROOT/package).resolve()
 if not package.is_dir():raise ValueError('Manifest hardware evidence package must be a local existing directory')
 gate('frozen_hardware_package','pending',package_inventory(package),'Evidence package is frozen; hardware compatibility and unresolved roles remain open.')
 from engineering_evidence import collect
 engineering=collect(m,source_sha,package,ROOT,out)
 gate('bom_geometry_coverage','pending',engineering['bom_coverage'],'Every purchased instance needs an explicit exact/proxy/missing geometry mapping; candidate geometry does not establish a released hardware package.')
 gate('mass_propulsion_performance',engineering['wright_air']['status'],engineering,'Wright-air integration: '+engineering['wright_air']['reason'])
 report=aggregate(m['design_revision'],source_sha,evidence)
 report.update(execution_policy={'max_workers':workers,'probe_timeout_seconds':trace.timeout_seconds,'isolated_edit_cases':split_edits,'native_library_threads':1},probe_timings=trace.rows,probe_timing_trace=str(trace.path),probe_timing_snapshot=str(trace.snapshot),runner_sha256=RUNNER_SHA256,manifest_sha256=sha(manifest),source_unchanged=sha(native)==source_sha,probes=results)
 if not report['source_unchanged']:report['passed']=False
 (out/'result.json').write_text(json.dumps(report,indent=2,allow_nan=False));print(out/'result.json')
if __name__=='__main__':
 p=argparse.ArgumentParser();p.add_argument('native');p.add_argument('manifest');p.add_argument('output');a=p.parse_args();run(a.native,a.manifest,a.output)
