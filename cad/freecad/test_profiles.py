import FreeCAD as App
import json,math,os,sys,unittest
from pathlib import Path
sys.path.insert(0,str(Path(__file__).resolve().parent))
from native import make_part,set_profile,set_parameter
from exchange import export_step

class Profiles(unittest.TestCase):
    def setUp(self):self.doc=App.newDocument('ProfileTests');self.doc.UndoMode=1
    def tearDown(self):App.closeDocument(self.doc.Name)
    def rings(self,a=0,b=10):return [[[a,a,z],[b,a,z],[b,b,z],[a,b,z]] for z in [0,20]]
    def test_loft_edit_persists_constrained_native_sections(self):
        make_part(self.doc,dict(name='Loft',kind='loft',params=dict(rings=self.rings())))
        self.assertAlmostEqual(self.doc.Loft.Shape.Volume,2000,places=5)
        set_profile(self.doc,'Loft_Section1',[[0,0,20],[20,0,20],[20,20,20],[0,20,20]])
        self.assertAlmostEqual(self.doc.Loft.Shape.Volume,20*(100+200+400)/3,places=5)
        out=Path(os.environ['WRIGHT_CAD_OUT']);out.mkdir(parents=True,exist_ok=True)
        path=out/'native_loft.FCStd';self.doc.saveAs(str(path));App.closeDocument(self.doc.Name);self.doc=App.openDocument(str(path))
        self.assertEqual(self.doc.Loft.TypeId,'Part::Loft')
        self.assertTrue(all(s.TypeId=='Sketcher::SketchObject' and s.FullyConstrained for s in self.doc.Loft.Sections))
        export_step([self.doc.Loft],out/'native_loft.step')
    def test_shell_volume_and_invalid_profile_rollback(self):
        make_part(self.doc,dict(name='Shell',kind='loft_shell',params=dict(rings=self.rings(),inner=self.rings(1,9))))
        self.assertAlmostEqual(self.doc.Shell.Shape.Volume,(100-64)*20,places=5)
        before=self.doc.Shell.Shape.Volume
        for points in [[[0,0,20],[10,10,20],[0,10,20],[10,0,20]],[[0,0,20],[10,0,20],[10,10,21],[0,10,20]]]:
            with self.assertRaises(ValueError):set_profile(self.doc,'Shell_Outer_Section1',points)
            self.assertAlmostEqual(self.doc.Shell.Shape.Volume,before,places=5)
    def test_extruded_skin_has_a_real_cavity_and_linked_length(self):
        make_part(self.doc,dict(name='Skin',kind='extruded_skin',params=dict(profile=self.rings()[0],direction=[0,0,20],wall=1)))
        self.assertAlmostEqual(self.doc.Skin.Shape.Volume,720,places=5)
        set_parameter(self.doc,'Skin_Outer','LengthFwd',30)
        self.assertAlmostEqual(self.doc.Skin.Shape.Volume,1080,places=5)
        self.assertEqual(self.doc.Skin_Inner.LengthFwd.Value,30)
        with self.assertRaises(ValueError):set_parameter(self.doc,'Skin_InnerProfile','Value',-50)
        self.assertAlmostEqual(self.doc.Skin.Shape.Volume,1080,places=5)

    def test_rib_outline_tracks_skin_cavity(self):
        make_part(self.doc,dict(name='Skin',kind='extruded_skin',params=dict(profile=self.rings()[0],direction=[0,0,20],wall=1)))
        make_part(self.doc,dict(name='Rib',kind='profile_plate',params=dict(profile_object='Skin_InnerProfile',direction=[0,0,1],thickness=2,bores=[[[5,5,-1],[5,5,3],1]])))
        self.assertAlmostEqual(self.doc.Rib.Shape.Volume,(64-math.pi)*2,places=5)
        set_parameter(self.doc,'Skin_InnerProfile','Value',-2)
        self.assertAlmostEqual(self.doc.Rib.Shape.Volume,(36-math.pi)*2,places=5)
        self.assertAlmostEqual(self.doc.Skin.Shape.common(self.doc.Rib.Shape).Volume,0,places=5)
        self.assertAlmostEqual(self.doc.Skin.Shape.distToShape(self.doc.Rib.Shape)[0],0,places=5)

    def test_oblique_profiles_preserve_world_coordinates(self):
        rings=[[[3+x,5+y,7+y+z] for x,y in [(0,0),(10,0),(10,10),(0,10)]] for z in [0,20]]
        result=make_part(self.doc,dict(name='Oblique',kind='loft',params=dict(rings=rings)))
        self.assertAlmostEqual(result['volume_mm3'],2000,places=5)
        for a,b in zip(result['bbox_mm'][0],[3,5,7]):self.assertAlmostEqual(a,b,places=5)
        for a,b in zip(result['bbox_mm'][1],[13,15,37]):self.assertAlmostEqual(a,b,places=5)

suite=unittest.defaultTestLoader.loadTestsFromTestCase(Profiles)
r=unittest.TextTestRunner(verbosity=2).run(suite)
Path(os.environ['WRIGHT_CAD_OUT']).mkdir(parents=True,exist_ok=True)
Path(os.environ['WRIGHT_CAD_OUT'],'profile-tests.json').write_text(json.dumps(dict(passed=r.wasSuccessful(),tests=r.testsRun,failures=len(r.failures),errors=len(r.errors))))
if not r.wasSuccessful():raise RuntimeError('profile tests failed')
