import json,unittest
from types import SimpleNamespace as Obj
import import_geometry as roles
class ReferenceCoverage(unittest.TestCase):
    def fixture(self):
        doc=Obj(Name='Test',Objects=[])
        def add(name,instance,role):
            obj=Obj(Name=name,Document=doc,WrightStepImport=json.dumps(dict(import_instance=instance,geometry_role=role,source_sha256='a'*64)))
            doc.Objects.append(obj);return obj
        return add('Solid','one','physical_solid'),add('Surface','one','reference_surface'),add('Other','two','reference_surface')
    def test_same_instance_reference_cannot_disappear_from_solid_check_scope(self):
        solid,ref,other=self.fixture();result=roles.coverage([solid])
        self.assertFalse(result['complete_source_geometry_checked']);self.assertEqual(result['omitted_reference_objects'],['Surface'])
        self.assertFalse(result['reference_surfaces_checked'])
    def test_unmarked_and_wrong_type_not_promoted_to_reference(self):
        with self.assertRaises(ValueError):roles.validate_reference(Obj(Name='Unmarked'))
        solid,ref,_=self.fixture();ref.TypeId='Part::Box'
        with self.assertRaises(ValueError):roles.validate_reference(ref)
    def test_derived_geometry_carries_source_reference_coverage(self):
        solid,ref,_=self.fixture()
        derived=Obj(Name='Derived',Document=solid.Document,OutList=[solid])
        solid.Document.Objects.append(derived)
        result=roles.coverage([derived]);self.assertEqual(result['omitted_reference_objects'],['Surface'])
        self.assertFalse(result['complete_source_geometry_checked'])
    def test_missing_source_references_do_not_restore_full_coverage(self):
        solid,ref,_=self.fixture();data=json.loads(solid.WrightStepImport)
        data.update(source_solid_count=1,source_reference_surface_count=1);solid.WrightStepImport=json.dumps(data)
        solid.Document.Objects.remove(ref)
        result=roles.coverage([solid]);self.assertEqual(result['missing_source_entity_count'],1)
        self.assertTrue(result['source_has_reference_geometry']);self.assertFalse(result['complete_source_geometry_checked'])
    def test_solid_only_instance_has_no_reference_omission(self):
        solid,ref,_=self.fixture();solid.Document.Objects.remove(ref)
        self.assertTrue(roles.coverage([solid])['complete_source_geometry_checked'])
if __name__=='__main__':unittest.main()
