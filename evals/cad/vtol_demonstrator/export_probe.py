import os,faulthandler
if os.environ.get("WRIGHT_CAD_DIAGNOSTICS")=="1":
 faulthandler.enable();faulthandler.dump_traceback_later(60,repeat=True)
"""Independent solid-by-solid STEP comparison; omitted COTS stay omitted."""
import argparse
import hashlib
import importlib.util
import json
import sys
from pathlib import Path
import FreeCAD as App
import Part
from assembly_frames import world_shape


def sha(path):
    return hashlib.sha256(Path(path).read_bytes()).hexdigest()


def compare(native, manifest, output):
    native, manifest, output = map(Path, (native, manifest, output))
    spec = json.loads(manifest.read_text())
    result = {'native_sha256': sha(native), 'manifest_sha256': sha(manifest),
              'status': 'running', 'passed': False,
              'scope': 'Only declared physical airframe STEP coverage; candidate COTS and reservations are omitted.'}
    try:
        if spec['native_sha256'] != result['native_sha256']:
            raise ValueError('Native artifact hash mismatch')
        module = Path(spec['proxy_module'])
        expected = {value for path,value in spec['generator_sources'].items() if Path(path).name==module.name}
        if expected != {sha(module)}:
            raise ValueError('Native feature dependency is not pinned to this manifest')
        module_spec = importlib.util.spec_from_file_location(module.stem, module)
        loaded = importlib.util.module_from_spec(module_spec)
        sys.modules[module.stem] = loaded
        module_spec.loader.exec_module(loaded)
        doc = App.openDocument(str(native.resolve()))
        doc.recompute()
        source = [(name, index, solid) for name in spec['physical']
                  for index, solid in enumerate(world_shape(doc.getObject(name)).Solids)]
        step = Path(spec['step']['path'])
        imported = Part.read(str(step))
        result.update(step_sha256=sha(step), source_solid_count=len(source),
                      export_solid_count=len(imported.Solids), rows=[])
        remaining = dict(enumerate(imported.Solids))
        export_bounds = {i:s.optimalBoundingBox(False, False) for i,s in remaining.items()}
        export_valid = {i:s.isValid() for i,s in remaining.items()}
        for name, index, solid in source:
            # Raw BoundBox can contain an untrimmed supporting surface. Use
            # trimmed native bounds, then actual Boolean differences, rather
            # than subtracting potentially different volume quadratures.
            b = solid.optimalBoundingBox(False, False)
            def near(i):
                c = export_bounds[i]
                return all(abs(getattr(b,k)-getattr(c,k)) < 1e-4
                           for k in ('XMin','XMax','YMin','YMax','ZMin','ZMax'))
            candidates = [i for i, s in remaining.items() if near(i) and export_valid[i]]
            match = None
            for i in candidates:
                difference = solid.cut(remaining[i]).Volume + remaining[i].cut(solid).Volume
                if difference <= max(1e-4,solid.Volume*1e-6):
                    match = (i,difference)
                    break
            row = {'object': name, 'solid_index': index, 'passed': match is not None}
            if match:
                row.update(export_index=match[0], symmetric_difference_mm3=match[1])
                del remaining[match[0]]
            else:
                def error(i):
                    c=export_bounds[i]
                    return max(abs(getattr(b,k)-getattr(c,k)) for k in ('XMin','XMax','YMin','YMax','ZMin','ZMax'))
                closest=sorted(remaining, key=error)[:2]
                row['nearest_candidates']=[{'export_index':i,'max_bounds_error_mm':error(i),
                    'volume_error_mm3':remaining[i].Volume-solid.Volume} for i in closest]
            result['rows'].append(row)
            output.write_text(json.dumps(result, indent=2))
        result.update(status='complete', unmatched_export_indices=list(remaining),
                      passed=bool(source) and not remaining and all(r['passed'] for r in result['rows']))
        App.closeDocument(doc.Name)
    except Exception as error:
        result.update(status='failed', error=str(error), passed=False)
    result['source_unchanged'] = sha(native) == result['native_sha256']
    result['passed'] = result['passed'] and result['source_unchanged']
    output.write_text(json.dumps(result, indent=2))
    print(json.dumps({k:result.get(k) for k in ('status','passed','error','source_solid_count','export_solid_count')}))


if __name__ == '__main__':
    parser = argparse.ArgumentParser()
    parser.add_argument('native'); parser.add_argument('manifest'); parser.add_argument('output')
    args = parser.parse_args()
    compare(args.native,args.manifest,args.output)
