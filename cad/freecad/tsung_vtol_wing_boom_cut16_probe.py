"""Exact native evidence for curved-skin boolean volume-gate failure."""
import json
from pathlib import Path
import FreeCAD as App
ROOT=Path(__file__).resolve().parents[2];out=ROOT/'out/tsung-vtol-wing-boom-assembly'
a=App.openDocument(str(ROOT/'out/tsung-vtol-smooth-aircraft-final/assembly.FCStd'));b=App.openDocument(str(ROOT/'out/tsung-vtol-wing-hardpoint-linked/assembly.FCStd'))
b.SparWeb.Width=16;b.SparWeb.setExpression('Placement.Base.y','HardpointControls.BoomY - 8 mm');b.SparCollarBlank.Height=16;b.SparCollarBlank.setExpression('Placement.Base.y','HardpointControls.BoomY - 8 mm');b.recompute()
skin=a.RightSkin.Shape;tool=b.HPNutSeat1.Shape;result=skin.cut(tool);intersection=skin.common(tool);removed=skin.cut(result);added=result.cut(skin)
r=dict(scope='Recreated16mm collar/web proposal from savedprior native graph against actualsmoothskin; no sourcefilesmodified',base_volume_mm3=skin.Volume,result_volume_mm3=result.Volume,volume_difference_mm3=skin.Volume-result.Volume,intersection_volume_mm3=intersection.Volume,intersection_solids=len(intersection.Solids),removed_volume_mm3=removed.Volume,removed_solids=len(removed.Solids),added_volume_mm3=added.Volume,added_solids=len(added.Solids),result_valid=result.isValid(),result_solids=len(result.Solids))
(out/'skin-cut-16mm-probe.json').write_text(json.dumps(r,indent=2));print(json.dumps(r),flush=True)
