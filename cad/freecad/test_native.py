"""Run with the installed FreeCAD Python runtime; independent analytic checks."""
import json
import math
import os
from pathlib import Path
import sys
import unittest
sys.path.insert(0,str(Path(__file__).resolve().parent))
import FreeCAD as App
import Part
from native import make_part, set_parameter, describe


class NativeTests(unittest.TestCase):
    def setUp(self):
        self.doc=App.newDocument('NativeTest')
        self.doc.UndoMode=1
    def tearDown(self):
        App.closeDocument(self.doc.Name)
    def test_edit_and_native_roundtrip(self):
        spec=dict(name='Mount',kind='mount',params=dict(x=40,y=20,z=20,bores=[[[-1,10,10],[41,10,10],6.35]]))
        result=make_part(self.doc,spec)
        self.assertAlmostEqual(result['volume_mm3'],16000-math.pi*6.35**2*40,places=5)
        set_parameter(self.doc,'Mount_Blank','Width',24)
        expected=19200-math.pi*6.35**2*40
        self.assertAlmostEqual(self.doc.Mount.Shape.Volume,expected,places=5)
        out=Path(os.environ['WRIGHT_CAD_OUT'])
        out.mkdir(parents=True,exist_ok=True)
        path=out/'typed_mount.FCStd'
        self.doc.saveAs(str(path))
        Part.export([self.doc.Mount],str(out/'typed_mount.step'))
        App.closeDocument(self.doc.Name)
        reopened=App.openDocument(str(path))
        self.doc=reopened
        self.assertEqual(reopened.Mount.TypeId,'Part::Cut')
        self.assertEqual(reopened.Mount.Base.TypeId,'Part::Box')
        self.assertEqual(reopened.Mount.Tool.TypeId,'Part::Cylinder')
        self.assertAlmostEqual(reopened.Mount.Shape.Volume,expected,places=5)
        self.assertAlmostEqual(Part.read(str(out/'typed_mount.step')).Volume,expected,places=5)
    def test_tube_geometry(self):
        result=make_part(self.doc,dict(name='Tube',kind='tube',params=dict(radius=5,bore_radius=4,height=30)))
        self.assertAlmostEqual(result['volume_mm3'],math.pi*9*30,places=5)
        self.assertEqual(result['type'],'Part::Cut')
        set_parameter(self.doc,'Tube_Outer','Height',40)
        self.assertAlmostEqual(self.doc.Tube.Shape.Volume,math.pi*9*40,places=5)
        self.assertEqual(self.doc.Tube_Bore.Height.Value,40)
        with self.assertRaises(ValueError):
            set_parameter(self.doc,'Tube_Bore','Radius',6)
        self.assertAlmostEqual(self.doc.Tube_Bore.Radius.Value,4)
    def test_atomic_rejection(self):
        for bad in [True,float('nan'),0,-1]:
            with self.assertRaises(ValueError):
                make_part(self.doc,dict(name='Bad',kind='box',params=dict(x=bad,y=2,z=3)))
            self.assertEqual(len(self.doc.Objects),0)
        with self.assertRaises(ValueError):
            make_part(self.doc,dict(name='BadSeat',kind='mount',params=dict(x=10,y=10,z=10,bores=[[[50,0,0],[50,0,20],1]])))
        self.assertEqual(len(self.doc.Objects),0)
    def test_invalid_edit_restores_dependents(self):
        make_part(self.doc,dict(name='Mount',kind='mount',params=dict(x=10,y=10,z=10,bores=[[[5,5,-1],[5,5,11],1]])))
        before=describe(self.doc.Mount)
        with self.assertRaises(ValueError):
            set_parameter(self.doc,'Mount_Seat0','Radius',20)
        self.assertAlmostEqual(self.doc.Mount_Seat0.Radius.Value,1)
        self.assertAlmostEqual(describe(self.doc.Mount)['volume_mm3'],before['volume_mm3'])


suite=unittest.defaultTestLoader.loadTestsFromTestCase(NativeTests)
result=unittest.TextTestRunner(verbosity=2).run(suite)
Path(os.environ['WRIGHT_CAD_OUT']).mkdir(parents=True,exist_ok=True)
Path(os.environ['WRIGHT_CAD_OUT'],'typed_tests.json').write_text(json.dumps(dict(passed=result.wasSuccessful(),tests=result.testsRun,failures=len(result.failures),errors=len(result.errors))))
if not result.wasSuccessful(): raise RuntimeError('Native FreeCAD tests failed')
