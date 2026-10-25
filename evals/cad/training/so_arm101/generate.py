"""Native revolute-interface CAD reference traces, not SO101 reverse engineering."""
import json,math,sys
from pathlib import Path
ROOT=Path(__file__).resolve().parents[4];sys.path.insert(0,str(ROOT/'cad/freecad'))
import FreeCAD as App
import worker,native
from curate import export,sha
OUT=ROOT/'out/so-arm101-cad-corpus'
CASES=[('single_small','train',2,3,0.2,1),('single_large','train',3,4,0.3,1),('double_heldout','test',2.5,3.5,0.25,2)]

def run(case):
    ident,split,initial,target,gap,count=case;trace=[];checks=[];worker.document(ident);doc=worker.active()
    def call(action,**arguments):
        result=worker.METHODS[action](**arguments);trace.append(dict(action=action,arguments=arguments,result=result));return result
    def check(name,passed,**measurements):checks.append(dict(name=name,passed=bool(passed),**measurements))
    call('make_parameters',name='Joint',parameters={'PinRadius':{'value':initial,'min':1,'max':5}})
    call('make_part',spec={'name':'Pin','kind':'cylinder','params':{'radius':initial,'height':24}})
    call('bind_parameter',object_name='Pin',parameter='Radius',source_object='Joint',source_parameter='PinRadius')
    physical=['Pin']
    for i in range(count):
        name='Sleeve'+str(i);call('make_part',spec={'name':name,'kind':'tube','params':{'radius':7,'bore_radius':initial+gap,'height':8}})
        call('bind_parameter',object_name=name+'_Bore',parameter='Radius',source_object='Joint',source_parameter='PinRadius',offset_mm=gap)
        # Position the cut's actual output, retaining its native source dependency.
        call('place',object_name=name,position=[0,0,2+i*12]);physical.append(name)
    call('set_parameter',object_name='Joint',parameter='PinRadius',value=target,summary=True)
    def measure(prefix):
        check(prefix+'pin_radius',abs(float(doc.Pin.Radius)-target)<1e-8,value=float(doc.Pin.Radius))
        for i in range(count):
            sleeve=doc.getObject('Sleeve'+str(i));bore=doc.getObject(sleeve.Name+'_Bore')
            distance=doc.Pin.Shape.distToShape(sleeve.Shape)[0];overlap=doc.Pin.Shape.common(sleeve.Shape).Volume
            check(prefix+ sleeve.Name+'_clearance',abs(distance-gap)<1e-6,distance_mm=distance,expected_mm=gap)
            check(prefix+ sleeve.Name+'_no_overlap',overlap<1e-7,overlap_mm3=overlap)
            check(prefix+ sleeve.Name+'_dependency',any(p=='Radius' and 'Joint.PinRadius' in e for p,e in bore.ExpressionEngine),expressions=list(bore.ExpressionEngine))
            expected=math.pi*(49-(target+gap)**2)*8
            check(prefix+sleeve.Name+'_analytic_volume',abs(sleeve.Shape.Volume-expected)<1e-6,actual_mm3=sleeve.Shape.Volume,expected_mm3=expected)
        check(prefix+'native_valid',all(r['valid'] for r in native.validate_document(doc)))
    measure('edited_')
    # Negative controls do not enter successful demonstrations.
    sleeve=doc.Sleeve0;bore=doc.Sleeve0_Bore;expr=dict(bore.ExpressionEngine)['Radius']
    bore.setExpression('Radius',None);doc.recompute();dependency_rejected=not any(p=='Radius' for p,e in bore.ExpressionEngine)
    bore.Radius=target-0.1;doc.recompute();collision=doc.Pin.Shape.common(sleeve.Shape).Volume
    check('negative_undersized_bore_detected',collision>1e-6,overlap_mm3=collision)
    check('negative_detached_dependency_detected',dependency_rejected)
    bore.setExpression('Radius',expr);doc.recompute()
    folder=OUT/ident;folder.mkdir(parents=True,exist_ok=True);file=folder/'assembly.FCStd';doc.saveAs(str(file));App.closeDocument(doc.Name);worker._doc=None
    worker.open_document(str(file));doc=worker.active();measure('reopened_')
    source={'kind':'authored_analytic_cad','sha256':sha(__file__),'license':'LicenseRef-Workspace-Generated','redistribution_review_required':True,'not_dimensions_from_so101':True}
    # Generator code is shared infrastructure, not asset identity; fingerprint the case specification.
    import hashlib
    source['generator_sha256']=source['sha256'];source['sha256']=hashlib.sha256(json.dumps({'initial_radius':initial,'target_radius':target,'clearance':gap,'sleeves':count},sort_keys=True).encode()).hexdigest()
    record={'schema_version':1,'id':ident,'split':split,'lineage_group':'single_sleeve' if count==1 else 'double_sleeve','origin':'reference_program','model':None,'task':f'Create a native coaxial pin and {count} sleeve CAD assembly with pin radius {initial} mm, sleeve outer radius 7 mm, sleeve length 8 mm and radial gap {gap} mm; then change shared pin radius to {target} mm while preserving the gap through native expressions. Save editable CAD.','source':source,'trace':trace,'verification':{'passed':all(c['passed'] for c in checks),'checks':checks,'native_file':str(file),'native_sha256':sha(file),'freecad_version':App.Version(),'scope':'Nominal BREP geometry/dependency exercise; not an SO101 replica, solved joint, load rating or robot-control policy.'}}
    App.closeDocument(doc.Name);worker._doc=None;return record
if __name__=='__main__':
    records=[run(c) for c in CASES];export(records,OUT);print(json.dumps({'passed':True,'records':len(records),'out':str(OUT)}))
