"""Render actual pod half-cut geometry, with full-aircraft fit counts from result."""
import json,sys
from pathlib import Path
import matplotlib
matplotlib.use('Agg')
import matplotlib.pyplot as plt
ROOT=Path(__file__).resolve().parents[2]
sys.path.insert(0,str(ROOT/'evals/cad'))
from raster import render
out=ROOT/'out/freecad-aircraft-pod'
mesh=json.loads((out/'cutaway-mesh.json').read_text());result=json.loads((out/'result.json').read_text())
fig,ax=plt.subplots(figsize=(13,7),dpi=130)
ax.imshow(render(mesh['faces'],mesh['colors'],1300,620));ax.axis('off')
fig.suptitle('Actual aircraft pod | hollow shell, root seats and underside access',fontsize=16)
fit=result['fit']
ax.set_title(f"Full aircraft: {result['physical_parts']} solids · {fit['pairs_total']} pairs · {fit['overlap_count']} clashes remain · {fit['unmet_contact_count']} missing contacts",fontsize=12)
fig.text(.5,.035,'Native B-rep cutaway at y = 0; wing skins hidden to reveal root rib and shell cavity\nGold: removable access patch. Zero-clearance interfaces; retention, reinforcement and fabrication remain unverified.',ha='center',fontsize=10)
fig.savefig(out/'cutaway.png',bbox_inches='tight',facecolor='#f3f1ec')
