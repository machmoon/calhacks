"""Verify declared unit graph and actual native normalization, including mixed units."""
import json,re,sys
from pathlib import Path
import FreeCAD as App
import Part
import step_units
ROOT=Path(__file__).resolve().parents[2];OUT=ROOT/'out/step-unit-normalization';OUT.mkdir(parents=True,exist_ok=True)
text="ISO-10303-21;#1=PRODUCT('a;b','it''s #88= text;',(),());#2=(LENGTH_UNIT()NAMED_UNIT(*)SI_UNIT(.MILLI.,.METRE.));#3=GLOBAL_UNIT_ASSIGNED_CONTEXT((#2));"
assert [int(m[1]) for m in step_units._RECORD.finditer(text)]==[1,2,3]
assert step_units.declared_units(text.encode())['source_length_unit']=='mm'
p=OUT/'mm.step';Part.makeBox(1,2,3).exportStep(str(p));mm=p.read_text()
unit_id=next(int(m[1]) for m in step_units._RECORD.finditer(mm) if 'LENGTH_UNIT' in m[2] and 'SI_UNIT' in m[2])
max_id=max(int(m[1]) for m in step_units._RECORD.finditer(mm));a,b,c=max_id+1,max_id+2,max_id+3
converted=re.sub(r'SI_UNIT\(\.MILLI\.,\.METRE\.\)',f"CONVERSION_BASED_UNIT('INCH',#{a})",mm)
# Replace NAMED_UNIT(*) only on the length definition, not angle units.
converted=re.sub(rf'#{unit_id}\s*=.*?;',f"#{unit_id}=(CONVERSION_BASED_UNIT('INCH',#{a})LENGTH_UNIT()NAMED_UNIT(#{c}));",converted,flags=re.S)
extra=f'\n#{a}=LENGTH_MEASURE_WITH_UNIT(LENGTH_MEASURE(25.4),#{b});\n#{b}=(LENGTH_UNIT()NAMED_UNIT(*)SI_UNIT(.MILLI.,.METRE.));\n#{c}=DIMENSIONAL_EXPONENTS(1.,0.,0.,0.,0.,0.,0.);\n'
head,body=converted.split('DATA;',1);body=body.split('ENDSEC;',1)[0]+extra
inch=head+'DATA;'+body+'ENDSEC;\nEND-ISO-10303-21;\n'
(OUT/'inch.step').write_text(inch)
assert step_units.declared_units(inch.encode())['source_length_to_mm']==25.4
shape=Part.read(str(OUT/'inch.step'));bounds=shape.BoundBox
assert max(abs(x-y) for x,y in zip([bounds.XLength,bounds.YLength,bounds.ZLength],[25.4,50.8,76.2]))<1e-6
first_head,first_body=mm.split('DATA;',1);first_body=first_body.split('ENDSEC;',1)[0]
offset=max_id+100
second=re.sub(r'#(\d+)',lambda m:'#'+str(int(m[1])+offset),body)
mixed=first_head+'DATA;'+first_body+second+'ENDSEC;\nEND-ISO-10303-21;\n'
(OUT/'mixed.step').write_text(mixed);audit=step_units.declared_units(mixed.encode());assert audit['source_length_unit']=='mixed' and audit['source_length_to_mm'] is None
shape=Part.read(str(OUT/'mixed.step'));volumes=sorted(s.Volume for s in shape.Solids)
assert len(volumes)==2 and abs(volumes[0]-6)<1e-6 and abs(volumes[1]-6*25.4**3)<1e-4,volumes
additional={}
for label,source,scale in [('foot',inch.replace("'INCH'","'FOOT'").replace('LENGTH_MEASURE(25.4)','LENGTH_MEASURE(304.8)'),304.8),('centimetre',mm.replace('SI_UNIT(.MILLI.,.METRE.)','SI_UNIT(.CENTI.,.METRE.)'),10)]:
 path=OUT/(label+'.step');path.write_text(source)
 assert step_units.declared_units(source.encode())['source_length_to_mm']==scale
 bb=Part.read(str(path)).BoundBox
 assert max(abs(x-y) for x,y in zip([bb.XLength,bb.YLength,bb.ZLength],[scale,2*scale,3*scale]))<1e-6
 additional[label]=[bb.XLength,bb.YLength,bb.ZLength]
rejected=0
for bad in [inch.replace('LENGTH_MEASURE(25.4)','LENGTH_MEASURE(1.0)'),inch.replace(f'LENGTH_MEASURE(25.4),#{b}',f'LENGTH_MEASURE(25.4),#{unit_id}'),inch.replace('DIMENSIONAL_EXPONENTS(1.,0.,0.,0.,0.,0.,0.)','DIMENSIONAL_EXPONENTS(0.,1.,0.,0.,0.,0.,0.)'),inch.replace("'INCH'","'GUESS'"),inch.replace(f'LENGTH_MEASURE(25.4),#{b}','LENGTH_MEASURE(25.4),#999999')]:
 try:step_units.declared_units(bad.encode())
 except ValueError:rejected+=1
assert rejected==5
(OUT/'result.json').write_text(json.dumps(dict(passed=True,inch_bounds_mm=[bounds.XLength,bounds.YLength,bounds.ZLength],mixed_context_volumes_mm3=volumes,mixed_unit_audit=audit,additional_unit_bounds_mm=additional,negative_controls=rejected),indent=2))
print('Native inch normalization, mixed mm/inch contexts and five negative controls passed')
