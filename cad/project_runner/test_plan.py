import unittest
from plan import parse_design
GOOD="DESIGN = {'Span':1800,'RootChord':300,'BoomHalfSpacing':320,'MotorFrontX':120,'MotorRearX':700}"
class DesignContract(unittest.TestCase):
    def test_literal_data_only(self):
        self.assertEqual(parse_design(GOOD)['Span'],1800)
        for code in [GOOD+'\nprint(1)',GOOD.replace('1800','True'),GOOD.replace('1800','9999'),"DESIGN = dict()",GOOD.replace('Span','span')]:
            with self.assertRaises((ValueError,TypeError)):parse_design(code)

    def test_appearance_contract(self):
        code=GOOD[:-1]+", 'WingletHeight':70,'WingletCantAngle':20,'WingletSweepOffset':30,'TailDihedralAngle':30}"
        self.assertEqual(parse_design(code,appearance=True)['TailDihedralAngle'],30)
        for candidate in [GOOD,code.replace(":30}",":90}"),code.replace("'WingletHeight':70","'WingletHeight':True")]:
            with self.assertRaises(ValueError):parse_design(candidate,appearance=True)
        with self.assertRaises(ValueError):parse_design(code)
