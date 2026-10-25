"""Actual FreeCAD mesh scale negative, not a fabricated verdict flag."""
import json,sys
from pathlib import Path
import FreeCAD as App,Mesh
sys.path.insert(0,str(Path(__file__).resolve().parent))
from geometry import check_mesh
root=Path(__file__).resolve().parents[3];p=root/'cad/projects/so-arm101/upstream/Simulation/SO101/assets/waveshare_mounting_plate_so101_v2.stl'
scale=App.Matrix();scale.scale(1000,1000,1000)
mesh=Mesh.Mesh(str(p));mesh.transform(scale);good=check_mesh(mesh,p);assert good['passed']
wrong=mesh.copy();wrong.transform(scale);bad=check_mesh(wrong,p);assert not bad['passed'] and bad['unmatched_vertices']>0
out=root/'out/so-arm101-evaluation/mesh-negative-controls.json';out.write_text(json.dumps({'correct_scale':good,'wrong_scale_rejected':bad},indent=2));print('Correct source mesh accepted; actual1000x wrong-scale mesh rejected')
