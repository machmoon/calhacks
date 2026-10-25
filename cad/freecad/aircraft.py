"""Rebuild the existing aircraft plan as editable native FreeCAD features.

This migration preserves the audited proxy geometry, including its defects. It
must not turn a backend change into a claim of completed aircraft engineering.
"""
import argparse,json,time,math,hashlib
from pathlib import Path
import FreeCAD as App
import native
import worker


def build(plan_path,out,reference_path):
    plan=json.loads(plan_path.read_text())
    reference=json.loads(reference_path.read_text())
    out.mkdir(parents=True,exist_ok=True)
    doc=worker.document('WrightAircraft')
    doc=worker.active()
    physical=[];comparisons=[];started=time.monotonic()
    for instance in plan['instances']:
        name=instance['name'];params=instance['params'];part=instance['part']
        location=instance['placement']['loc']
        if any(v!=0 for v in location['rot']):raise ValueError('nonzero source Euler rotations need an explicit tested conversion')
        position=native.vector(location['pos']);rotation=App.Rotation()
        if part=='common.block':spec=dict(name=name,kind='box',params=params)
        elif part=='common.mount_block':spec=dict(name=name,kind='mount',params=params)
        elif part=='common.loft':
            if params.get('inner'):spec=dict(name=name,kind='loft_shell',params=params)
            else:spec=dict(name=name,kind='loft',params=dict(rings=params['rings']))
        elif part=='common.axial_tube':
            start,end=native.vector(params['start']),native.vector(params['end']);axis=end-start
            p=dict(radius=params['radius'],height=axis.Length)
            kind='cylinder'
            if params['bore_radius']>0:p['bore_radius']=params['bore_radius'];kind='tube'
            spec=dict(name=name,kind=kind,params=p)
            position+=start;rotation=App.Rotation(App.Vector(0,0,1),axis)
        else:raise ValueError('unconverted source part '+part)
        native.make_part(doc,spec)
        obj=doc.getObject(name)
        obj.Placement=App.Placement(position,rotation)
        doc.recompute()
        evidence=native.describe(obj)
        expected=reference['parts'][name]
        error=abs(evidence['volume_mm3']-expected['volume_mm3'])
        bbox_error=max(abs(a-b) for row,target in zip(evidence['bbox_mm'],expected['bbox_world_mm']) for a,b in zip(row,target))
        solids=obj.Shape.Solids
        cg=sum((s.CenterOfMass*s.Volume for s in solids),App.Vector())/sum(s.Volume for s in solids)
        cg_error=max(abs(a-b) for a,b in zip(cg,expected['cg_world_mm']))
        passed=error<=max(1e-5,expected['volume_mm3']*1e-8) and bbox_error<=1e-5 and cg_error<=1e-5
        comparisons.append(dict(passed=passed,volume_error_mm3=error,bbox_error_mm=bbox_error,cg_error_mm=cg_error,**evidence))
        physical.append(name)
        print(f'{len(physical)}/{len(plan["instances"])} {name}: comparison {passed}',flush=True)
    contacts=sorted({tuple(sorted([i['name'],other])) for i in plan['instances'] for other in i.get('touches',[])})
    report=worker.check(physical,[list(p) for p in contacts])
    exports=worker.save(str(out),physical)
    result=dict(name=plan['name'],backend='FreeCAD',version=App.Version(),source_plan=str(plan_path),
                source_plan_sha256=hashlib.sha256(plan_path.read_bytes()).hexdigest(),
                native_objects=len(doc.Objects),physical_solids=len(physical),
                constrained_profiles=sum(o.TypeId=='Sketcher::SketchObject' and o.FullyConstrained for o in doc.Objects),
                comparisons=comparisons,geometry_equivalent=all(c['passed'] for c in comparisons),
                fit=report,exports=exports,elapsed_s=time.monotonic()-started,
                scope='Native migration of existing aircraft proxy geometry; wings remain filled envelopes, materials/mass and fabrication completeness unverified',
                aircraft_complete=False)
    (out/'result.json').write_text(json.dumps(result,indent=2,allow_nan=False))
    print(json.dumps({k:result[k] for k in ['physical_solids','native_objects','constrained_profiles','geometry_equivalent','elapsed_s']}),flush=True)
    print('Clashes:',sum(p['clash'] for p in report['pairs']),'Floating:',sum(p['floating'] for p in report['pairs']),flush=True)
    App.closeDocument(doc.Name)
    return result

if __name__=='__main__':
    parser=argparse.ArgumentParser(description=__doc__)
    parser.add_argument('plan',type=Path);parser.add_argument('reference',type=Path);parser.add_argument('out',type=Path)
    args=parser.parse_args();build(args.plan.resolve(),args.out.resolve(),args.reference.resolve())
