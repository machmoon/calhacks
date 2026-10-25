"""Fail-closed CAD reference corpus curation; never invent agent transcripts."""
import hashlib,json
from pathlib import Path

def sha(path):return hashlib.sha256(Path(path).read_bytes()).hexdigest()
def validate(records):
    ids=set();groups={};hashes={}
    for r in records:
        if r['id'] in ids:raise ValueError('duplicate id')
        ids.add(r['id']);split=r['split']
        if split not in ('train','validation','test'):raise ValueError('unknown split')
        if r['origin']!='reference_program' or r['model'] is not None:raise ValueError('reference corpus must not impersonate model output')
        if r['verification']['passed'] is not True or not r['verification']['checks']:raise ValueError('unverified reference')
        if any(c['passed'] is not True for c in r['verification']['checks']):raise ValueError('failed geometry check')
        if not r['trace'] or any(set(t)!={'action','arguments','result'} for t in r['trace']):raise ValueError('incomplete action/result trace')
        if r['source']['kind']=='upstream_so101' and split=='train':raise ValueError('SO101 benchmark geometry excluded from training')
        for index,key in ((groups,r['lineage_group']),(hashes,r['source']['sha256'])):
            if key in index and index[key]!=split:raise ValueError('source or geometry family crosses split')
            index[key]=split
    return records

def export(records,out):
    validate(records);out=Path(out);out.mkdir(parents=True,exist_ok=True)
    for split in ('train','validation','test'):
        (out/(split+'.jsonl')).write_text(''.join(json.dumps(r,allow_nan=False)+'\n' for r in records if r['split']==split))
    (out/'manifest.json').write_text(json.dumps({'schema_version':1,'counts':{s:sum(r['split']==s for r in records) for s in ('train','validation','test')},'weights_trained':False,'model_success_rate_measured':False,'provider_training_upload_approved':False},indent=2))
