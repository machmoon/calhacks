import copy
import unittest
from design_review import validate_review, PENDING


class ReviewContract(unittest.TestCase):
    def test_selected_architecture_and_uncertainty_survive(self):
        good = dict(configuration='inverted_v', selection_origin='user_selected',
                    reason='The selected two-surface layout fits the twin-boom arrangement as a provisional design.',
                    alternatives={'tapered_h_tail': 'Separate pitch and yaw surfaces retain the previous architecture.',
                                  'upright_v': 'A central support would require a different attachment arrangement.'},
                    source_ids=['nasa_aerosonde', 'fva_v_tail'], unverified_checks=PENDING[:])
        self.assertIn('not independent proof', validate_review(good)['scope'])
        for key, replacement in [('configuration', 'arched_h_tail'),
                                 ('selection_origin', 'aerodynamically_optimized'),
                                 ('unverified_checks', []), ('source_ids', ['invented']),
                                 ('alternatives', {}), ('reason', 'looks good')]:
            bad = copy.deepcopy(good)
            bad[key] = replacement
            with self.assertRaises(ValueError):
                validate_review(bad)


if __name__ == '__main__':
    unittest.main()
