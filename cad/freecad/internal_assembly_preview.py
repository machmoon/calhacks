"""Depth-buffered cutaway from fixture B-rep triangulations; requires numpy/matplotlib."""
import json,sys
from pathlib import Path
import matplotlib
matplotlib.use('Agg')
import matplotlib.pyplot as plt
ROOT=Path(__file__).resolve().parents[2]
sys.path.insert(0,str(ROOT/'evals/cad'))
from raster import render
out=Path(sys.argv[1]) if len(sys.argv)>1 else ROOT/'out/freecad-internal-assembly'
mesh=json.loads((out/'cutaway-mesh.json').read_text())
fig,ax=plt.subplots(figsize=(12,7),dpi=130)
ax.imshow(render(mesh['faces'],mesh['colors'],1200,620));ax.axis('off')
fig.suptitle('Native internal assembly | two-deck instrument enclosure',fontsize=17)
ax.set_title('28 solids · 378 pair checks · 1 contact group · 0 clashes',fontsize=12)
fig.text(.5,.035,'B-rep cutaway: front housing and front half of lid removed for visibility\nProvisional board/component envelopes; threads, retention, cables and thermal/load checks incomplete',ha='center',fontsize=10)
fig.savefig(out/'cutaway.png',bbox_inches='tight',facecolor='#f3f1ec')
