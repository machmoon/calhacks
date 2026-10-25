"""Prepare/score a single independently attempted edit; not a generalization score."""
import argparse,hashlib,json,math,sys
from pathlib import Path
import FreeCAD as App
ROOT=Path(__file__).resolve().parents[2]
sys.path.insert(0,str(Path(__file__).parent/'training'))
from evaluate import build,evaluate_geometry
OUT=ROOT/'out/cad-agent-trial'
case=dict(id='UnseenStack',split='test',family='six_bolt',shell_radius=43,plate_width=49,bolt_count=6,levels=3,target_thickness=4.25)
mode=argparse.ArgumentParser();mode.add_argument('mode',choices=['prepare','score']);args=mode.parse_args()
OUT.mkdir(parents=True,exist_ok=True)
if args.mode=='prepare':
    doc,physical,joints=build(case)
    for o in doc.Objects:
        if 'Visibility' in o.PropertiesList:o.Visibility=o.Name in physical
    doc.saveAs(str(OUT/'input.FCStd'))
    (OUT/'oracle.json').write_text(json.dumps(dict(case=case,physical=physical,joints=joints,input_sha256=hashlib.sha256((OUT/'input.FCStd').read_bytes()).hexdigest())))
    print('Prepared input.FCStd')
else:
    oracle=json.loads((OUT/'oracle.json').read_text())
    doc=App.openDocument(str(OUT/'attempt/assembly.FCStd'));doc.recompute()
    result=evaluate_geometry(doc,oracle['physical'],oracle['joints'])
    expected=(case['plate_width']**2-case['bolt_count']*math.pi*1.7**2)*case['target_thickness']
    result['target_thickness_passed']=all(abs(doc.getObject(f'Tray{i}_Blank').Height.Value-case['target_thickness'])<1e-7 for i in range(3))
    result['tray_material_passed']=all(abs(doc.getObject(f'Tray{i}').Shape.Volume-expected)<1e-5 for i in range(3))
    original=App.openDocument(str(OUT/'input.FCStd'))
    unchanged=[n for n in oracle['physical'] if not n.startswith(('Tray','Bolt'))]
    result['unchanged_parts_passed']=all(not original.getObject(n).Shape.cut(doc.getObject(n).Shape).Solids and not doc.getObject(n).Shape.cut(original.getObject(n).Shape).Solids for n in unchanged)
    result['input_preserved']=hashlib.sha256((OUT/'input.FCStd').read_bytes()).hexdigest()==oracle['input_sha256']
    result['passed']=result['passed'] and all(result[k] for k in ['target_thickness_passed','tray_material_passed','unchanged_parts_passed','input_preserved'])
    result['scope']='One independently attempted synthetic edit; not a broad agent success rate'
    (OUT/'score.json').write_text(json.dumps(result,indent=2));print(json.dumps({k:v for k,v in result.items() if k!='checks'}))
