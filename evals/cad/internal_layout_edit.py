"""End-to-end typed agent API edit of a saved dense native assembly.

Controlled integration task, not a blind model-accuracy measurement.
Run with cad/freecad/run.sh after internal_assembly_fixture.py.
"""
import hashlib,json,os
from pathlib import Path
import worker
ROOT=Path(__file__).resolve().parents[2]
source=ROOT/'out/freecad-internal-assembly/assembly.FCStd'
baseline=json.loads((source.parent/'result.json').read_text())
out=Path(os.environ.get('WRIGHT_CAD_OUT',ROOT/'out/freecad-internal-layout-edit')).resolve()
out.mkdir(parents=True,exist_ok=True)
source_hash=hashlib.sha256(source.read_bytes()).hexdigest()
trace=[]
def action(name,args):
    result=worker.METHODS[name](**args)
    trace.append(dict(action=name,args=args,result=result))
    return result

action('open_document',dict(path=str(source)))
doc=worker.active()
objects=baseline['fit']['objects']
contacts=[[p['a'],p['b']] for p in baseline['fit']['pairs'] if p['required_contact']]
before={n:doc.getObject(n).Shape.copy() for n in objects}
controls=next(o for o in action('inspect',{}) if o['name']=='Dimensions')
assert controls['editable_parameters']['LowerClearance']==12
# Task: increase clearance below the lower deck from 12 to 18 mm, preserving
# the upper stack spacing, enclosure, fasteners and component/deck contacts.
action('set_parameter',dict(object_name='Dimensions',parameter='LowerClearance',value=18))
fit=action('check',dict(objects=objects,contacts=contacts,tolerance_mm=.01,require_connected=True))
assert fit['passed'] and fit['pairs_total']==378
unchanged=['Base','Lid','LeftWall','RightWall','FrontWall','BackWall']
for n in unchanged:
    a,b=before[n],doc.getObject(n).Shape
    assert not a.cut(b).Solids and not b.cut(a).Solids
for n in ['LowerDeck','UpperDeck','UpperSpacer0','UpperDeckComponent0']:
    assert abs(doc.getObject(n).Shape.BoundBox.ZMin-before[n].BoundBox.ZMin-6)<1e-7
assert abs(doc.UpperDeckComponent0.Shape.distToShape(doc.Lid.Shape)[0]-9)<1e-7
assert abs(doc.LowerDeckComponent0.Shape.distToShape(doc.UpperDeck.Shape)[0]-10)<1e-7
assert doc.Dimensions.DeckSpacing.Value==18
saved=action('save',dict(directory=str(out),objects=objects))
assert hashlib.sha256(source.read_bytes()).hexdigest()==source_hash
report=dict(passed=True,scope='Controlled typed API integration, not independent agent design accuracy',source_sha256=source_hash,physical_parts=len(objects),pairs=fit['pairs_total'],unchanged_housing_parts=unchanged,stack_translation_mm=6,lid_clearance_mm=9,interdeck_clearance_mm=10,source_preserved=True,complete_product=False)
(out/'result.json').write_text(json.dumps(report,indent=2))
(out/'action-trace.json').write_text(json.dumps(trace,indent=2))
print(json.dumps(report))
