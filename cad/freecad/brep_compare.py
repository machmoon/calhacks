"""Compare all serialized BREP data, allowing only bounded float roundoff.
This is not a volume-only or sampled-mesh geometry comparison.
"""
import hashlib,math,re
_NUMBER=re.compile(r'([-+]?(?:\d+\.\d*|\.\d+|\d+)(?:[eE][-+]?\d+)?)')
_INTEGER=re.compile(r'[-+]?\d+\Z')


def _without_checked_cache(text):
    # OCCT TopTools_ShapeSet::Write serializes seven flags after each shape.
    # Only flag 3 is Checked: a cached validation state, not geometry/topology.
    # https://github.com/Open-Cascade-SAS/OCCT/blob/V7_8_1/src/TopTools/TopTools_ShapeSet.cxx#L515
    prefix,separator,shapes=text.partition('TShapes ')
    if not separator:return text
    return prefix+separator+re.sub(r'(?m)^([01]{2})[01]([01]{4})$',r'\g<1>0\g<2>',shapes)


def compare(actual,expected,tolerance=1e-10):
    aa=_without_checked_cache(actual);bb=_without_checked_cache(expected)
    a=_NUMBER.split(aa);b=_NUMBER.split(bb)
    result=dict(actual_sha256=hashlib.sha256(actual.encode()).hexdigest(),expected_sha256=hashlib.sha256(expected.encode()).hexdigest(),exact_bytes=actual==expected,absolute_numeric_tolerance=tolerance,structure_equal=False,checked_cache_normalized=(aa!=actual or bb!=expected),numeric_coefficients_checked=0,changed_numeric_tokens=0,max_numeric_delta=0.0,passed=False)
    if len(a)!=len(b) or a[::2]!=b[::2]:return result
    result['structure_equal']=True
    for x,y in zip(a[1::2],b[1::2]):
        result['numeric_coefficients_checked']+=1
        if x==y:continue
        result['changed_numeric_tokens']+=1
        if _INTEGER.fullmatch(x) and _INTEGER.fullmatch(y):return result
        delta=abs(float(x)-float(y));result['max_numeric_delta']=max(result['max_numeric_delta'],delta)
        if not math.isfinite(delta) or delta>tolerance:return result
    result['passed']=True
    return result
