"""Explicit integer budgets; callers never silently increase default limits."""
def integer(value,label,low,high):
    if type(value) is not int or not low<=value<=high:raise ValueError(f'{label} must be an integer in {low}..{high}')
    return value
