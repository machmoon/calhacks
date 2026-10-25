"""Reject import-time execution before handing a model module to the GUI host."""
import ast,sys
from pathlib import Path

def check(path):
 tree=ast.parse(Path(path).read_text());builds=[]
 for node in tree.body:
  if isinstance(node,(ast.Import,ast.ImportFrom)):
   roots=[n.name.split('.')[0] for n in node.names] if isinstance(node,ast.Import) else [(node.module or '').split('.')[0]]
   if any(r not in {'FreeCAD','FreeCADGui','Part','Sketcher','Mesh','math','json','pathlib','hashlib','os','sys','collections','traceback','copy','re','functools','itertools','typing','__future__','datetime','time','struct'} for r in roots):raise ValueError('unapproved import module')
   continue
  if isinstance(node,ast.Expr) and isinstance(node.value,ast.Constant) and isinstance(node.value.value,str):continue
  if isinstance(node,(ast.FunctionDef,ast.AsyncFunctionDef)):
   if node.decorator_list:raise ValueError('function decorators execute at import')
   annotations=[node.returns]+[a.annotation for a in node.args.args+node.args.kwonlyargs]
   if any(isinstance(n,ast.Call) for annotation in annotations if annotation is not None for n in ast.walk(annotation)):raise ValueError('annotation calls execute at import')
   for default in node.args.defaults+node.args.kw_defaults:
    if default is not None:ast.literal_eval(default)
   if node.name=='build':builds.append(node)
   continue
  # Literal constant tables are safe; constructor calls and comprehensions are not.
  if isinstance(node,(ast.Assign,ast.AnnAssign)):
   targets=node.targets if isinstance(node,ast.Assign) else [node.target]
   if any(not isinstance(t,ast.Name) for t in targets):raise ValueError('top-level assignment target must be a plain name')
   ast.literal_eval(node.value);continue
  raise ValueError('module contains executable top-level statement: '+type(node).__name__)
 if len(builds)!=1 or isinstance(builds[0],ast.AsyncFunctionDef):raise ValueError('one synchronous build function required')
 args=builds[0].args
 if [a.arg for a in args.args]!=['input_path','output_dir','checkpoint'] or len(args.defaults)!=1 or not isinstance(args.defaults[0],ast.Constant) or args.defaults[0].value is not None:raise ValueError('build signature must be build(input_path,output_dir,checkpoint=None)')
 return True
if __name__=='__main__':check(sys.argv[1]);print('Static module boundary passed; geometry and runtime safety not evaluated')
