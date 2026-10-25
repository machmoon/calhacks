"""Fail-closed negative controls against retained independent measurements."""
import copy,json,unittest
from pathlib import Path
from run import evaluate,error,ROOT,HERE
class EvaluatorNegatives(unittest.TestCase):
 @classmethod
 def setUpClass(cls):
  folder=ROOT/'out/so-arm101-evaluation/baseline-v2';cls.first=json.loads((folder/'initial.json').read_text());cls.second=json.loads((folder/'reopened.json').read_text());cls.cases=json.loads((HERE/'cases.json').read_text());cls.urdf=ROOT/'cad/projects/so-arm101/upstream/Simulation/SO101/so101_new_calib.urdf'
 def score(self,a=None,b=None):return evaluate(a or self.first,b or self.second,self.urdf,self.cases)
 def test_unmodified_baseline(self):self.assertTrue(self.score()['passed'])
 def test_omitted_instance(self):
  a=copy.deepcopy(self.first);a['visual_names'].pop();self.assertFalse(self.score(a)['gates']['coverage'])
 def test_extra_real_geometry(self):
  a=copy.deepcopy(self.first);a['unexpected_geometry'].append({'name':'UnexpectedBox','type':'Part::Box'});self.assertFalse(self.score(a)['gates']['coverage'])
 def test_frozen_controls(self):
  a=copy.deepcopy(self.first);zero=a['states'][1]
  for state in a['states'][2:]:state['links']=copy.deepcopy(zero['links']);state['visuals']=copy.deepcopy(zero['visuals'])
  self.assertFalse(self.score(a)['gates']['independent_sampled_articulation'])
 def test_empty_states(self):
  a=copy.deepcopy(self.first);a['states']=[]
  with self.assertRaises(ValueError):self.score(a)
 def test_duplicate_state(self):
  a=copy.deepcopy(self.first);a['states'][-1]=copy.deepcopy(a['states'][0])
  with self.assertRaises(ValueError):self.score(a)
 def test_nonfinite_matrix(self):
  m=copy.deepcopy(self.first['states'][0]['links']['base_link']);m[0][0]=float('nan')
  with self.assertRaises(ValueError):error(m,m)
 def test_scaled_rotation(self):
  m=copy.deepcopy(self.first['states'][0]['links']['base_link']);m[0][0]=2
  with self.assertRaises(ValueError):error(m,m)
 def test_wrong_axis_pose(self):
  a=copy.deepcopy(self.first);a['states'][2]['links']['shoulder_link'][0][3]+=10;self.assertFalse(self.score(a)['passed'])
 def test_missing_reopen_limits(self):
  b=copy.deepcopy(self.second);b['controls']['gripper'].pop('gripper_lower');self.assertFalse(self.score(b=b)['gates']['native_controls_and_limits'])
if __name__=='__main__':unittest.main()
