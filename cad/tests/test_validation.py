import math
import unittest
from unittest.mock import patch
from typing import Annotated as A
from wright_cad import Assembly, Loc, Param, part
from wright_cad import library
from wright_cad.validation import pair_result


@part(material="al6061_t6")
def example(size: A[float, Param("mm", "length", min=1)] = 10.,
            count: A[int, Param("count", "copies", min=1)] = 2,
            points: A[list, Param("mm", "coordinates")] = ((0, 0), (1, 1))):
    return None


class ValidationTests(unittest.TestCase):
    def test_numeric_types_rejected_before_kernel(self):
        with patch.object(library, "load", return_value=example):
            for bad in (True, "20", None, [], float("nan"), float("inf")):
                with self.subTest(bad=bad), self.assertRaises(ValueError):
                    library.check_params("test.example", {"size": bad})
            for bad in (1.3, True, "2", float("nan")):
                with self.subTest(bad=bad), self.assertRaises(ValueError):
                    library.check_params("test.example", {"count": bad})
            with self.assertRaises(ValueError):
                library.check_params("test.example", {"points": [[0, float("inf")]]})
            self.assertEqual(library.check_params("test.example", {"count": 3.})["count"], 3)

    def test_part_id_cannot_escape_library(self):
        for bad in ("../x", "a.../x", "a..x", ".foo", "a._hidden", "a.x/y"):
            with self.subTest(bad=bad), self.assertRaises(ValueError):
                library.part_file(bad)

    def test_empty_and_bad_contact_fail(self):
        with self.assertRaises(ValueError):
            Assembly("empty").to_json()
        a = Assembly("bad")
        a.add("box", "common.block", touches=("missing",))
        with self.assertRaises(ValueError):
            a.to_json()

    def test_nonfinite_placement_fails(self):
        a = Assembly("bad")
        a.add("box", "common.block", at=Loc(x=math.nan))
        with self.assertRaises(ValueError):
            a.to_json()

    def test_expected_contact_never_hides_penetration(self):
        self.assertEqual(pair_result("a", "b", 1000, 0, True, 1, .1)[0], "clash")
        self.assertEqual(pair_result("a", "b", 0, 0, True, 1, .1)[0], "contact")
        self.assertEqual(pair_result("a", "b", 0, 2, True, 1, .1)[0], "floating")
        self.assertEqual(pair_result("a", "b", 0, 2, False, 1, .1)[0], "clearance")

    def test_failed_measurements_cannot_pass(self):
        for volume, distance in ((math.nan, 0), (0, math.nan), (-1, 0), (0, math.inf)):
            with self.subTest(volume=volume, distance=distance), self.assertRaises(ValueError):
                pair_result("a", "b", volume, distance, True, 1, .1)


if __name__ == "__main__":
    unittest.main()
