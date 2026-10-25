"""Real vendor and KiCad STEP import, units, rejection and saved-solid checks."""
import hashlib,json
from pathlib import Path
import FreeCAD as App
import Part
import native,worker,step_import,component_library,brep_compare
ROOT=Path(__file__).resolve().parents[2];OUT=ROOT/'out/native-step-import';OUT.mkdir(parents=True,exist_ok=True)
worker.document('NativeStepImport');doc=worker.active()
assert component_library.catalog('AOS2807')['components']==[]
assert component_library.catalog('Gemfan7042')['components']==[]
assert all(c['source_hash_verified'] for c in component_library.catalog()['components'])
# Unit normalization fixture: change a known STEP's length context to metres.
part=Part.makeBox(1,2,3);mm=OUT/'unit-mm.step';part.exportStep(str(mm));metres=OUT/'unit-metres.step'
metres.write_text(mm.read_text().replace('SI_UNIT(.MILLI.,.METRE.)','SI_UNIT($,.METRE.)'))
assert step_import.declared_units(metres.read_bytes())['source_length_unit']=='m'
u=worker.METHODS['import_step'](path=str(metres),name='MetreFixture')
b=doc.MetreFixture.Shape.BoundBox
assert max(abs(a-b) for a,b in zip([b.XLength,b.YLength,b.ZLength],[1000,2000,3000]))<1e-5
assert u['provenance']['source_length_to_mm']==1000
before={o.Name for o in doc.Objects}
def reject(path,name='Rejected',**kwargs):
    try:worker.METHODS['import_step'](path=str(path),name=name,**kwargs)
    except (ValueError,TypeError):pass
    else:raise AssertionError('invalid import accepted')
    assert {o.Name for o in doc.Objects}==before
reject(mm,name='MetreFixture');reject(mm,axis=[0,0,0]);reject(mm,source_units='in')
unknown=OUT/'unknown.step';unknown.write_text(mm.read_text().replace('SI_UNIT(.MILLI.,.METRE.)',"CONVERSION_BASED_UNIT('INCH',#999)"));reject(unknown)
face=OUT/'surface.step';Part.makePlane(2,3).exportStep(str(face));reject(face)
invalid=OUT/'invalid.step';invalid.write_text('invalid');reject(invalid)
bad=doc.addObject('Part::Feature','Invalid');before={o.Name for o in doc.Objects};reject(mm);doc.removeObject(bad.Name);doc.recompute()
doc.removeObject('MetreFixture');doc.recompute()
fixtures=[('Motor',ROOT/'cad/components/vendor/tmotor/mn2806evo.step',[0,0,0]),('Propeller',ROOT/'cad/components/vendor/tmotor/p12x4.step',[100,0,0]),('PcbFixture',ROOT/'out/step-import-fixture/pcb-assembly-board.step',[0,60,0])]
results=[];names=[];originals={}
for name,path,position in fixtures:
    source_hash=hashlib.sha256(path.read_bytes()).hexdigest();originals[str(path)]=source_hash
    if name in ('Motor','Propeller'):
        cid={'Motor':'tmotor_mn2806_evo','Propeller':'tmotor_p12x4'}[name]
        result=worker.METHODS['import_component'](component_id=cid,name=name,position=position)
        assert result['provenance']['component_identity']['catalog_id']==cid
        assert result['provenance']['component_identity']['bom_catalog_ids']==[{'Motor':'tmotor_mn2806_evo_650kv','Propeller':'tmotor_p12x4_cf'}[name]]
        assert result['provenance']['component_identity']['mounting_frame_verified'] is False
    else:result=worker.METHODS['import_step'](path=str(path),name=name,position=position)
    assert result['provenance']['source_sha256']==source_hash
    assert result['provenance']['native_length_unit']=='mm'
    expected=Part.read(str(path));selected=[doc.getObject(n) for n in result['object_names']]
    actual=Part.makeCompound([o.Shape for o in selected]);unplaced=actual.copy();unplaced.translate(App.Vector(*[-v for v in position]))
    assert len(actual.Solids)==len(expected.Solids)
    assert abs(actual.Volume-expected.Volume)<=max(1e-5,expected.Volume*1e-8)
    # Full BREP structure/coefficient comparison plus inverse rigid placement verifies preservation.
    hashes=[]
    inverse=App.Placement(App.Vector(*position),App.Rotation()).inverse()
    for obj,solid in zip(selected,expected.Solids):
        actual_solid=obj.Shape.copy();actual_solid.Placement=inverse.multiply(actual_solid.Placement)
        matrix_error=max(abs(a-b) for a,b in zip(actual_solid.Placement.toMatrix().A,solid.Placement.toMatrix().A))
        original_solid=solid.copy();actual_solid.Placement=App.Placement();original_solid.Placement=App.Placement()
        ah=hashlib.sha256(actual_solid.cleaned().exportBrepToString().encode()).hexdigest();eh=hashlib.sha256(original_solid.cleaned().exportBrepToString().encode()).hexdigest()
        comparison=brep_compare.compare(actual_solid.cleaned().exportBrepToString(),original_solid.cleaned().exportBrepToString())
        hashes.append(dict(comparison,placement_matrix_error=matrix_error))
        assert comparison['passed'] and matrix_error<1e-9,(name,hashes)
    result['preservation_checks']=hashes
    results.append(result);names.extend(result['object_names'])
assert results[2]['solid_count']==6
labels=[doc.getObject(n).Label for n in results[2]['object_names']]
assert 'assembly_board_PCB' in labels and 'SOT-223' in labels
# Native placement remains editable, without pretending vendor features exist.
first=names[0];old=doc.getObject(first).Shape.BoundBox.XMin
worker.place(first,[20,0,0]);assert abs(doc.getObject(first).Shape.BoundBox.XMin-old-20)<1e-6
assert native.editable_parameters(doc.getObject(first))==set()
path=OUT/'imported-components.FCStd';doc.saveAs(str(path));before_shapes={n:dict(volume=doc.getObject(n).Shape.Volume,brep=doc.getObject(n).Shape.cleaned().exportBrepToString()) for n in names};App.closeDocument(doc.Name);worker._doc=None
worker.open_document(str(path));doc=worker.active()
for name in names:
    obj=doc.getObject(name);before=before_shapes[name]
    assert obj.Shape.isValid() and len(obj.Shape.Solids)==1
    assert abs(obj.Shape.Volume-before['volume'])<max(1e-5,before['volume']*1e-8),(name,obj.Shape.Volume,before['volume'])
    assert brep_compare.compare(obj.Shape.cleaned().exportBrepToString(),before['brep'])['passed'],(name,brep_compare.compare(obj.Shape.cleaned().exportBrepToString(),before['brep']))
    assert json.loads(obj.WrightStepImport)['source_sha256'] in originals.values()
assert 'step_import' in worker.inspect_tree([names[0]])[0]
assert all(hashlib.sha256(Path(p).read_bytes()).hexdigest()==digest for p,digest in originals.items())
(OUT/'result.json').write_text(json.dumps(dict(passed=True,imports=results,pcb_fixture_scope='PCB assembly_board test fixture, not confirmed requested VTOL PCB',millimetre_native_units=True,metre_source_normalization_verified=True,unresolved_conversion_unit_rejected=True,non_solids_and_invalid_inputs_rejected=True,transaction_rollback_verified=True,editable_placement=True,source_parametric_history=False,source_files_unchanged=True,reopened_solid_count=len(names),pcb_labels=labels),indent=2))
print('Real motor, propeller and6-solid PCB imported; source geometry, SI unit conversion, placement, rollback and native reopen pass')
App.closeDocument(doc.Name)
