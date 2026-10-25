import json,tempfile,unittest
from pathlib import Path
from summarize_timing import summarize
class SummaryTests(unittest.TestCase):
 def test_nested_intervals_not_double_counted_and_active_elapsed(self):
  with tempfile.TemporaryDirectory() as folder:
   p=Path(folder);rows=[{'type':'action_started','action_id':1,'action':'planning','monotonic_s':100,'elapsed_s':0},{'type':'action_started','action_id':2,'action':'model_subprocess','monotonic_s':101,'elapsed_s':1},{'type':'action_finished','action_id':2,'monotonic_s':109,'status':'succeeded'},{'type':'action_finished','action_id':1,'monotonic_s':110,'status':'succeeded'},{'type':'action_started','action_id':3,'action':'freecad_build','monotonic_s':112,'elapsed_s':12}];(p/'timing-trace.jsonl').write_text(''.join(json.dumps(x)+'\n' for x in rows)+'{"partial"')
   r=summarize(p,130);self.assertEqual(r['wall_elapsed_s'],30);self.assertEqual(r['observed_top_level_union_s'],28);self.assertEqual(len(r['top_level_actions']),2);self.assertEqual(r['unfinished_actions'][0]['duration_s'],18);self.assertEqual(r['ignored_incomplete_or_invalid_jsonl_records'],1)
 def test_probe_errors_and_render_failures_do_not_copy_payload(self):
  with tempfile.TemporaryDirectory() as folder:
   p=Path(folder);(p/'audit').mkdir();(p/'cad').mkdir();(p/'audit/probe-timings.jsonl').write_text(json.dumps({'event':'finished','stage':'geometry','status':'failed','duration_seconds':2,'error':{'type':'ValueError','message':'private-base64-payload'},'command':['secret-token']})+'\n');(p/'cad/artifact-manifest.json').write_text(json.dumps({'render_failures':{'rear':'private-base64-payload'}}));r=summarize(p,130);serialized=json.dumps(r);self.assertNotIn('private-base64',serialized);self.assertNotIn('secret-token',serialized);self.assertEqual(r['audit_probes'][0]['error_class'],'ValueError');self.assertEqual(r['render_failures'][0]['view'],'rear')
if __name__=='__main__':unittest.main()
