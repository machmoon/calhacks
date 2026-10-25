import json,tempfile,unittest
from pathlib import Path
from timing import TimingTrace
class TimingTests(unittest.TestCase):
 def test_success_failure_and_no_payload(self):
  with tempfile.TemporaryDirectory() as folder:
   t=TimingTrace(folder)
   with t.span('success') as details:details['return_code']=0
   with self.assertRaises(ValueError):
    with t.span('failure'):raise ValueError('sk-secret base64-private-payload')
   t.finish('failed');raw=t.path.read_text();self.assertNotIn('sk-secret',raw);self.assertNotIn('base64-private',raw)
   rows=[json.loads(line) for line in raw.splitlines()];self.assertTrue(all('utc' in r and 'monotonic_s' in r and 'elapsed_s' in r for r in rows));self.assertEqual([r['elapsed_s'] for r in rows],sorted(r['elapsed_s'] for r in rows))
   s=json.loads(t.summary.read_text());self.assertEqual(s['actions'][1]['error_class'],'ValueError');self.assertGreaterEqual(s['actions'][0]['duration_s'],0)
 def test_response_only_usage_and_tool_names(self):
  with tempfile.TemporaryDirectory() as folder:
   p=Path(folder)/'response.json';p.write_text(json.dumps({'usage':{'input_tokens':12,'output_tokens':3,'secret':'sk-private'},'content':[{'type':'tool_use','name':'submit_candidate','input':{'python':'private base64 payload'}}]}));t=TimingTrace(folder);t.response(p);t.finish();raw=t.path.read_text();self.assertNotIn('private',raw);self.assertIn('submit_candidate',raw);self.assertIn('12',raw)
if __name__=='__main__':unittest.main()
