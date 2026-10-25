import json,tempfile,unittest
from pathlib import Path
from unittest.mock import patch
from engineering_evidence import collect
from report import sha,collision_gate_status
class EvidenceTests(unittest.TestCase):
 def test_no_default_aircraft_is_substituted(self):
  with tempfile.TemporaryDirectory() as folder:
   root=Path(folder);(root/'bom.json').write_text(json.dumps({'items':[{'id':'motor','quantity':4,'mass_g':45}]}))
   with patch('engineering_evidence.subprocess.run') as launch:
    result=collect({'physical':[],'references':[]},'native',root,root,root)
   launch.assert_not_called();self.assertEqual(result['wright_air']['status'],'pending')
   self.assertEqual(result['bom_coverage']['known_purchased_mass_subtotal_g'],180)
   self.assertFalse(result['bom_coverage']['rows'][0]['covered'])
 def test_stale_binding_never_launches_analytical_checks(self):
  with tempfile.TemporaryDirectory() as folder:
   root=Path(folder);(root/'bom.json').write_text('{"items":[]}')
   binding=root/'binding.json';binding.write_text(json.dumps(dict(native_sha256='stale',bom_sha256=sha(root/'bom.json'),design_path='x',design_sha256='x',spec_path='x',spec_sha256='x',native_mass_reconciliation={})))
   with patch('engineering_evidence.subprocess.run') as launch:
    result=collect({'wright_air_binding':str(binding)},'native',root,root,root)
   launch.assert_not_called();self.assertEqual(result['wright_air']['status'],'inconclusive')
 def test_complete_physical_pair_scope_can_pass_without_claiming_bom_completeness(self):
  probe={'status':'complete','source_unchanged':True,'check':{'geometry_verified':True,'overlap_count':0},'coverage':{'physical_count':2,'objects':['a','b']}}
  self.assertEqual(collision_gate_status(probe),'passed')
  probe['check']['overlap_count']=1
  self.assertEqual(collision_gate_status(probe),'failed')
if __name__=='__main__':unittest.main()
