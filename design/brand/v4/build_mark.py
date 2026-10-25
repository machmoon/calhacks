"""Wright: Devin's three-cell arrangement with the existing partner glyphs.
Reference: https://devin.ai/assets/images/new-home/proactive-issues/devin.svg
Run from any directory to rebuild the vector assets.
"""
from pathlib import Path
import contextlib
import io
import runpy

ROOT = Path(__file__).resolve().parent
with contextlib.redirect_stdout(io.StringIO()):
    glyphs = runpy.run_path(str(ROOT.parent / "v3" / "build_mark.py.txt"))
OUTLINE = 'M1 6.04984V2.32199C1 2.15816 1.08901 2.00677 1.2335 1.92485L4.52028 0.0614377C4.66477 -0.0204792 4.84279 -0.0204792 4.98727 0.0614377L8.27406 1.92486C8.41855 2.00677 8.50756 2.15816 8.50756 2.32199V4.22582C8.52197 4.85845 8.86247 5.46863 9.46132 5.80814C10.0602 6.14766 10.7686 6.13213 11.3337 5.82808L13.0127 4.87616C13.1572 4.79425 13.3352 4.79425 13.4797 4.87616L16.7665 6.73958C16.911 6.8215 17 6.97289 17 7.13672V10.8636C17 11.0274 16.911 11.1788 16.7665 11.2607L13.4797 13.1241C13.3352 13.206 13.1572 13.206 13.0127 13.1241L11.3473 12.1799C10.78 11.8688 10.0651 11.8502 9.46148 12.1924C8.86264 12.5319 8.52214 13.1421 8.50772 13.7747V15.678C8.50772 15.8418 8.41871 15.9932 8.27422 16.0751L4.98744 17.9386C4.84295 18.0205 4.66493 18.0205 4.52044 17.9386L1.23366 16.0751C1.08917 15.9932 1.00016 15.8418 1.00016 15.678V11.9512C1.00016 11.7873 1.08917 11.636 1.23366 11.554L4.52044 9.69062C4.66493 9.6087 4.84295 9.6087 4.98743 9.69062L6.67026 10.6447C7.23473 10.9466 7.94111 10.9613 8.53852 10.6226C9.14214 10.2804 9.48316 9.66314 9.49244 9.02512C9.47803 8.39249 9.13753 7.71692 8.53868 7.3774C7.93982 7.03789 7.23144 7.05341 6.6663 7.35747L4.97956 8.31916C4.83444 8.4019 4.65528 8.40183 4.51024 8.31896L1.23216 6.44622C1.08842 6.3641 0.999999 6.21313 1 6.04984Z'

def mark(ink="#000"):
    cuts = "".join(
        f'<g transform="translate({x} {y}) scale(0.118)">{glyphs[name]((0, 0))}</g>'
        for name, x, y in (("creao", 4.754, 4.186), ("claude", 4.754, 13.814),
                           ("azure", 13.246, 9))
    )
    return (f'<defs><mask id="wright" maskUnits="userSpaceOnUse" x="0" y="-1" width="18" height="20">'
            f'<path d="{OUTLINE}" fill="white"/><g fill="black">{cuts}</g></mask></defs>'
            f'<rect x="0" y="-1" width="18" height="20" fill="{ink}" mask="url(#wright)"/>')

for filename, ink in (("mark.svg", "#000"), ("mark-white.svg", "#fff")):
    (ROOT / filename).write_text(
        '<svg xmlns="http://www.w3.org/2000/svg" viewBox="-1 -1 20 20" width="512" height="512">'
        '<title>Wright</title>' + mark(ink) + '</svg>\n')
(ROOT / "app-icon.svg").write_text(
    '<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 1024 1024" width="1024" height="1024">'
    '<title>Wright</title><rect x="100" y="100" width="824" height="824" rx="185" fill="white"/>'
    '<g transform="translate(233 233) scale(31)">' + mark() + '</g></svg>\n')
