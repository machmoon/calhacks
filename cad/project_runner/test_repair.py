import unittest
from repair import parse_repairs

class Repairs(unittest.TestCase):
    def test_only_observed_pairs_and_manufactured_targets(self):
        physical=['Mount'];pairs=[{'a':'Mount','b':'Motor'}]
        good="REPAIRS = [{'target':'Mount','tool':'Motor'}]"
        self.assertEqual(len(parse_repairs(good,physical,pairs)),1)
        for text in [good+'\nprint(1)',good.replace("'Mount','tool':'Motor'","'Motor','tool':'Mount'"),good.replace('Motor','Wing'),
                     "REPAIRS=[]", "REPAIRS=[{'target':'Mount','tool':'Motor','delete':True}]",good.replace('}]','},{\'target\':\'Mount\',\'tool\':\'Motor\'}]')]:
            with self.assertRaises((ValueError,TypeError)):parse_repairs(text,physical,pairs)
