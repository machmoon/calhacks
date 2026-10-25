import copy
import unittest
from pdf_context import validate_report


class PdfInterpretation(unittest.TestCase):
    def test_unsupported_and_untraceable_requirements_fail_closed(self):
        report = {'supported': True, 'summary': 'A compatible VTOL specification.',
                  'unsupported_requirements': [], 'parameter_sources': {
                      'Span': {'origin': 'pdf_text', 'page': 1,
                               'evidence': 'Wingspan: 1900 mm'}}}
        self.assertEqual(validate_report(report, ['Span'], 1), report)
        for patch in [{'supported': False}, {'unsupported_requirements': ['Requires a jet engine']},
                      {'parameter_sources': {}}, {'summary': ''}]:
            invalid = dict(report, **patch)
            with self.assertRaises(ValueError):
                validate_report(invalid, ['Span'], 1)
        for source in [{'origin': 'pdf_visual', 'page': 2, 'evidence': 'Dimension in drawing'},
                       {'origin': 'recipe_default', 'page': 1, 'evidence': 'Invented page citation'},
                       {'origin': 'pdf_text', 'page': True, 'evidence': 'Invalid boolean page'}]:
            invalid = copy.deepcopy(report)
            invalid['parameter_sources']['Span'] = source
            with self.assertRaises(ValueError):
                validate_report(invalid, ['Span'], 1)


if __name__ == '__main__':
    unittest.main()
