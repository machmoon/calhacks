"""Fresh native oracle for the bounded Astra exact-component study."""
import hashlib,json,sys
from pathlib import Path
import FreeCAD as App
import Part
import brep_compare,component_library
run=Path(sys.argv[1]).resolve();checkpoint=run/'cad/working.FCStd'
doc=App.openDocument(str(checkpoint));doc.recompute(None,True,True)
expected={'tmotor_mn2806_evo':'tmotor_mn2806_evo_650kv','tmotor_p12x4':'tmotor_p12x4_cf'}
rows=[];seen=[]
for obj in doc.Objects:
    if not hasattr(obj,'Shape') or obj.Shape.isNull():continue
    assert hasattr(obj,'WrightStepImport'),'non-imported physical object'
    metadata=json.loads(obj.WrightStepImport);identity=metadata['component_identity'];cid=identity['catalog_id'];seen.append(cid)
    assert cid in expected and identity['bom_catalog_ids']==[expected[cid]]
    assert identity['mounting_frame_verified'] is False
    entry=component_library.catalog(cid)['components'][0];source=component_library.ROOT/entry['file']
    digest=hashlib.sha256(source.read_bytes()).hexdigest();assert digest==metadata['source_sha256']==entry['sha256']
    original=Part.read(str(source)).Solids[metadata['source_solid_index']-1]
    assert obj.Shape.isValid() and original.isValid() and len(obj.Shape.Solids)==1
    a=obj.Shape.copy();b=original.copy();a.Placement=App.Placement();b.Placement=App.Placement()
    evidence=brep_compare.compare(a.cleaned().exportBrepToString(),b.cleaned().exportBrepToString())
    rows.append(dict(name=obj.Name,catalog_id=cid,source_sha256=digest,placement_matrix=obj.Placement.toMatrix().A,preservation=evidence,source_bounds=[original.BoundBox.XLength,original.BoundBox.YLength,original.BoundBox.ZLength],valid=True))
assert sorted(seen)==sorted(expected),'missing or duplicate exact components'
a,b=[doc.getObject(r['name']).Shape for r in rows];gap=a.distToShape(b)[0];overlap=a.common(b).Volume
report=dict(passed=all(r['preservation']['passed'] for r in rows) and gap>=20 and abs(overlap)<1e-7,objects=rows,separation_mm=gap,overlap_mm3=overlap,checkpoint=str(checkpoint),native_checkpoint_verified=True,checked_step_export_verified=False,mounting_frames_verified=False,scope='Exact source solids and explicit separate placements only; no reference BOM substitution, compatibility or flight assessment')
(run/'independent-native-oracle.json').write_text(json.dumps(report,indent=2));print(json.dumps(report,indent=2));assert report['passed']
