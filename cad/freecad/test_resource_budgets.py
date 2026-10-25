import unittest
from resource_budgets import integer
class Budgets(unittest.TestCase):
    def test_boundaries_and_strict_types(self):
        for label,high in [('max_source_mib',200),('max_entities',4096),('max_objects',4096),('max_source_faces',100000)]:
            self.assertEqual(integer(1,label,1,high),1);self.assertEqual(integer(high,label,1,high),high)
            for invalid in [True,False,0,-1,high+1,1.,'1',None]:
                with self.subTest(label=label,value=invalid),self.assertRaises(ValueError):integer(invalid,label,1,high)
if __name__=='__main__':unittest.main()
