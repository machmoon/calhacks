"""Check a detected coupler/bulkhead overlap against original OR dimensions."""
import hashlib
import json
import math
from pathlib import Path
import xml.etree.ElementTree as ET
import zipfile

ROOT = Path(__file__).resolve().parents[2]
refs = ROOT/'evals/cad/references'
source = next(row for row in json.loads((refs/'sources.json').read_text())
              if row['file'] == 'Two stage high power rocket.ork')
raw = (refs/source['file']).read_bytes()
assert hashlib.sha256(raw).hexdigest() == source['sha256']
with zipfile.ZipFile(refs/source['file']) as archive:
    xml = ET.fromstring(archive.read('rocket.ork'))
coupler = next(e for e in xml.iter('tubecoupler')
               if e.findtext('id') == '1b6ccd8c-3617-4ef0-a8b9-8d7c2254b5df')
bulkhead = next(e for e in coupler.find('subcomponents')
                if e.findtext('id') == 'ea8ebe8b-955b-4d0e-bc42-bad00893b21e')
def mm(element, field):
    value = float(element.findtext(field))*1000
    assert math.isfinite(value)
    return value
assert bulkhead.tag == 'bulkhead'
assert bulkhead.find('axialoffset').attrib['method'] == 'bottom'
assert mm(coupler,'radialposition') == mm(bulkhead,'radialposition') == 0
r = mm(coupler,'outerradius'); bore = r-mm(coupler,'thickness')
length = mm(coupler,'length'); br = mm(bulkhead,'outerradius')
bl = mm(bulkhead,'length'); start = length-bl+mm(bulkhead,'axialoffset')
def overlap(radius, z):
    axial = max(0, min(length,z+bl)-max(0,z))
    radial_area = math.pi*max(0, min(r,radius)**2-bore**2)
    return axial*radial_area
expected = overlap(br,start)
assert expected > 0
assert overlap(bore,start) == 0
assert overlap(br,length+1) == 0
report = dict(source=source, coupler_source_id=coupler.findtext('id'),
    bulkhead_source_id=bulkhead.findtext('id'),
    dimensions_mm=dict(coupler_radius=r,coupler_bore_radius=bore,
        coupler_length=length,bulkhead_radius=br,bulkhead_length=bl,
        bulkhead_start_from_parent=start),
    analytic_source_overlap_mm3=expected,
    source_dimensions_require_intersection=True,
    negative_controls_passed=True,
    conclusion='This source-defined coaxial pair intersects even before CAD import. '
        'Do not silently shrink or relocate it to make the importer pass. '
        'A manufacturing design needs an explicitly derived fit or stepped interface.',
    scope='One source pair only; does not classify the other detected overlaps or establish source author intent.')
out=ROOT/'out/openrocket-assembly-fit';out.mkdir(parents=True,exist_ok=True)
(out/'source-overlap-analysis.json').write_text(json.dumps(report,indent=2))
print(json.dumps(report,indent=2))
