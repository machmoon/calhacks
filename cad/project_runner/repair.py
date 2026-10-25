"""Propose bounded native cuts from measured collisions; never execute model code."""
import argparse
import ast
import hashlib
import json
import os
from pathlib import Path
import subprocess

ROOT=Path(__file__).resolve().parents[2]


def parse_repairs(source, physical, clashes):
    tree=ast.parse(source)
    if len(tree.body)!=1 or not isinstance(tree.body[0],ast.Assign):
        raise ValueError('Expected one REPAIRS assignment')
    node=tree.body[0]
    if len(node.targets)!=1 or not isinstance(node.targets[0],ast.Name) or node.targets[0].id!='REPAIRS':
        raise ValueError('Expected REPAIRS assignment')
    rows=ast.literal_eval(node.value)
    if not isinstance(rows,list) or not 1<=len(rows)<=12:
        raise ValueError('Expected one to twelve repairs')
    allowed={frozenset((p['a'],p['b'])) for p in clashes}
    seen=set()
    for row in rows:
        if not isinstance(row,dict) or set(row) not in ({'target','tool'},{'target','tool','tool_form'}) or not all(isinstance(v,str) for v in row.values()):
            raise ValueError('Each repair must identify only target and tool')
        if row.get('tool_form','actual') not in {'actual','outer_envelope'}:
            raise ValueError('Unknown source-derived tool form')
        pair=frozenset((row['target'],row['tool']))
        if row['target'] not in physical or len(pair)!=2 or pair not in allowed or pair in seen:
            raise ValueError('Repair must cut a declared manufactured part at one measured clash')
        seen.add(pair)
    return rows


def main():
    p=argparse.ArgumentParser();p.add_argument('manifest');p.add_argument('pairs');p.add_argument('output');a=p.parse_args()
    manifest=json.loads(Path(a.manifest).read_text());pairs=json.loads(Path(a.pairs).read_text())
    if manifest['native_sha256']!=pairs['native_sha256'] or pairs.get('status')!='complete':
        raise ValueError('Require completed collision evidence for this native artifact')
    clashes=[row for row in pairs['check']['pairs'] if row['clash']]
    output=Path(a.output).resolve();output.mkdir(parents=True,exist_ok=False)
    prompt=('Propose at most12 native Boolean cuts to repair measured overlaps in an existing FreeCAD assembly. '
      'Return exactly REPAIRS = [{"target":"name","tool":"name","tool_form":"actual"}, ...] in submit_candidate.python. '
      'Only target manufactured parts listed below, never purchased hardware. Every target/tool pair must be '
      'one of the measured clashes. The tool is RETAINED as a physical part. These nominal seat cuts do not '
      'qualify clearance, strength, retention or insertion paths. No geometry is deleted or moved. '
      'Prefer cutting mounting bodies/receiving seats around spars and trays, preserving the spars, joiner and tray. '
      'For hollow spars, tool_form="outer_envelope" requests their existing native outer cylinder, mirrored when necessary; '
      'this follows the original cylinder parameters and creates a through-bore, without guessing dimensions. '
      'The executor strictly recognizes coaxial cylinder-minus-cylinder tubes and rejects all other outer-envelope requests. '
      'Using the actual hollow tube as a cutter can leave a disconnected plug and will be rejected. '
      'Native execution will reject disconnected parts or removal over10percent of a target volume. '
      'This is a repair experiment, not a complete aircraft claim.\nMANUFACTURED PARTS:\n'+
      json.dumps(manifest['physical'])+'\nMEASURED CLASHES:\n'+json.dumps(clashes))
    request={'model':'gpt-6-astra','max_tokens':2000,'system':'Use only the provided native geometry evidence. Submit one literal repair list.',
      'messages':[{'role':'user','content':prompt}], 'tools':[{'name':'submit_candidate','description':'Propose bounded cuts, preserving all components.',
       'input_schema':{'type':'object','additionalProperties':False,'required':['python','summary'],
        'properties':{'python':{'type':'string'},'summary':{'type':'string'}}}}]}
    req=output/'model-request.json';req.write_text(json.dumps(request,indent=2))
    binary=os.environ.get('WRIGHT_CAD_MODEL_RUNNER',str(ROOT/'out/tools/wright-cad-model'))
    with (output/'planner.log').open('w') as log:
        subprocess.run([binary,str(req),str(output/'planning')],check=True,timeout=240,stdout=log,stderr=subprocess.STDOUT)
    candidate=output/'planning/candidate.py';repairs=parse_repairs(candidate.read_text(),manifest['physical'],clashes)
    (output/'repairs.json').write_text(json.dumps({'native_sha256':manifest['native_sha256'],
      'manifest':str(Path(a.manifest).resolve()),'pairs':str(Path(a.pairs).resolve()),
      'proposal_sha256':hashlib.sha256(candidate.read_bytes()).hexdigest(),'repairs':repairs},indent=2))
    print(output/'repairs.json')


if __name__=='__main__':main()
