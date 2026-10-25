import unittest
from brep_compare import compare
class BrepEvidence(unittest.TestCase):
    def test_exact_and_roundoff(self):
        original='CASCADE Topology V3\nVertices 2\n-29.9964871186102 8.04082567239624 -4.46630750205196\n'
        self.assertTrue(compare(original,original)['exact_bytes'])
        rounded=original.replace('-29.9964871186102','-29.9964871186101')
        result=compare(rounded,original);self.assertTrue(result['passed']);self.assertEqual(result['changed_numeric_tokens'],1)
    def test_geometry_and_topology_changes_fail(self):
        original='Vertices 2\n1.2345 2.3456 3.4567\n'
        for changed in [original.replace('1.2345','1.2355'),original.replace('Vertices 2','Vertices 3'),original+'Face 1\n',original.replace('Vertices','Edges')]:
            self.assertFalse(compare(changed,original)['passed'])
    def test_only_checked_cache_flag_is_ignored(self):
        original='TShapes 1\nFa\n\n0101000\n+1 0 *\n'
        self.assertTrue(compare(original,original.replace('0101000','0111000'))['passed'])
        for flags in ['0101100','0100000','1101000']:
            self.assertFalse(compare(original,original.replace('0101000',flags))['passed'])
    def test_actual_saved_propeller_brep(self):
        from pathlib import Path
        directory=Path(__file__).resolve().parents[2]/'out/step-import-fixture'
        a=directory/'prop-actual.brep';b=directory/'prop-expected.brep'
        if not a.exists() or not b.exists():self.skipTest('saved diagnostic absent')
        result=compare(a.read_text(),b.read_text());self.assertTrue(result['passed']);self.assertLess(result['max_numeric_delta'],1e-10)
if __name__=='__main__':unittest.main()
