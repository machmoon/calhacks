import sys,unittest
from pathlib import Path
sys.path.insert(0,str(Path(__file__).resolve().parents[1]/'freecad'))
from live_edits import validate
class EditInput(unittest.TestCase):
 def test_partial_edits_accept_only_bounded_numeric_parameters(self):
  self.assertEqual(validate({'Span':1850,'WingletHeight':90}),{'Span':1850,'WingletHeight':90})
  for value in [{},{'Span':True},{'Span':float('nan')},{'Span':1500},{'Shape':'code'},{'__class__':'x'},['Span',1850]]:
   with self.assertRaises(ValueError):validate(value)
