"""Subscription-backed design parameters, parsed as data, never executed code."""
import ast
import hashlib
import json
import math
import os
import subprocess
from pathlib import Path
from timing import span

BOUNDS = {'Span': (1600,2000), 'RootChord': (280,340),
          'BoomHalfSpacing': (280,380), 'MotorFrontX': (80,140),
          'MotorRearX': (680,740)}
APPEARANCE_BOUNDS = {'WingletHeight': (40,110), 'WingletCantAngle': (5,35),
                     'WingletSweepOffset': (10,60), 'TailDihedralAngle': (20,40)}


def parse_design(source, appearance=False):
    bounds = dict(BOUNDS, **APPEARANCE_BOUNDS) if appearance else BOUNDS
    tree=ast.parse(source)
    if len(tree.body)!=1 or not isinstance(tree.body[0],ast.Assign):
        raise ValueError('Model must submit only DESIGN = {...}')
    node=tree.body[0]
    if len(node.targets)!=1 or not isinstance(node.targets[0],ast.Name) or node.targets[0].id!='DESIGN':
        raise ValueError('Expected DESIGN assignment')
    design=ast.literal_eval(node.value)
    if not isinstance(design,dict) or set(design)!=set(bounds):
        raise ValueError('Model must supply exactly the supported airframe parameters')
    for key,value in design.items():
        if isinstance(value,bool) or not isinstance(value,(int,float)) or not math.isfinite(value) or not bounds[key][0]<=value<=bounds[key][1]:
            raise ValueError(f'Invalid {key}: {value}')
    return design


def plan(root,brief,request,output,appearance=False,pdf=None,timing=None):
    bounds = dict(BOUNDS, **APPEARANCE_BOUNDS) if appearance else BOUNDS
    binary=Path(os.environ.get('WRIGHT_CAD_MODEL_RUNNER',str(root/'out/tools/wright-cad-model')))
    if not binary.is_file():
        raise RuntimeError('Astra project planner missing: build evals/cad/so_arm101_one_shot and set WRIGHT_CAD_MODEL_RUNNER')
    instructions=('Choose provisional geometry for the native FreeCAD VTOL recipe. '
      'You cannot certify flight performance or resolve missing manufacturer interfaces. '
      'Submit exactly one tool call. Its python field must contain only DESIGN = {..}, '
      'with every key and numeric values within these bounds: '+json.dumps(bounds)+
      '. Angle keys use degrees; all other keys use mm. '+
      ('User-selected configuration: clean inverted-V, two inclined tail halves rising inward from the booms, separate ruddervators; no vertical fins or flat roof. Defaults: WingletHeight70 WingletCantAngle20 WingletSweepOffset30 TailDihedralAngle30. These numeric defaults are recipe suggestions, not values specified by the user or optimized results. TailDihedralAngle is above horizontal; WingletCantAngle is from vertical. Supply design_review using the supplied evidence; do not claim aerodynamic optimality or checks that have not run. ' if appearance else '')+
      '. Prefer Span1800 RootChord300 BoomHalfSpacing320 MotorFrontX120 MotorRearX700 '
      'unless the brief substantiates another choice. Battery, motors, wiring and bought '
      'parts are source-controlled and may not be silently substituted. This is a prepared '
      'parametric recipe, not arbitrary code generation. The engineering bounds are unverified.\n'
      'PROJECT BRIEF (user-authorized file contents):\n'+brief.read_text()+
      '\nUSER REQUEST:\n'+request)
    body={'model':'gpt-6-astra','max_tokens':2000,'system':'Plan a native FreeCAD project with explicit limitations. Never claim checks ran.',
      'messages':[{'role':'user','content':instructions}],
      'tools':[{'name':'submit_candidate','description':'Submit a literal DESIGN assignment and concise rationale.',
        'input_schema':{'type':'object','additionalProperties':False,'required':['python','summary'],
          'properties':{'python':{'type':'string'},'summary':{'type':'string'}}}}]}
    if appearance:
        from design_review import SCHEMA, EVIDENCE
        body['tools'][0]['input_schema']['required'].append('design_review')
        body['tools'][0]['input_schema']['properties']['design_review']=SCHEMA
        body['messages'][0]['content']+='\nREFERENCE EVIDENCE:\n'+json.dumps(EVIDENCE)
    if pdf is not None:
        from pdf_context import content_blocks, report_schema
        schema=body['tools'][0]['input_schema']
        schema['required'].append('pdf_report')
        schema['properties']['pdf_report']=report_schema(bounds)
        instructions=body['messages'][0]['content']+'\nAccount for every parameter in pdf_report. Cite PDF pages for document-derived values; identify recipe defaults honestly. Set supported=false for unrelated PDFs, conflicting tail architectures, out-of-range dimensions or other requirements this prepared recipe cannot implement. Never label unsupported requirements as satisfied. If unsupported, submit bounded defaults only to complete the tool schema; the runner will stop before building.'
        body['messages'][0]['content']=[{'type':'text','text':instructions}]+content_blocks(pdf)
        body['max_tokens']=6000
    frozen=output/'model-request.json';frozen.write_text(json.dumps(body,indent=2))
    with span(timing,'model_subprocess') as model_span:
        with (output/'planner.log').open('w') as log:
            p=subprocess.run([str(binary),str(frozen),str(output/'planning')],cwd=root,stdout=log,stderr=subprocess.STDOUT,timeout=240)
        if p.returncode:raise RuntimeError('Astra planning failed; see planner.log. No fallback or CAD build was started.')
    if timing:timing.response(output/'planning/response.json')
    with span(timing,'candidate_validation'):
        if pdf is not None:
            from pdf_context import validate_report
            submission=json.loads((output/'planning/submission.json').read_text())
            raw_report=submission.get('pdf_report')
            (output/'pdf-interpretation.json').write_text(json.dumps(raw_report,indent=2))
            validate_report(raw_report,bounds,pdf['pages'])
        candidate=output/'planning/candidate.py'
        design=parse_design(candidate.read_text(),appearance=appearance)
        if appearance:
            from design_review import validate_review
            submission=json.loads((output/'planning/submission.json').read_text())
            review=validate_review(submission.get('design_review'))
            (output/'design-review.json').write_text(json.dumps(review,indent=2))
        path=output/'design-parameters.json';path.write_text(json.dumps(design,indent=2))
        return path,hashlib.sha256(candidate.read_bytes()).hexdigest()
