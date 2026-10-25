"""Offline timing regressions: no FreeCAD or subprocess is launched."""
import importlib.util
import json
import subprocess
import sys
import tempfile
import unittest
from pathlib import Path
from unittest.mock import patch

HERE=Path(__file__).resolve().parent
sys.path.insert(0,str(HERE))
spec=importlib.util.spec_from_file_location('vtol_audit_run',HERE/'run.py')
module=importlib.util.module_from_spec(spec);spec.loader.exec_module(module)

class TimingTests(unittest.TestCase):
 def test_running_snapshot_is_visible_and_monotonic_duration_survives_clock_change(self):
  with tempfile.TemporaryDirectory() as directory:
   trace=module.ProbeTrace(directory)
   with patch.object(module.time,'time',side_effect=[100,98]),patch.object(module.time,'monotonic',side_effect=[50,53.5]):
    row=trace.start('geometry',['not-executed'])
    self.assertEqual(json.loads(trace.snapshot.read_text())['probes'][0]['status'],'running')
    trace.finish(row,{'status':'complete','passed':False})
   events=[json.loads(line) for line in trace.path.read_text().splitlines()]
   self.assertEqual([e['event'] for e in events],['started','finished'])
   self.assertEqual(events[-1]['duration_seconds'],3.5)
   self.assertEqual(events[-1]['wall_duration_seconds'],-2)
   self.assertEqual(events[-1]['status'],'complete')
   self.assertFalse(events[-1]['engineering_passed'])

 def test_timeout_failure_and_skip_are_explicit(self):
  with tempfile.TemporaryDirectory() as directory:
   trace=module.ProbeTrace(directory)
   row=trace.start('pairs',['not-executed'])
   trace.finish(row,{'status':'failed','passed':False},subprocess.TimeoutExpired('not-executed',300))
   row=trace.start('exports',['not-executed'])
   trace.finish(row,{'passed':False},ValueError('invalid JSON'))
   row=trace.start('access',['not-executed'])
   trace.finish(row,{'passed':False,'process_returncode':2})
   trace.skip('reopen','no edited native')
   rows=json.loads(trace.snapshot.read_text())['probes']
   self.assertEqual([r['status'] for r in rows],['timed_out','failed','failed','skipped'])
   self.assertEqual(rows[0]['error']['type'],'TimeoutExpired')
   self.assertEqual(rows[1]['error']['message'],'invalid JSON')
   self.assertEqual(rows[2]['error']['type'],'ProcessExit')

 def test_every_probe_has_incremental_records_without_changing_verdicts(self):
  with tempfile.TemporaryDirectory() as directory:
   root=Path(directory);native=root/'assembly.FCStd';native.write_bytes(b'frozen fixture')
   manifest=root/'manifest.json';manifest.write_text(json.dumps({'native_sha256':module.sha(native),'physical':[],'references':[],'design_revision':'test','package':str(root),'step':{'path':'fixture.step'}}))
   out=root/'audit'
   def fake_run(command,**kwargs):
    stage='exports' if command[1].endswith('export_probe.py') else command[2]
    snapshot=json.loads((out/'probe-timings.json').read_text())
    self.assertTrue(any(r['stage']==stage and r['status']=='running' for r in snapshot['probes']))
    result={'status':'complete','passed':True,'source_unchanged':True,'native_sha256':module.sha(native)}
    if stage=='edits':result.update(edited_native=str(native),edited_sha256=module.sha(native),cases=[])
    path=out/('export-equivalence.json' if stage=='exports' else stage+'.json');path.write_text(json.dumps(result))
    return subprocess.CompletedProcess(command,0)
   with patch.object(module.subprocess,'run',side_effect=fake_run),patch.object(module,'package_inventory',return_value={}),patch.object(module,'aggregate',return_value={'passed':False}):
    module.run(native,manifest,out,split_edits=False)
   rows=json.loads((out/'probe-timings.json').read_text())['probes']
   self.assertEqual(set(r['stage'] for r in rows),{'geometry','pairs','edits','reopen','sweeps','access','aero','exports'})
   self.assertTrue(all(r['status']=='complete' and r['duration_seconds']>=0 for r in rows))
   report=json.loads((out/'result.json').read_text());self.assertFalse(report['passed']);self.assertTrue(report['source_unchanged'])


 def test_parallel_trace_writes_are_atomic_and_preserve_all_workers(self):
  from concurrent.futures import ThreadPoolExecutor
  import threading
  with tempfile.TemporaryDirectory() as directory:
   trace=module.ProbeTrace(directory);barrier=threading.Barrier(4)
   def job(i):
    row=trace.start(str(i),[]);barrier.wait(timeout=2)
    trace.finish(row,{'status':'complete','passed':True})
   with ThreadPoolExecutor(max_workers=4) as pool:list(pool.map(job,range(4)))
   events=[json.loads(line) for line in trace.path.read_text().splitlines()]
   self.assertEqual(len(events),8)
   self.assertEqual([r['event'] for r in events[:4]],['started']*4)
   self.assertTrue(all(r['status']=='complete' for r in json.loads(trace.snapshot.read_text())['probes']))

 def test_native_error_is_execution_failure_even_when_process_exits_zero(self):
  with tempfile.TemporaryDirectory() as directory:
   trace=module.ProbeTrace(directory);row=trace.start('geometry',[])
   trace.finish(row,{'status':'failed','passed':False,'error':'native failed'})
   self.assertEqual(row['status'],'failed')

class SplitEditTests(unittest.TestCase):
 def test_isolated_edits_reopen_after_saved_span_and_keep_fit_gap(self):
  with tempfile.TemporaryDirectory() as folder:
   root=Path(folder);native=root/'assembly.FCStd';native.write_bytes(b'unchanged')
   manifest=root/'manifest.json';manifest.write_text(json.dumps({'native_sha256':module.sha(native),'physical':[],'references':[],'design_revision':'test','package':str(root)}))
   out=root/'audit';saved=[]
   def fake(command,**kwargs):
    stage=command[2];result={'stage':stage,'status':'complete','passed':True,'native_sha256':module.sha(native),'source_unchanged':True}
    if stage.startswith('edit_'):
     result['cases']=[{'case':stage[5:],'passed':True}]
     if stage=='edit_span':result.update(edited_native=str(native),edited_sha256=module.sha(native));saved.append(True)
     if stage=='edit_servo_reservation':result['cases'].append({'case':'servo_hardware_fit','passed':False,'status':'pending'});result['passed']=False
    if stage=='reopen':self.assertTrue(saved)
    (out/(stage+'.json')).write_text(json.dumps(result));return subprocess.CompletedProcess(command,0)
   with patch.object(module.subprocess,'run',side_effect=fake):module.run(native,manifest,out,workers=3)
   report=json.loads((out/'result.json').read_text())
   self.assertEqual(report['gates']['span_chord_boom_servo_battery_edits']['status'],'passed')
   self.assertEqual(report['gates']['native_reopen']['status'],'passed')
   self.assertEqual(report['gates']['frozen_hardware_package']['status'],'pending')
   self.assertFalse(report['passed'])

if __name__=='__main__':unittest.main()
