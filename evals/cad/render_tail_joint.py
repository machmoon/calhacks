"""Native selected-part views of the proposed reference tail joint."""
import base64,json
from pathlib import Path
import worker
ROOT=Path(__file__).resolve().parents[2];OUT=ROOT/'out/tsung-vtol-tail-joint'
worker.open_document(str(OUT/'assembly.FCStd'))
views=[('joint',['FinClampDrill1','LowerClampDrill1'],-38,25),
       ('lower-clamp',['LowerClampDrill1'],-52,40)]
for name,objects,azimuth,elevation in views:
    result=worker.METHODS['render'](objects=objects,width=640,height=480,
        azimuth_degrees=azimuth,elevation_degrees=elevation)
    blocks=result.pop('content_blocks')
    (OUT/(name+'.png')).write_bytes(base64.b64decode(blocks[0]['source']['data']))
    (OUT/(name+'-render.json')).write_text(json.dumps(result,indent=2))
    print(name,result['triangles'],result['source_geometry_unchanged'],flush=True)
