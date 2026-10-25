"""Derive explicit boom seats for the aircraft's mounting blanks.

This produces a separate revised CAD plan. It does not alter the aerodynamic
source model or silently subtract all colliding components. Only declared
boom/clamp, boom/lift-mount and boom/tail-joint cylindrical interfaces are cut.
Mount fastening, split-clamp closure and strength are still unverified.
"""
import json
from pathlib import Path
import sys


def refine(plan):
    import copy
    revised=copy.deepcopy(plan)
    items={p['name']:p for p in revised['instances']}
    edits=[]
    for side in ('R','L'):
        boom=items[f'part_boom_{side}']['params']
        for name in (f'part_boom_clamp_{side}0',f'part_boom_clamp_{side}1',
                     f'part_lift_mount_F{side}',f'part_lift_mount_R{side}',f'part_tail_joint_{side}'):
            target=items[name]
            if target['part']!='common.block':
                raise ValueError(f'{name}: expected rectangular mount blank')
            location=target['placement']['loc']
            if location['rot']!=[0.,0.,0.]:
                raise ValueError('seat conversion requires an unrotated local blank')
            origin=location['pos']
            local=lambda p:[v-o for v,o in zip(p,origin)]
            target['part']='common.mount_block'
            target['params']['bores']=[[local(boom['start']),local(boom['end']),boom['radius']]]
            edits.append(dict(instance=name,feature='cylindrical boom seat',
                              source=f'part_boom_{side}',radial_clearance_mm=0.))
    revised['name']=plan['name']+'_seated_mounts'
    revised['cad_features']=edits
    revised['scope']='CAD refinement: nominal boom seats; wing skins, fasteners and several other interfaces remain incomplete'
    return revised


if __name__=='__main__':
    source,destination=map(Path,sys.argv[1:])
    destination.write_text(json.dumps(refine(json.loads(source.read_text())),indent=2))
