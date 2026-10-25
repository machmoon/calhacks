"""Deterministic native CAD edit examples and independent geometric oracles."""
import argparse
import hashlib
import itertools
import json
import math
import re
from pathlib import Path
import sys

ROOT = Path(__file__).resolve().parents[3]
sys.path.insert(0, str(ROOT / 'cad/freecad'))


def load_cases(path):
    data = json.loads(path.read_text())
    if set(data) != {'schema_version','units','cases'} or data.get('schema_version') != 1 or data.get('units') != 'mm':
        raise ValueError('unsupported schema or units')
    required = {'id', 'split', 'family', 'shell_radius', 'plate_width', 'bolt_count', 'levels', 'target_thickness'}
    if not isinstance(data['cases'],list) or not data['cases']:
        raise ValueError('cases must be a nonempty array')
    seen = set()
    for case in data['cases']:
        if set(case) != required or case['id'] in seen:
            raise ValueError('invalid fields or duplicate case ID')
        if not isinstance(case['id'],str) or not re.fullmatch(r'[A-Za-z][A-Za-z0-9_]*',case['id']) or not isinstance(case['family'],str) or not case['family']:
            raise ValueError('invalid ID or family')
        seen.add(case['id'])
        if case['split'] not in {'train', 'validation', 'test'}:
            raise ValueError('invalid split')
        for key in ('shell_radius', 'plate_width', 'bolt_count', 'levels', 'target_thickness'):
            value = case[key]
            if isinstance(value, bool) or not isinstance(value, (int, float)) or not math.isfinite(value) or value <= 0:
                raise ValueError('invalid numeric field: ' + key)
        if type(case['bolt_count']) is not int or case['bolt_count'] not in (4, 6) or type(case['levels']) is not int or case['levels'] < 2:
            raise ValueError('invalid pattern')
    train_families = {c['family'] for c in data['cases'] if c['split'] == 'train'}
    if train_families & {c['family'] for c in data['cases'] if c['split'] == 'test'}:
        raise ValueError('held-out topology family leaks into training')
    return data['cases']


def build(case):
    import FreeCAD as App
    import native
    doc = App.newDocument(case['id'])
    doc.UndoMode = 1
    physical = []
    joints = []
    def part(name, kind, params, xyz=(0, 0, 0)):
        native.make_part(doc, dict(name=name, kind=kind, params=params))
        obj = doc.getObject(name)
        obj.Placement.Base = App.Vector(*xyz)
        physical.append(name)
        return obj
    r, w = case['shell_radius'], case['plate_width']
    part('Shell', 'tube', dict(radius=r, bore_radius=r-2, height=case['levels']*15+15), (w/2, w/2, 0))
    # Shell center matches tray center; bolt pattern differs topologically in test.
    for level in range(case['levels']):
        z = 10 + 15*level
        points = [(w/2 + (w/2-5)*math.cos(i*2*math.pi/case['bolt_count']),
                   w/2 + (w/2-5)*math.sin(i*2*math.pi/case['bolt_count'])) for i in range(case['bolt_count'])]
        tray = f'Tray{level}'
        part(tray, 'mount', dict(x=w, y=w, z=2, bores=[[[x,y,-1],[x,y,8],1.7] for x,y in points]), (0,0,z))
        for i, (x,y) in enumerate(points):
            bolt = f'Bolt{level}_{i}'
            shaft = part(bolt, 'cylinder', dict(radius=1.5,height=4), (x,y,z-1))
            shaft.setExpression('Height', f'{tray}_Blank.Height + 2 mm')
            part(f'Washer{level}_{i}', 'tube', dict(radius=3,bore_radius=1.6,height=1), (x,y,z-1))
            part(f'Head{level}_{i}', 'cylinder', dict(radius=3,height=1), (x,y,z-2))
            joints.append((tray, i, bolt, z))
    doc.recompute()
    return doc, physical, joints


def evaluate_geometry(doc, physical, joints):
    checks = []
    def check(name, passed, **evidence):
        checks.append(dict(name=name, passed=bool(passed), **evidence))
    for name in physical:
        s = doc.getObject(name).Shape
        check('valid:'+name, s.isValid() and len(s.Solids)==1 and s.Volume>0)
    for a,b in itertools.combinations(physical,2):
        sa,sb = doc.getObject(a).Shape, doc.getObject(b).Shape
        ba,bb = sa.BoundBox,sb.BoundBox
        separated = any(getattr(ba,axis+'Max') < getattr(bb,axis+'Min') or getattr(bb,axis+'Max') < getattr(ba,axis+'Min') for axis in 'XYZ')
        overlap = 0.0 if separated else sa.common(sb).Volume
        check('nonpenetration:'+a+':'+b, overlap <= 1e-6, overlap_mm3=overlap)
    for tray, i, bolt, z in joints:
        shaft = doc.getObject(bolt)
        blank = doc.getObject(tray+'_Blank')
        bore = doc.getObject(tray+f'_Seat{i}')
        gap = doc.getObject(tray).Shape.distToShape(shaft.Shape)[0]
        check('shaft_clearance:'+bolt, abs(gap-.2)<1e-6, measured_mm=gap)
        expression = dict(shaft.ExpressionEngine).get('Height','')
        check('dependency:'+bolt, tray+'_Blank.Height' in expression and abs(shaft.Height.Value-blank.Height.Value-2)<1e-7, expression=expression)
        protrusion = shaft.Shape.BoundBox.ZMax-(z+blank.Height.Value)
        check('protrusion:'+bolt, abs(protrusion-1)<1e-7, measured_mm=protrusion)
        check('bore_radius:'+bolt, abs(bore.Radius.Value-1.7)<1e-7)
    return dict(passed=all(c['passed'] for c in checks), checks=checks, physical_parts=len(physical))


def run(case, out):
    import FreeCAD as App
    import native
    doc, physical, joints = build(case)
    try:
        baseline = evaluate_geometry(doc,physical,joints)
        if not baseline['passed']: raise AssertionError('fixture failed baseline')
        before = {n:doc.getObject(n).Shape.Volume for n in physical}
        operations = []
        for level in range(case['levels']):
            op = dict(action='set_parameter',object_name=f'Tray{level}_Blank',parameter='Height',value=case['target_thickness'])
            native.set_parameter(doc,op['object_name'],op['parameter'],op['value'])
            operations.append(op)
        edited = evaluate_geometry(doc,physical,joints)
        w=case['plate_width']; expected=(w*w-case['bolt_count']*math.pi*1.7**2)*case['target_thickness']
        edited['analytic_tray_volume_passed']=all(abs(doc.getObject(f'Tray{i}').Shape.Volume-expected)<1e-5 for i in range(case['levels']))
        edited['unchanged_shell_passed']=abs(doc.Shell.Shape.Volume-before['Shell'])<1e-6
        path = out/(case['id']+'.FCStd'); doc.saveAs(str(path))
        App.closeDocument(doc.Name); doc=App.openDocument(str(path));doc.recompute()
        reopened=evaluate_geometry(doc,physical,joints)
        # Independent negative controls must fail even though native solids remain valid.
        native.set_parameter(doc,'Tray0_Seat0','Radius',1.4)
        clearance=evaluate_geometry(doc,physical,joints)
        native.set_parameter(doc,'Tray0_Seat0','Radius',1.7)
        shaft=doc.getObject('Bolt0_0');shaft.setExpression('Height',None)
        doc.recompute();dependency=evaluate_geometry(doc,physical,joints)
        shaft.setExpression('Height','Tray0_Blank.Height + 2 mm')
        doc.getObject('Head0_0').Placement.Base.z+=.5;doc.recompute()
        hidden=evaluate_geometry(doc,physical,joints)
        negative={key:{'detected':not report['passed'],'failures':[c for c in report['checks'] if not c['passed']]} for key,report in [('undersized_bore',clearance),('severed_expression',dependency),('internal_head_collision',hidden)]}
        passed=edited['passed'] and edited['analytic_tray_volume_passed'] and edited['unchanged_shell_passed'] and reopened['passed'] and all(n['detected'] for n in negative.values())
        return dict(case=case,passed=passed,baseline=baseline,edited=edited,reopened=reopened,negative_controls=negative,reference_operations=operations,artifact=str(path))
    finally:
        App.closeDocument(doc.Name)


def main():
    parser=argparse.ArgumentParser();parser.add_argument('--out',type=Path,default=ROOT/'out/cad-training');parser.add_argument('--validate-only',action='store_true');args=parser.parse_args()
    source=Path(__file__).with_name('cases.json');cases=load_cases(source)
    if args.validate_only:
        print(f'Validated {len(cases)} cases and held-out family isolation');return
    import FreeCAD as App
    args.out.mkdir(parents=True,exist_ok=True)
    reports=[run(c,args.out) for c in cases]
    result=dict(schema_version=1,passed=all(r['passed'] for r in reports),freecad_version=App.Version(),manifest_sha256=hashlib.sha256(source.read_bytes()).hexdigest(),evaluator_sha256=hashlib.sha256(Path(__file__).read_bytes()).hexdigest(),reports=reports)
    (args.out/'report.json').write_text(json.dumps(result,indent=2))
    # Only verified train examples can enter this reference trajectory corpus.
    with (args.out/'train.jsonl').open('w') as stream:
        for report in reports:
            if report['case']['split']=='train' and report['passed']:
                stream.write(json.dumps(dict(schema_version=1,id=report['case']['id'],provenance='synthetic_native_oracle',task='Increase every internal tray thickness to '+str(report['case']['target_thickness'])+' mm; preserve all bore clearances, bolt dependencies, and 1 mm protrusion.',fixture=report['case'],operations=report['reference_operations'],evidence='report.json',manufacturing_approved=False))+'\n')
    print(json.dumps(dict(passed=result['passed'],cases=len(reports),parts=[r['edited']['physical_parts'] for r in reports],report=str(args.out/'report.json'))))
    if not result['passed']:raise SystemExit(1)

if __name__=='__main__':main()
