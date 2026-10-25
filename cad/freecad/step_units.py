"""Audit STEP length-unit references before Open CASCADE normalization.

Deliberately supports SI metre/mm/cm and graph-defined inch/foot only.
Conversion labels alone never establish a scale. No caller unit override.
"""
import math
import re

_RECORD=re.compile(r"#(\d+)\s*=\s*([^;']*(?:'[^']*(?:''[^']*)*'[^;']*)*);",re.S)

def declared_units(data):
    text=data.decode('latin1')
    if not text.lstrip().startswith('ISO-10303-21;'):raise ValueError('expected STEP physical file')
    records={};contexts=[]
    for match in _RECORD.finditer(text):
        record=match[2].upper()
        if any(k in record for k in ('LENGTH_UNIT','LENGTH_MEASURE_WITH_UNIT','DIMENSIONAL_EXPONENTS','GLOBAL_UNIT_ASSIGNED_CONTEXT')):
            key=int(match[1])
            if key in records:raise ValueError('duplicate STEP unit/context record')
            records[key]=record
        context=re.search(r'GLOBAL_UNIT_ASSIGNED_CONTEXT\s*\(\s*\(([^)]*)\)\s*\)',record)
        if context:contexts.append((int(match[1]),[int(x) for x in re.findall(r'#(\d+)',context[1])]))
    length={k:v for k,v in records.items() if re.search(r'\bLENGTH_UNIT\s*\(',v)}
    resolved={}
    def unit(key,seen=()):
        if key in seen or len(seen)>8:raise ValueError('cyclic/excessive STEP unit graph')
        if key in resolved:return resolved[key]
        record=length.get(key)
        if record is None:raise ValueError('unresolved STEP length unit')
        si=re.search(r'SI_UNIT\s*\(\s*(\$|\.[A-Z]+\.)\s*,\s*\.METRE\.\s*\)',record)
        if si:
            value={'$':('m',1000.),'.MILLI.':('mm',1.),'.CENTI.':('cm',10.)}.get(si[1])
            if value is None:raise ValueError('unsupported STEP SI length prefix')
        else:
            conversion=re.search(r"CONVERSION_BASED_UNIT\s*\(\s*'(INCH|FOOT)'\s*,\s*#(\d+)\s*\)",record)
            dimension=re.search(r'NAMED_UNIT\s*\(\s*#(\d+)\s*\)',record)
            if not conversion or not dimension:raise ValueError('unsupported conversion-based STEP length unit')
            exp=re.fullmatch(r'\s*DIMENSIONAL_EXPONENTS\s*\(([^)]*)\)\s*',records.get(int(dimension[1]),''))
            try:dimensions=[float(x) for x in exp[1].split(',')] if exp else []
            except ValueError:dimensions=[]
            if dimensions!=[1,0,0,0,0,0,0]:raise ValueError('invalid length dimension exponents')
            measure=re.fullmatch(r'\s*LENGTH_MEASURE_WITH_UNIT\s*\(\s*LENGTH_MEASURE\s*\(([^)]*)\)\s*,\s*#(\d+)\s*\)\s*',records.get(int(conversion[2]),''))
            if not measure:raise ValueError('missing STEP length conversion factor')
            try:factor=float(measure[1].replace('D','E'))
            except ValueError:raise ValueError('invalid STEP conversion number')
            scale=factor*unit(int(measure[2]),seen+(key,))[1]
            expected={'INCH':25.4,'FOOT':304.8}[conversion[1]]
            if not math.isfinite(scale) or not math.isclose(scale,expected,rel_tol=1e-12,abs_tol=1e-12):raise ValueError('STEP conversion factor disagrees with declared inch/foot unit')
            value=({'INCH':'in','FOOT':'ft'}[conversion[1]],scale)
        resolved[key]=value;return value
    for key in length:unit(key)
    if not resolved or not contexts:raise ValueError('missing STEP length units or unit contexts')
    audit=[]
    for key,refs in contexts:
        lengths=[r for r in refs if r in length]
        if len(lengths)!=1:raise ValueError('STEP context must assign exactly one audited length unit')
        label,scale=unit(lengths[0]);audit.append(dict(context_id=key,length_unit_id=lengths[0],unit=label,to_mm=scale))
    distinct=sorted(set((a['unit'],a['to_mm']) for a in audit))
    return dict(source_length_unit=distinct[0][0] if len(distinct)==1 else 'mixed',
        source_length_to_mm=distinct[0][1] if len(distinct)==1 else None,
        native_length_unit='mm',normalization='Open CASCADE STEP reader; audited per-context units; no caller scale or unit override',
        source_unit_contexts=audit)
