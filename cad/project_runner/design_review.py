"""Recorded design rationale; never treated as engineering verification."""
EVIDENCE = {
    'nasa_aerosonde': {
        'url': 'https://esdpubs.nasa.gov/aircraft/Mk_44_Aerosonde',
        'finding': 'NASA describes a rear engine, high wing, twin booms and inverted-V tail. This establishes a precedent, not suitability of our dimensions.'},
    'fva_v_tail': {
        'url': 'https://www.fva.rwth-aachen.de/en/2020/01/29/v-tail/',
        'finding': 'V-tail sizing considers longitudinal and lateral control, combined rudder deflection limits and AVL cases. Its numeric results do not transfer to this aircraft.'},
}
PENDING = ['pitch_yaw_authority', 'ruddervator_mixing_and_travel',
           'propeller_clearance_and_flow', 'attachments_and_strength']
SCHEMA = {'type': 'object', 'additionalProperties': False,
          'required': ['configuration', 'selection_origin', 'reason', 'alternatives',
                       'source_ids', 'unverified_checks'],
          'properties': {
              'configuration': {'type': 'string', 'enum': ['inverted_v']},
              'selection_origin': {'type': 'string', 'enum': ['user_selected']},
              'reason': {'type': 'string', 'minLength': 30, 'maxLength': 2000},
              'alternatives': {'type': 'object', 'additionalProperties': False,
                  'required': ['tapered_h_tail', 'upright_v'],
                  'properties': {key: {'type': 'string', 'minLength': 20, 'maxLength': 1000}
                                 for key in ['tapered_h_tail', 'upright_v']}},
              'source_ids': {'type': 'array', 'uniqueItems': True, 'minItems': 2,
                             'items': {'type': 'string', 'enum': list(EVIDENCE)}},
              'unverified_checks': {'type': 'array', 'uniqueItems': True,
                                    'items': {'type': 'string', 'enum': PENDING}, 'minItems': 4}}}


def validate_review(value):
    if not isinstance(value, dict) or set(value) != set(SCHEMA['required']):
        raise ValueError('Missing or malformed design review')
    if value['configuration'] != 'inverted_v' or value['selection_origin'] != 'user_selected':
        raise ValueError('Design review contradicts the selected architecture')
    reason = value['reason']
    if not isinstance(reason, str) or not 30 <= len(reason.strip()) <= 2000:
        raise ValueError('Design review needs a bounded explanatory rationale')
    alternatives = value['alternatives']
    if not isinstance(alternatives, dict) or set(alternatives) != {'tapered_h_tail', 'upright_v'}:
        raise ValueError('Design review must compare both alternatives')
    if any(not isinstance(text, str) or not 20 <= len(text.strip()) <= 1000 for text in alternatives.values()):
        raise ValueError('Alternative tradeoffs are missing')
    for key, expected in [('source_ids', list(EVIDENCE)), ('unverified_checks', PENDING)]:
        items = value[key]
        if not isinstance(items, list) or any(not isinstance(item, str) for item in items) or sorted(items) != sorted(expected):
            raise ValueError('Review must retain the supplied sources and all unverified checks')
    return dict(value, evidence=EVIDENCE, scope='Recorded model rationale only; prose is not independent proof or an optimization result')
