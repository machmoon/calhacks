"""Native FreeCAD integration smoke test; run with freecadcmd, no GUI needed.

WRIGHT_CAD_OUT chooses the output folder. Creates editable native features,
changes a dimension, saves/reopens the feature graph and checks STEP import.
"""
import json
import math
import os
from pathlib import Path
import traceback
import FreeCAD as App
import Part


def run():
    out=Path(os.environ['WRIGHT_CAD_OUT']).resolve()
    out.mkdir(parents=True,exist_ok=True)
    doc=App.newDocument('WrightNativeProbe')
    box=doc.addObject('Part::Box','MountBlank')
    box.Length=40
    box.Width=20
    box.Height=20
    bore=doc.addObject('Part::Cylinder','BoomSeat')
    bore.Radius=6.35
    bore.Height=42
    bore.Placement=App.Placement(App.Vector(-1,10,10),App.Rotation(App.Vector(0,0,1),App.Vector(1,0,0)))
    cut=doc.addObject('Part::Cut','Mount')
    cut.Base=box
    cut.Tool=bore
    doc.recompute()
    expected=40*20*20-math.pi*6.35**2*40
    assert cut.Shape.isValid() and len(cut.Shape.Solids)==1
    assert abs(cut.Shape.Volume-expected)<1e-5
    initial_volume=cut.Shape.Volume
    box.Width=24
    doc.recompute()
    expected_changed=40*24*20-math.pi*6.35**2*40
    assert abs(cut.Shape.Volume-expected_changed)<1e-5
    native=out/'native_mount.FCStd'
    doc.saveAs(str(native))
    Part.export([cut],str(out/'native_mount.step'))
    App.closeDocument(doc.Name)
    reopened=App.openDocument(str(native))
    restored=reopened.getObject('Mount')
    assert restored.Base.Name=='MountBlank' and restored.Tool.Name=='BoomSeat'
    assert restored.Shape.isValid() and abs(restored.Shape.Volume-expected_changed)<1e-5
    step=Part.read(str(out/'native_mount.step'))
    assert step.isValid() and abs(step.Volume-expected_changed)<1e-5
    result=dict(passed=True,version=App.Version(),occ_version=Part.OCC_VERSION,
                native_features=[o.TypeId for o in reopened.Objects],
                original_volume_mm3=initial_volume,edited_volume_mm3=restored.Shape.Volume,
                persistent_boolean_links=True,step_roundtrip_valid=True)
    aircraft=os.environ.get('WRIGHT_CAD_AIRCRAFT_STEP')
    if aircraft:
        model=Part.read(aircraft)
        assert model.isValid()
        assert len(model.Solids)==50
        result['aircraft_step_import']=dict(valid=True,solids=len(model.Solids))
    (out/'result.json').write_text(json.dumps(result,indent=2,allow_nan=False))
    print('WRIGHT_FREECAD_RESULT '+json.dumps(result),flush=True)
    App.closeDocument(reopened.Name)


try:
    run()
except Exception:
    print(traceback.format_exc(),flush=True)
    raise
