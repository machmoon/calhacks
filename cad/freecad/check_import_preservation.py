"""Fresh native verification of untouched imported solids/reference surfaces."""
import hashlib,json,sys
from pathlib import Path
import FreeCAD as App
import Part
import native,step_import,brep_compare
path=Path(sys.argv[1]).resolve();output=Path(sys.argv[2]).resolve();doc=App.openDocument(str(path));native.validate_document(doc)
sources={};rows=[]
for obj in doc.Objects:
    if not hasattr(obj,'WrightStepImport'):continue
    metadata=json.loads(obj.WrightStepImport);source=metadata['source_path']
    if source not in sources:
        digest=hashlib.sha256(Path(source).read_bytes()).hexdigest();entities=step_import._entities(Part.read(source),True)
        ordered=[s for s,role in entities if role=='physical_solid']+[s for s,role in entities if role=='reference_surface']
        sources[source]=(digest,ordered)
    digest,ordered=sources[source];assert digest==metadata['source_sha256']
    index=metadata.get('source_entity_index',metadata.get('source_solid_index'));original=ordered[index-1]
    actual=obj.Shape.copy();expected=original.copy()
    placement=metadata['assembly_placement'];transform=App.Placement(App.Vector(*placement['position_mm']),App.Rotation(App.Vector(*placement['axis']),placement['angle_degrees']))
    target=transform.multiply(expected.Placement)
    placement_error=max(abs(a-b) for a,b in zip(actual.Placement.toMatrix().A,target.toMatrix().A))
    actual.Placement=App.Placement();expected.Placement=App.Placement()
    comparison=brep_compare.compare(actual.cleaned().exportBrepToString(),expected.cleaned().exportBrepToString())
    rows.append(dict(name=obj.Name,source=source,index=index,geometry_role=metadata.get('geometry_role','physical_solid'),placement_matrix_error=placement_error,brep=comparison))
counts={source:len([r for r in rows if r['source']==source]) for source in sources}
complete=all(counts[source]==len(values[1]) and len({r['index'] for r in rows if r['source']==source})==len(values[1]) for source,values in sources.items())
report=dict(passed=complete and all(r['brep']['passed'] and r['placement_matrix_error']<1e-9 for r in rows),all_source_entities_retained_once=complete,objects=rows,scope='Untouched import geometry/initial placement and full source-entity coverage; not mass, collision or STEP exchange certification')
output.write_text(json.dumps(report,indent=2));print('Fresh imported entity preservation:',report['passed'],len(rows));assert report['passed']
App.closeDocument(doc.Name)
