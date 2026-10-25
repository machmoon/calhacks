"""Documented CAD-version support must not become an unconditional bypass."""
import os,sys,json
from pathlib import Path
sys.path.insert(0,os.environ['WRIGHT_ROCKET_WORKBENCH'])
import FreeCAD as App
from Rocket.Importer.OpenRocket.OpenRocket import OpenRocketElement
from Rocket.Exceptions import UnsupportedVersion
for version in ['1.10','1.11']:
    OpenRocketElement(None,'openrocket',{'version':version},None,'fixture.ork',1)
for version in ['1.12','2.0','garbage']:
    try:OpenRocketElement(None,'openrocket',{'version':version},None,'fixture.ork',1)
    except UnsupportedVersion:pass
    else:raise AssertionError('unknown format accepted: '+version)
out=Path(os.environ['WRIGHT_CAD_OUT']);out.mkdir(parents=True,exist_ok=True)
(out/'version-tests.json').write_text(json.dumps(dict(passed=True,supported_tested=['1.10','1.11'],rejected=['1.12','2.0','garbage'],scope='Version dispatch only; component coverage evaluated separately')))
print('Explicit CAD version dispatch tests passed')
