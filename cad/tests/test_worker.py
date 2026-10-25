import json
from pathlib import Path
import subprocess
import sys
import unittest


class WorkerTests(unittest.TestCase):
    def test_invalid_requests_do_not_kill_worker(self):
        messages = ['{broken', '[]',
                    json.dumps(dict(jsonrpc="2.0",id=1,method="missing")),
                    json.dumps(dict(jsonrpc="2.0",id=2,method="ping",params=[])),
                    json.dumps(dict(jsonrpc="2.0",id=3,method="ping",params={"bad":1})),
                    json.dumps(dict(jsonrpc="2.0",id=4,method="describe",params={"part":"../x"})),
                    json.dumps(dict(jsonrpc="2.0",id=5,method="ping"))]
        worker = Path(__file__).resolve().parents[1]/"wright_cad/worker.py"
        result = subprocess.run([sys.executable,str(worker)],input='\n'.join(messages)+'\n',
                                text=True,capture_output=True,timeout=60,check=True)
        responses = [json.loads(line) for line in result.stdout.splitlines()]
        self.assertEqual(responses[0]['method'],'ready')
        self.assertEqual([r['error']['code'] for r in responses[1:-1]],
                         [-32700,-32600,-32601,-32602,-32602,-32000])
        self.assertEqual(responses[-1]['id'],5)
        self.assertIn('pid',responses[-1]['result'])


if __name__ == '__main__':
    unittest.main()
