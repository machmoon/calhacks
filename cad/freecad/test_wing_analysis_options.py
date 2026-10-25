import os,tempfile,unittest
from pathlib import Path
from unittest.mock import patch
import wing_analysis
class WingOptions(unittest.TestCase):
    def test_bounds_and_strict_types(self):
        invalid=[dict(object_name='../Wing'),dict(velocity_m_s=True),dict(velocity_m_s=0),dict(velocity_m_s=101),dict(velocity_m_s=float('nan')),dict(alphas_deg=[]),dict(alphas_deg=[0]*6),dict(alphas_deg=[0,0.]),dict(alphas_deg=[True]),dict(alphas_deg=['4']),dict(alphas_deg=[-11]),dict(alphas_deg=[13]),dict(altitude_m=-1),dict(altitude_m=20001),dict(altitude_m=float('inf')),dict(samples_per_branch=32),dict(samples_per_branch=130),dict(samples_per_branch=65.),dict(samples_per_branch=True),dict(symmetric=1)]
        for values in invalid:
            with self.subTest(values=values),self.assertRaises(ValueError):wing_analysis.options(**dict(dict(object_name='Wing'),**values))
        self.assertEqual(wing_analysis.options('Wing')['alphas_deg'],[0.,4.,8.])
        self.assertEqual(wing_analysis.options('Wing',100,[-10,0,12],20000,129,False)['samples_per_branch'],129)
    def test_explicit_python_priority_and_preserved_venv_path(self):
        with tempfile.TemporaryDirectory() as d:
            base=Path(d)/'base';base.write_text('');base.chmod(0o700)
            venv=Path(d)/'venv-python';venv.symlink_to(base)
            with patch.dict(os.environ,{'WRIGHT_ANALYSIS_PYTHON':str(venv),'WRIGHT_PYTHON':'/missing'}):self.assertEqual(wing_analysis.python(),venv)
            with patch.dict(os.environ,{'WRIGHT_ANALYSIS_PYTHON':'/missing'}),self.assertRaises(ValueError):wing_analysis.python()
if __name__=='__main__':unittest.main()
