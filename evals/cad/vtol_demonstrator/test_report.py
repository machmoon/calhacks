import unittest
from report import aggregate,GATES
class Report(unittest.TestCase):
 def item(self,name='native_geometry',**kw):return dict(gate=name,revision='r1',native_sha256='abc',status='passed',measurements={'count':2},**kw)
 def test_local_pass_not_product_pass(self):
  r=aggregate('r1','abc',[self.item()]);self.assertFalse(r['passed']);self.assertEqual(r['gates']['native_geometry']['status'],'passed')
 def test_stale_geometry_is_pending(self):
  r=aggregate('r1','changed',[self.item()]);self.assertEqual(r['gates']['native_geometry']['status'],'pending')
 def test_stale_revision_is_pending(self):self.assertFalse(aggregate('r2','abc',[self.item()])['passed'])
 def test_empty_success_rejected(self):
  i=self.item();i['measurements']={}
  with self.assertRaises(ValueError):aggregate('r1','abc',[i])
 def test_duplicate_cannot_overwrite_failure(self):
  with self.assertRaises(ValueError):aggregate('r1','abc',[self.item(),self.item()])
 def test_all_gates_required(self):self.assertTrue(aggregate('r1','abc',[self.item(k) for k in GATES])['passed'])

class CollisionProofTests(unittest.TestCase):
 def test_unverified_geometry_cannot_clear(self):
  from report import collision_gate_status
  for verified in [False,None]:
   self.assertEqual(collision_gate_status({'status':'complete','check':{'overlap_count':0,'geometry_verified':verified}}),'failed')
 def test_clear_subset_still_requires_full_coverage(self):
  from report import collision_gate_status
  self.assertEqual(collision_gate_status({'status':'complete','check':{'overlap_count':0,'geometry_verified':True}}),'pending')

if __name__=='__main__':unittest.main()
