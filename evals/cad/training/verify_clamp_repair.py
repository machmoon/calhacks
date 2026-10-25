"""Fresh native oracle for a model-produced v1 clamp repair; never repairs its files.
Run with cad/freecad/run.sh after the model's bounded attempt has ended.
"""
import argparse
import hashlib
import itertools
import json
import math
from pathlib import Path
import sys
ROOT=Path(__file__).resolve().parents[3]
sys.path.insert(0,str(ROOT/'cad/freecad'))
import FreeCAD as App
import Part
import native
import worker


def equal_material(a,b):
    removed=a.cut(b).Volume;added=b.cut(a).Volume
    tol=max(1e-5,max(a.Volume,b.Volume)*1e-8)
    bounds=max(abs(getattr(a.BoundBox,k)-getattr(b.BoundBox,k)) for k in ['XMin','YMin','ZMin','XMax','YMax','ZMax'])
    return dict(passed=removed<=tol and added<=tol and bounds<=1e-5,removed_mm3=removed,added_mm3=added,bounds_difference_mm=bounds,tolerance_mm3=tol)


def cylinder_faces(shape):
    result=[]
    for face in shape.Faces:
        surface=face.Surface
        if isinstance(surface,Part.Cylinder):
            result.append((surface.Radius,surface.Center,surface.Axis,face.Area))
    return result


def assess(directory):
    attempt=json.loads((directory/'attempt.json').read_text())
    source=Path(attempt['source']);snapshot=json.loads((directory.parent/'source-snapshot.json').read_text())
    result=dict(scope='Nominal rigid geometry and editable dependencies only; not functional clamp, preload, strength or flight approval',checks={},source=str(source),artifact=str(directory/'cad/assembly.FCStd'))
    checks=result['checks']
    checks['source_unchanged']=dict(passed=bool(attempt['source_bytes_unchanged']) and hashlib.sha256(source.read_bytes()).hexdigest()==snapshot['sha256'],sha256=hashlib.sha256(source.read_bytes()).hexdigest())
    selection=attempt.get('saved_selection')
    checks['model_saved_four_outputs']=dict(passed=isinstance(selection,list) and len(selection)==4 and len(set(selection))==4 and 'BoomRight' in selection and 'TopStabilizer' in selection,selection=selection)
    saved=directory/'cad/assembly.FCStd'
    if not checks['model_saved_four_outputs']['passed'] or not saved.is_file():
        result['passed']=False;result['failure']='No successful model save with the required four physical outputs';return result
    baseline=worker.open_checked(str(source))
    original={n:baseline.getObject(n).Shape.copy() for n in ['BoomRight','FinClampDrill1','LowerClampDrill1','TopStabilizer']}
    App.closeDocument(baseline.Name)
    worker.open_document(str(saved));doc=worker.active()
    try:
        native.validate_document(doc)
        outputs=worker.selected_objects(selection)
        other=[o for o in outputs if o.Name not in ('BoomRight','TopStabilizer')]
        upper=max(other,key=lambda o:o.Shape.BoundBox.ZLength);lower=min(other,key=lambda o:o.Shape.BoundBox.ZLength)
        if upper==lower:raise ValueError('cannot distinguish the fin and lower clamp outputs')
        result['identified_outputs']=dict(fin_and_upper=upper.Name,lower=lower.Name)
        boom=doc.getObject('BoomRight');top=doc.getObject('TopStabilizer')
        checks['boom_unchanged']=equal_material(original['BoomRight'],boom.Shape)
        checks['stabilizer_unchanged']=equal_material(original['TopStabilizer'],top.Shape)
        roi=Part.makeBox(160,70,30,App.Vector(620,185,-22))
        checks['fin_outside_root_unchanged']=equal_material(original['FinClampDrill1'].cut(roi),upper.Shape.cut(roi))
        checks['fin_outside_root_unchanged']['allowed_root_roi_mm']=[[620,185,-22],[780,255,8]]
        boom_cylinder=max(cylinder_faces(boom.Shape),key=lambda c:c[0]);axis=boom_cylinder[2];axis.normalize();origin=boom_cylinder[1]
        for obj in [upper,lower]:
            gap=obj.Shape.distToShape(boom.Shape)[0];overlap=obj.Shape.common(boom.Shape).Volume
            seats=[dict(radius_mm=radius,area_mm2=area,axis_offset_mm=(center-origin).cross(axis).Length,axis_parallel_error=direction.cross(axis).Length) for radius,center,direction,area in cylinder_faces(obj.Shape) if abs(radius-5)<1e-6 and (center-origin).cross(axis).Length<1e-5 and direction.cross(axis).Length<1e-7 and area>1e-3]
            checks['nominal_boom_seat:'+obj.Name]=dict(passed=bool(seats) and gap<=1e-5 and overlap<=1e-6,gap_mm=gap,overlap_mm3=overlap,coaxial_r5_faces=seats)
        split=upper.Shape.distToShape(lower.Shape)[0]
        checks['remaining_split_gap']=dict(passed=abs(split-.5)<=1e-5,measured_mm=split,target_mm=.5)
        check=worker.check(selection,contacts=[['BoomRight',upper.Name],['BoomRight',lower.Name],[upper.Name,'TopStabilizer']],tolerance_mm=1e-5,require_connected=True)
        checks['physical_fit']=dict(passed=check['passed'],report=check)
        # Recompute probes operate in reopened memory only; neither artifact nor source is saved.
        artifact_hash=hashlib.sha256(saved.read_bytes()).hexdigest()
        for obj in [upper,lower]:
            bindings=native.parameter_bindings(obj);probe=None
            for binding in bindings:
                if binding['parameter']!='Radius' or abs(binding['value']-5)>1e-6:continue
                before=obj.Shape.Volume;parameter_object=doc.getObject(binding['object_name']);value=float(parameter_object.Radius)
                try:
                    native.set_parameter(doc,parameter_object.Name,'Radius',value+.001)
                    delta=obj.Shape.Volume-before
                finally:
                    native.set_parameter(doc,parameter_object.Name,'Radius',value)
                restored=abs(obj.Shape.Volume-before)<=max(1e-5,abs(before)*1e-8)
                if abs(delta)>1e-7:
                    probe=dict(object_name=parameter_object.Name,parameter='Radius',probe_delta_mm=.001,volume_delta_mm3=delta,restored=restored);break
            checks['live_dependency:'+obj.Name]=dict(passed=bool(bindings) and probe is not None and probe['restored'],type=obj.TypeId,parameter_binding_count=len(bindings),probe=probe)
        checks['oracle_did_not_save_artifact']=dict(passed=hashlib.sha256(saved.read_bytes()).hexdigest()==artifact_hash,sha256=artifact_hash)
        checks['source_still_unchanged']=dict(passed=hashlib.sha256(source.read_bytes()).hexdigest()==snapshot['sha256'])
        result['passed']=all(c['passed'] for c in checks.values())
        return result
    finally:
        App.closeDocument(doc.Name)


def main():
    p=argparse.ArgumentParser();p.add_argument('attempt_directory',type=Path);args=p.parse_args()
    try:result=assess(args.attempt_directory.resolve())
    except Exception as error:result=dict(passed=False,error=type(error).__name__+': '+str(error),scope='Oracle failed closed; no model success established')
    output=args.attempt_directory/'independent-repair-oracle.json';output.write_text(json.dumps(result,indent=2))
    print(json.dumps(result,indent=2))
    if not result['passed']:raise SystemExit(1)

if __name__=='__main__':main()
