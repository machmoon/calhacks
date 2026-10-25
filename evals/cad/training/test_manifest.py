"""Corpus integrity gates, runnable without FreeCAD."""
import copy
import json
from pathlib import Path
import tempfile
import unittest
from evaluate import load_cases


class CorpusIntegrity(unittest.TestCase):
    def setUp(self):
        self.data=json.loads(Path(__file__).with_name('cases.json').read_text())
    def validate(self):
        with tempfile.TemporaryDirectory() as directory:
            path=Path(directory)/'cases.json';path.write_text(json.dumps(self.data))
            return load_cases(path)
    def test_reference(self):
        self.assertEqual(len(self.validate()),3)
    def test_holdout_leak(self):
        self.data['cases'][-1]['family']=self.data['cases'][0]['family']
        with self.assertRaises(ValueError):self.validate()
    def test_duplicate(self):
        self.data['cases'].append(copy.deepcopy(self.data['cases'][0]))
        with self.assertRaises(ValueError):self.validate()
    def test_bad_dimensions(self):
        for value in [True,0,-1,float('nan'),'3']:
            with self.subTest(value=value):
                self.data['cases'][0]['target_thickness']=value
                with self.assertRaises(ValueError):self.validate()
    def test_unknown_fields(self):
        self.data['cases'][0]['secret_answer']='must not silently accept'
        with self.assertRaises(ValueError):self.validate()

if __name__=='__main__':unittest.main()
