import tempfile,unittest
from pathlib import Path
from check_submission import check
class Boundary(unittest.TestCase):
 def run_code(self,code):
  with tempfile.TemporaryDirectory() as d:
   p=Path(d)/'candidate.py';p.write_text(code);return check(p)
 def test_build_module(self):self.assertTrue(self.run_code('import FreeCAD as App\nCONST=3\ndef build(input_path,output_dir,checkpoint=None):\n doc=App.openDocument(input_path)\n if checkpoint: checkpoint("opened")\n'))
 def test_reject_execution(self):
  valid='def build(input_path,output_dir,checkpoint=None): pass\n'
  for code in [valid+'build("x","y")', 'doc=App.newDocument()\n'+valid, '@execute()\n'+valid, 'import hidden_evaluator\n'+valid, 'obj.foo=2\n'+valid, 'def helper(x=run()):pass\n'+valid, 'def helper()->run():pass\n'+valid]:
   with self.subTest(code=code),self.assertRaises((ValueError,SyntaxError)):self.run_code(code)
 def test_signature(self):
  with self.assertRaises(ValueError):self.run_code('def build(input_path,output_dir):pass')
if __name__=='__main__':unittest.main()
