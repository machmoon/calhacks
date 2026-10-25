"""Run with cad/freecad/run.sh; real kernel positive and negative controls."""
import json
import tempfile
from pathlib import Path
import FreeCAD as App
import Part
from export_probe import compare, sha

with tempfile.TemporaryDirectory() as directory:
    root=Path(directory)
    module=root/'fixture_features.py';module.write_text('# No scripted fixture features\n')
    doc=App.newDocument('ExportControl')
    box=doc.addObject('Part::Box','Box');box.Length=20;box.Width=10;box.Height=5
    doc.recompute();native=root/'assembly.FCStd';doc.saveAs(str(native))
    step=root/'airframe.step';Part.export([box],str(step));App.closeDocument(doc.Name)
    manifest=root/'manifest.json'
    manifest.write_text(json.dumps({'native_sha256':sha(native),'proxy_module':str(module),
      'generator_sources':{str(module):sha(module)},'physical':['Box'],'step':{'path':str(step)}}))
    compare(native,manifest,root/'good.json')
    assert json.loads((root/'good.json').read_text())['passed']
    shifted=Part.makeBox(20,10,5,App.Vector(.2,0,0));shifted.exportStep(str(step))
    compare(native,manifest,root/'shifted.json')
    assert not json.loads((root/'shifted.json').read_text())['passed']
    extra=Part.makeCompound([Part.makeBox(20,10,5),Part.makeBox(1,1,1,App.Vector(40,0,0))])
    extra.exportStep(str(step));compare(native,manifest,root/'extra.json')
    assert not json.loads((root/'extra.json').read_text())['passed']
    print('Export controls: exact roundtrip passes; displacement and extra solid fail')
