"""Straight service-path evidence on the real saved two-deck fixture."""
import json
from pathlib import Path
import worker
ROOT=Path(__file__).resolve().parents[2]
source=ROOT/'out/freecad-internal-assembly'
worker.open_document(str(source/'assembly.FCStd'))
physical=json.loads((source/'result.json').read_text())['fit']['objects']
move='UpperDeckComponent0'
call=worker.METHODS['check_translation']
closed=call(moving=[move],obstacles=[n for n in physical if n!=move],translation=[0,0,50])
assert closed['status']=='blocked' and any(c['obstacle']=='Lid' for c in closed['collisions'])
open_lid=call(moving=[move],obstacles=[n for n in physical if n not in [move,'Lid']],translation=[0,0,50])
assert open_lid['status']=='clear'
lower='LowerDeckComponent0'
blocked_lower=call(moving=[lower],obstacles=[n for n in physical if n not in [lower,'Lid']],translation=[0,0,50])
assert blocked_lower['status']=='blocked' and any(c['obstacle']=='UpperDeck' for c in blocked_lower['collisions'])
out=ROOT/'out/freecad-internal-service-access';out.mkdir(exist_ok=True)
(out/'result.json').write_text(json.dumps(dict(passed=True,scope='Selected envelope geometry only; fasteners, connectors and cable removal not modeled',closed_lid=closed,lid_removed=open_lid,lower_component=blocked_lower),indent=2))
print('Service path: upper component blocked by closed lid, clear with lid absent; lower component blocked by upper deck')
