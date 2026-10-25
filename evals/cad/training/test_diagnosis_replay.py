import copy
import unittest
from replay_diagnosis_checks import equivalent


class CompactEvidence(unittest.TestCase):
    def setUp(self):
        def pair(a,b,clash=False,required=False,distance=2):
            return dict(a=a,b=b,clash=clash,required_contact=required,floating=required and distance>.1,broad_phase=False,overlap_mm3=1 if clash else 0,distance_mm=distance,distance_lower_bound_mm=distance)
        self.full=dict(objects=['A','B','C'],components=[['A','B'],['C']],overlap_count=1,unmet_contact_count=1,connected=False,passed=False,pairs_total=3,require_connected=True,tolerance_mm=.1,pairs=[pair('A','B',clash=True,distance=0),pair('A','C',required=True),pair('B','C')])
        self.compact=copy.deepcopy(self.full);self.compact.update(pairs=self.compact['pairs'][:2],pair_details_scope='all_clashes_and_declared_contacts',omitted_pair_count=1,contact_pair_count=0)

    def test_valid_full_and_compact(self):
        self.assertTrue(equivalent(self.full,self.full))
        self.assertTrue(equivalent(self.compact,self.full,True))
        self.assertFalse(equivalent(self.compact,self.full))

    def test_omitted_clash_or_required_contact(self):
        for index in (0,1):
            bad=copy.deepcopy(self.compact);bad['pairs'].pop(index);bad['omitted_pair_count']+=1
            self.assertFalse(equivalent(bad,self.full,True))

    def test_false_counts_and_connectivity(self):
        for key in ('omitted_pair_count','contact_pair_count','overlap_count','unmet_contact_count','pairs_total'):
            bad=copy.deepcopy(self.compact);bad[key]+=1
            self.assertFalse(equivalent(bad,self.full,True),key)
        bad=copy.deepcopy(self.compact);bad['components']=[['A','B','C']]
        self.assertFalse(equivalent(bad,self.full,True))

    def test_duplicates_changed_measurements_and_full_omission(self):
        bad=copy.deepcopy(self.compact);bad['pairs'].append(bad['pairs'][0]);self.assertFalse(equivalent(bad,self.full,True))
        bad=copy.deepcopy(self.compact);bad['pairs'][0]['overlap_mm3']=0;self.assertFalse(equivalent(bad,self.full,True))
        bad=copy.deepcopy(self.full);bad['pairs'].pop();self.assertFalse(equivalent(bad,self.full))


if __name__=='__main__':unittest.main()
