import copy,unittest
from curate import validate
class Integrity(unittest.TestCase):
 def record(self):return {'id':'a','split':'train','lineage_group':'family','origin':'reference_program','model':None,'source':{'kind':'authored_analytic_cad','sha256':'a'},'verification':{'passed':True,'checks':[{'passed':True}]},'trace':[{'action':'make_part','arguments':{},'result':{}}]}
 def test_valid(self):self.assertEqual(len(validate([self.record()])),1)
 def test_negative_controls(self):
  for change in [lambda r:r.update(model='invented'),lambda r:r['verification'].update(passed=False),lambda r:r['verification']['checks'][0].update(passed=False),lambda r:r['source'].update(kind='upstream_so101'),lambda r:r.update(trace=[])]:
   r=self.record();change(r)
   with self.assertRaises(ValueError):validate([r])
 def test_family_leak(self):
  a=self.record();b=copy.deepcopy(a);b.update(id='b',split='test');b['source']['sha256']='b'
  with self.assertRaises(ValueError):validate([a,b])
 def test_asset_leak(self):
  a=self.record();b=copy.deepcopy(a);b.update(id='b',split='test',lineage_group='other')
  with self.assertRaises(ValueError):validate([a,b])
if __name__=='__main__':unittest.main()
