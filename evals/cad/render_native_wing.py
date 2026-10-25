"""Render the explicit native wing cutaway triangle export."""
import json,sys
from pathlib import Path
import matplotlib
matplotlib.use('Agg')
import matplotlib.pyplot as plt
from raster import render
root=Path(sys.argv[1]).resolve()
m=json.loads((root/'preview-mesh.json').read_text())
r=json.loads((root/'result.json').read_text())
fig=plt.figure(figsize=(12,7),facecolor='#f3f1ec');ax=fig.add_axes([.03,.18,.94,.64])
ax.imshow(render(m['faces'],m['colors']));ax.set_axis_off()
fig.text(.05,.93,'Native wing · hollow skin and linked ribs',fontsize=22,weight='bold',color='#263245')
fig.text(.05,.875,f"600 mm span · {len(r['physical_parts'])} solids · {sum(p['clash'] for p in r['fit']['pairs'])} overlaps · {sum(p['floating'] for p in r['fit']['pairs'])} unresolved contact",fontsize=13,color='#536173')
fig.text(.05,.10,'Cutaway view: upper skin removed for visibility only. Native model retains the full skin.',fontsize=11,color='#536173')
fig.text(.05,.05,'Provisional rib layout. Source joiner clearance is 0.105 mm; joint and load validation remain open.',fontsize=11,color='#536173')
fig.savefig(root/'preview.png',dpi=160,facecolor=fig.get_facecolor());plt.close(fig)
