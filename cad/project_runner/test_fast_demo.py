import json,unittest
from fast_demo import parameters
class FixedParameters(unittest.TestCase):
    def test_exact_values_no_defaults_or_model(self):
        expected=dict(Span=1900,RootChord=300,BoomHalfSpacing=320,MotorFrontX=120,MotorRearX=700,WingletHeight=85,WingletCantAngle=20,WingletSweepOffset=30,TailDihedralAngle=32)
        text='WRIGHT_FAST_DEMO\nWRIGHT_DESIGN_JSON: '+json.dumps(expected)
        self.assertEqual(parameters(text),expected)
        with self.assertRaises(ValueError):parameters(text+'\nWRIGHT_DESIGN_JSON: {}')
        expected['Span']=9000
        with self.assertRaises(ValueError):parameters('WRIGHT_DESIGN_JSON: '+json.dumps(expected))
    def test_missing_values_fail_without_inventing_dimensions(self):
        with self.assertRaises(ValueError):parameters('WRIGHT_FAST_DEMO')
        with self.assertRaises(ValueError):parameters('WRIGHT_DESIGN_JSON: {"Span":1900}')
