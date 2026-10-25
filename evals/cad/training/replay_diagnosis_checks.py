"""Replay a completed read-only Astra diagnosis's actual checks in fresh FreeCAD.
Does not feed independent evidence to the model or alter the source document.
"""
import argparse
import hashlib
import json
import math
from pathlib import Path
import sys
ROOT=Path(__file__).resolve().parents[3]
sys.path.insert(0,str(ROOT/'cad/freecad'))


def equivalent(expected,actual,summary=False):
    # Actual must always be the full fresh native report, even for compact output.
    for key in ('overlap_count','unmet_contact_count','connected','passed','pairs_total','require_connected','tolerance_mm'):
        if expected.get(key)!=actual.get(key):return False
    if sorted(expected.get('objects',[]))!=sorted(actual.get('objects',[])):return False
    if sorted(sorted(c) for c in expected.get('components',[]))!=sorted(sorted(c) for c in actual.get('components',[])):return False
    all_pairs=actual['pairs']
    if summary:
        if expected.get('pair_details_scope')!='all_clashes_and_declared_contacts':return False
        required=[p for p in all_pairs if p['clash'] or p['required_contact']]
        if expected.get('omitted_pair_count')!=len(all_pairs)-len(required):return False
        contacts=sum(not p['clash'] and p['distance_mm'] is not None and p['distance_mm']<=actual['tolerance_mm'] for p in all_pairs)
        if expected.get('contact_pair_count')!=contacts:return False
    else:
        if expected.get('pair_details_scope') is not None:return False
        required=all_pairs
    ep={tuple(sorted([p['a'],p['b']])):p for p in expected['pairs']}
    ap={tuple(sorted([p['a'],p['b']])):p for p in required}
    if len(ep)!=len(expected['pairs']) or ep.keys()!=ap.keys():return False
    for key,p in ep.items():
        q=ap[key]
        for field in ('clash','floating','required_contact','broad_phase'):
            if p.get(field)!=q.get(field):return False
        for field in ('overlap_mm3','distance_mm','distance_lower_bound_mm'):
            a=p.get(field);b=q.get(field)
            if a is None or b is None:
                if a!=b:return False
            elif not math.isfinite(a) or abs(a-b)>max(1e-5,abs(a)*1e-8):return False
    return True


def assess(directory):
    import FreeCAD as App
    import worker
    snapshot=json.loads((directory.parent/'source-snapshot.json').read_text())
    source=Path(snapshot['source']);before=hashlib.sha256(source.read_bytes()).hexdigest()
    trace=json.loads((directory/'full-request-response-trace.json').read_text());calls={};observed={};usage={};inspection_calls=[]
    for turn in trace:
        if turn['route']!='astra':continue
        response=turn['response']
        for k,v in response.get('usage',{}).items():usage[k]=usage.get(k,0)+v
        for block in response.get('content',[]):
            if block.get('type')!='tool_use' or block.get('name')!='cad_action':continue
            action=block['input']['action'];args=json.loads(block['input']['args_json'])
            if action=='check':calls[block['id']]=args
            if action=='inspect':inspection_calls.append(args)
        for message in turn['request'].get('messages',[]):
            blocks=message.get('content',[])
            if not isinstance(blocks,list):continue
            for block in blocks:
                if block.get('type')!='tool_result':continue
                content=block.get('content','')
                texts=[content] if isinstance(content,str) else [b.get('text','') for b in content if b.get('type')=='text']
                for text in texts:
                    try:value=json.loads(text)
                    except (ValueError,TypeError):continue
                    if isinstance(value,dict) and 'pairs' in value:observed[block['tool_use_id']]=value
    worker.open_document(str(source));replays=[]
    try:
        for tool_id,args in calls.items():
            full_args=dict(args);summary=full_args.pop("summary",False)
            native=worker.check(**full_args);expected=observed.get(tool_id)
            replays.append(dict(tool_use_id=tool_id,args=args,model_received_result=expected is not None,matches=expected is not None and equivalent(expected,native,summary=summary),native=native))
    finally:App.closeDocument(worker.active().Name)
    after=hashlib.sha256(source.read_bytes()).hexdigest()
    return dict(scope='Independent replay of model-selected native fit checks; does not establish completeness of mechanical reasoning or retention',source_sha256=before,source_unchanged=before==after==snapshot['sha256'],usage=usage,inspection_calls=inspection_calls,all_inspections_summary_or_scoped=all(a.get('summary') is True or bool(a.get('objects')) for a in inspection_calls),checks=replays,passed=bool(replays) and all(r['matches'] for r in replays) and before==after==snapshot['sha256'])


if __name__=='__main__':
    parser=argparse.ArgumentParser();parser.add_argument('directory',type=Path);args=parser.parse_args()
    result=assess(args.directory.resolve());(args.directory/'independent-replay.json').write_text(json.dumps(result,indent=2));print(json.dumps({k:result[k] for k in ('passed','source_unchanged','source_sha256','usage','all_inspections_summary_or_scoped')}))
    if not result['passed']:raise SystemExit(1)
