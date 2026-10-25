import importlib.util,tempfile,unittest
from pathlib import Path
spec=importlib.util.spec_from_file_location('execute',Path(__file__).with_name('execute.py'));m=importlib.util.module_from_spec(spec);spec.loader.exec_module(m)
class Isolation(unittest.TestCase):
 def test_write_and_oracle_isolation(self):
  with tempfile.TemporaryDirectory(dir=m.ROOT/'out') as d:
   root=Path(d);inputs=root/'inputs';out=root/'attempt';inputs.mkdir();out.mkdir();(inputs/'public.txt').write_text('public');(root/'hidden.txt').write_text('hidden')
   (out/'candidate.py').write_text("import pathlib,sys\ni=pathlib.Path(sys.argv[1]);o=pathlib.Path(sys.argv[2]);assert (i/'public.txt').read_text()=='public'\nfor p in [i/'public.txt',i.parent/'hidden.txt']:\n try:\n  (p.write_text('bad') if p.name=='public.txt' else p.read_text());raise AssertionError('sandbox bypass')\n except PermissionError:pass\n(o/'proof.txt').write_text('passed')\n")
   result=m.execute(inputs,out,python='/usr/bin/python3',timeout=20);self.assertEqual(result['exit_code'],0,(out/'execution.log').read_text());self.assertTrue(result['source_unchanged']);self.assertEqual((out/'proof.txt').read_text(),'passed')
   with self.assertRaises(ValueError):m.execute(inputs,out,python='/usr/bin/python3')
if __name__=='__main__':unittest.main()
