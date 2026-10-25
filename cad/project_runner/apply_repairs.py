"""Apply validated data through fixed FreeCAD operations to a separate candidate."""
import argparse
import hashlib
import importlib.util
import json
import sys
from pathlib import Path
import FreeCAD as App
import booleans
import cut_proofs
import tube_envelope
from repair import parse_repairs


def sha(path):return hashlib.sha256(Path(path).read_bytes()).hexdigest()


def main():
    parser=argparse.ArgumentParser();parser.add_argument('proposal');parser.add_argument('output');a=parser.parse_args()
    proposal=json.loads(Path(a.proposal).read_text())
    manifest=json.loads(Path(proposal['manifest']).read_text())
    pairs=json.loads(Path(proposal['pairs']).read_text())
    source=Path(manifest['native']);source_sha=sha(source)
    if source_sha!=proposal['native_sha256'] or source_sha!=pairs['native_sha256']:
        raise ValueError('Repair evidence must identify the unchanged source artifact')
    repairs=parse_repairs('REPAIRS = '+repr(proposal['repairs']),manifest['physical'],
                          [row for row in pairs['check']['pairs'] if row['clash']])
    output=Path(a.output).resolve();output.mkdir(parents=True,exist_ok=False)
    report={'passed':False,'source_sha256':source_sha,'repairs':[],'scope':'Nominal seat cuts only; no strength, motion, clearance or installation qualification'}
    try:
        module=Path(manifest['proxy_module'])
        declared={value for path,value in manifest['generator_sources'].items() if Path(path).name==module.name}
        if declared!={sha(module)}:raise ValueError('Feature dependency changed or ambiguous')
        spec=importlib.util.spec_from_file_location(module.stem,module);loaded=importlib.util.module_from_spec(spec)
        sys.modules[module.stem]=loaded;spec.loader.exec_module(loaded)
        doc=App.openDocument(str(source));doc.recompute()
        names={name:name for name in manifest['collision_objects']}
        initial_volumes={name:doc.getObject(name).Shape.Volume for name in manifest['physical']}
        for index,row in enumerate(repairs):
            target,tool=doc.getObject(names[row['target']]),doc.getObject(names[row['tool']])
            if target.Shape.common(tool.Shape).Volume<=1e-6:
                report['repairs'].append(dict(row,status='already_resolved_by_prior_cut'))
                continue
            if row.get('tool_form','actual')=='outer_envelope':
                tool=tube_envelope.make_outer(doc,tool,f'AIEnvelope{index}_')
            # Fixed native API, never eval/exec of the model's submitted text.
            feature=doc.addObject('Part::Cut',f'AIRepair{index}_{row["target"]}')
            feature.Base=target;feature.Tool=tool
            feature.addProperty('App::PropertyBool','WrightTypedBoolean');feature.WrightTypedBoolean=True
            feature.addProperty('App::PropertyBool','WrightRetainToolAsPart');feature.WrightRetainToolAsPart=row.get('tool_form','actual')=='actual'
            feature.addProperty('App::PropertyString','OriginalPartName');feature.OriginalPartName=row['target']
            doc.recompute();booleans.validate(feature);cut_proofs.record_verified(feature)
            original=doc.getObject(row['target']).Shape
            removed=original.cut(feature.Shape).Volume
            fraction=removed/initial_volumes[row['target']]
            if not 0<fraction<=.1:raise ValueError('Repair exceeds the cumulative 10% material-removal limit')
            names[row['target']]=feature.Name
            report['repairs'].append(dict(row,result=feature.Name,native_tool=tool.Name,removed_mm3=removed,removed_fraction=fraction))
        manifest['source_name_map']=names
        manifest['physical']=[names[name] for name in manifest['physical']]
        manifest['collision_objects']=[names[name] for name in manifest['collision_objects']]
        manifest['contact_pairs']=[[names.get(name,name) for name in pair] for pair in manifest['contact_pairs']]
        if len(manifest['physical'])!=len(set(manifest['physical'])):raise ValueError('Part identity was lost')
        for name in manifest['physical']:
            shape=doc.getObject(name).Shape
            if not shape.isValid() or len(shape.Solids)!=1:raise ValueError('Repair left an invalid or disconnected manufactured part')
        native=output/'assembly.FCStd';doc.saveAs(str(native));App.closeDocument(doc.Name)
        manifest.update(native=str(native),native_sha256=sha(native),design_revision=manifest['design_revision']+'-astra-seat-repair',
                        repair_proposal=str(Path(a.proposal).resolve()),parent_native_sha256=source_sha,status='candidate_requires_independent_audit')
        manifest.pop('open_macro',None)
        manifest.pop('step',None);manifest.pop('renders',None);manifest.pop('images',None);manifest.pop('diagnostic_views',None)
        manifest['generator_sources'][str(Path(__file__).resolve())]=sha(__file__)
        (output/'artifact-manifest.json').write_text(json.dumps(manifest,indent=2))
        report.update(passed=True,native=str(native),native_sha256=sha(native))
    except Exception as error:
        report.update(error=str(error));raise
    finally:
        report['source_unchanged']=sha(source)==source_sha
        (output/'repair-result.json').write_text(json.dumps(report,indent=2))


if __name__=='__main__':main()
