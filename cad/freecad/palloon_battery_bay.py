"""Adjustable provisional battery envelope and open-top bay, not a measured pack."""
import json
import os
from pathlib import Path
import FreeCAD as App
import Part
import layout
import motion
import native
import worker

ROOT = Path(__file__).resolve().parents[2]
OUT = Path(os.environ.get('WRIGHT_CAD_OUT', ROOT/'out/palloon-battery-bay')).resolve()
OUT.mkdir(parents=True, exist_ok=True)
ASSUMPTIONS = ROOT/'cad/projects/agents_in_space/battery-envelope-assumptions.json'
assumptions = json.loads(ASSUMPTIONS.read_text())
worker.document('PalloonProvisionalBatteryBay')
doc = worker.active()
layout.create(doc, 'BatteryLayout', {
    'BayLength': {'value':155, 'min':155, 'max':185},
    'BayWidth': {'value':80, 'min':80, 'max':100},
    'BayHeight': {'value':55, 'min':55, 'max':65},
    'Wall': {'value':2, 'min':1, 'max':4},
    'Floor': {'value':2, 'min':1, 'max':4},
    'BodyLength': {'value':150, 'min':100, 'max':150},
    'BodyWidth': {'value':76, 'min':50, 'max':76},
    'BodyHeight': {'value':36, 'min':24, 'max':55},
    'LeadReserve': {'value':30, 'min':30, 'max':50},
})

def box(name, expressions):
    obj = doc.addObject('Part::Box', name)
    for key, expression in expressions.items():
        obj.setExpression(key, expression)
    return obj

outer = box('BayOuter', {'Length':'BatteryLayout.BayLength + 2 * BatteryLayout.Wall',
    'Width':'BatteryLayout.BayWidth + 2 * BatteryLayout.Wall',
    'Height':'BatteryLayout.BayHeight + BatteryLayout.Floor'})
cavity = box('BayCavity', {'Length':'BatteryLayout.BayLength', 'Width':'BatteryLayout.BayWidth',
    'Height':'BatteryLayout.BayHeight + 1 mm', 'Placement.Base.x':'BatteryLayout.Wall',
    'Placement.Base.y':'BatteryLayout.Wall', 'Placement.Base.z':'BatteryLayout.Floor'})
bay = doc.addObject('Part::Cut', 'BatteryBay'); bay.Base=outer; bay.Tool=cavity
bay.Label='Provisional open-top battery bay — 2 mm walls/floor'
body = box('BatteryEnvelope', {'Length':'BatteryLayout.BodyLength','Width':'BatteryLayout.BodyWidth',
    'Height':'BatteryLayout.BodyHeight',
    'Placement.Base.x':'BatteryLayout.Wall + (BatteryLayout.BayLength - BatteryLayout.BodyLength) / 2',
    'Placement.Base.y':'BatteryLayout.Wall + (BatteryLayout.BayWidth - BatteryLayout.BodyWidth) / 2',
    'Placement.Base.z':'BatteryLayout.Floor'})
body.Label='ASSUMED custom battery envelope — not measured'
lead = box('NonphysicalLeadKeepout', {'Length':'BatteryLayout.LeadReserve',
    'Width':'BatteryLayout.BayWidth', 'Height':'BatteryLayout.BayHeight',
    'Placement.Base.x':'BatteryLayout.BayLength + 2 * BatteryLayout.Wall',
    'Placement.Base.y':'BatteryLayout.Wall','Placement.Base.z':'BatteryLayout.Floor'})
connector = box('NonphysicalConnectorKeepout', {'Length':'30 mm','Width':'25 mm','Height':'20 mm',
    'Placement.Base.x':'BatteryLayout.BayLength + 2 * BatteryLayout.Wall',
    'Placement.Base.y':'BatteryLayout.Wall + (BatteryLayout.BayWidth - 25 mm)/2',
    'Placement.Base.z':'BatteryLayout.Floor'})
for obj in (lead, connector):
    obj.addProperty('App::PropertyBool','NonphysicalKeepout','Wright'); obj.NonphysicalKeepout=True
    obj.Label=obj.Name+' — excluded from physical exports'
for obj in (body,bay):
    obj.addProperty('App::PropertyString','DesignStatus','Wright')
    obj.DesignStatus='Provisional design assumption; no actual custom-pack measurement'
doc.recompute()
native.validate_document(doc)


def evidence():
    body=worker.active().BatteryEnvelope; bay=worker.active().BatteryBay
    dims=worker.active().BatteryLayout
    b=body.Shape.BoundBox; wall=float(dims.Wall); floor=float(dims.Floor)
    contained=(b.XMin>=wall-1e-8 and b.XMax<=wall+float(dims.BayLength)+1e-8
        and b.YMin>=wall-1e-8 and b.YMax<=wall+float(dims.BayWidth)+1e-8
        and b.ZMin>=floor-1e-8 and b.ZMax<=floor+float(dims.BayHeight)+1e-8)
    overlap=body.Shape.common(bay.Shape).Volume
    movement=motion.check_translation([body],[bay],[0,0,70])
    assert contained and overlap<=1e-6 and movement['status']=='clear'
    expected=(float(dims.BayLength)+2*wall)*(float(dims.BayWidth)+2*wall)*(float(dims.BayHeight)+floor)-float(dims.BayLength)*float(dims.BayWidth)*float(dims.BayHeight)
    assert abs(bay.Shape.Volume-expected)<1e-6
    assert worker.check(['BatteryEnvelope','BatteryBay'],[['BatteryEnvelope','BatteryBay']])['passed']
    return dict(bay_length_mm=float(dims.BayLength),contained=contained,overlap_mm3=overlap,
        bay_material_volume_mm3=bay.Shape.Volume,analytic_bay_volume_mm3=expected,
        removal=movement,body=native.describe(body),bay=native.describe(bay))


def save_with_keepouts(directory):
    result=worker.save(str(directory),['BatteryEnvelope','BatteryBay'])
    current=worker.active()
    for obj in (current.NonphysicalLeadKeepout,current.NonphysicalConnectorKeepout):obj.Visibility=True
    current.save()  # Native keepouts visible; physical STEP/GLB selection remains two parts.
    return result

baseline=evidence()
exports=save_with_keepouts(OUT)
native.set_parameter(doc,'BatteryLayout','BayLength',185)
expanded=evidence(); expanded_exports=save_with_keepouts(OUT/'expanded')
App.closeDocument(doc.Name); worker._doc=None
worker.restore(expanded_exports['native']); reopened=evidence()
assert reopened['bay_length_mm']==185
assert worker.active().NonphysicalLeadKeepout.Visibility
native.set_parameter(worker.active(),'BatteryLayout','BayLength',155)
restored=evidence(); assert restored['bay_length_mm']==155
# Leave default artifact unchanged; demonstrate reopened expressions can also return to default.
report=dict(status='Provisional geometry passed; actual pack fit unverified',assumptions=str(ASSUMPTIONS),
    selected_physical_outputs=['BatteryEnvelope','BatteryBay'],nonphysical_keepouts=['NonphysicalLeadKeepout','NonphysicalConnectorKeepout'],
    default=baseline,expanded=expanded,reopened_expanded=reopened,reopened_restored_default=restored,
    exports=exports,expanded_exports=expanded_exports,
    limitations=['Battery body is a homogeneous placeholder, not battery construction or measured mass.',
      'Wall/floor thicknesses are provisional; no retention, load, thermal or manufacturing validation.',
      'Keepouts reserve space only; connector unselected, no cable routing through the wall modeled.',
      'Straight-up removal checks body against bay only; no installed cables, straps or surrounding payload.',
      'No BMS, fasteners, lid, straps or electrical construction added.'])
(OUT/'result.json').write_text(json.dumps(report,indent=2))
App.closeDocument(worker.active().Name);worker._doc=None
print('Battery bay passed: containment, no penetration, straight-up removal, 155 -> 185 mm edit, saved/reopened dependencies; two physical exports.')
