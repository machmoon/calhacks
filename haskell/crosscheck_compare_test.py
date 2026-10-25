"""Review 4, finding 11: the comparison passed when nothing was compared.
Run: python3 -I haskell/crosscheck_compare_test.py"""
import io, json, os, sys, tempfile, unittest
from contextlib import redirect_stdout

sys.path.insert(0, os.path.dirname(os.path.abspath(__file__)))
from crosscheck_compare import main  # noqa: E402

CASES = [{"fn": "us76", "z_m": 11000}, {"fn": "us76", "z_m": 20000}]
GOOD = [{"T_K": 216.65, "p_Pa": 22632.0}, {"T_K": 216.65, "p_Pa": 5474.9}]


def run(cases, oracle, rust):
    d = tempfile.mkdtemp()
    paths = []
    for n, v in (("c", cases), ("h", oracle), ("r", rust)):
        p = os.path.join(d, n + ".json")
        with open(p, "w") as f:
            json.dump(v, f)
        paths.append(p)
    with redirect_stdout(io.StringIO()):
        return main(paths)


class Compare(unittest.TestCase):
    def test_agreement_passes(self):
        self.assertEqual(run(CASES, GOOD, GOOD), 0)

    def test_every_way_of_comparing_nothing_fails(self):
        self.assertEqual(run(CASES, GOOD, [{"T_K": None, "p_Pa": None}, GOOD[1]]), 1)  # null
        self.assertEqual(run(CASES, GOOD, [{"error": "feature off"}, GOOD[1]]), 1)  # error / skipped
        self.assertEqual(run(CASES, GOOD, GOOD[:1]), 1)  # length mismatch
        self.assertEqual(run(CASES, GOOD, [{"T_K": 216.65}, GOOD[1]]), 1)  # output missing
        self.assertEqual(run([], [], []), 1)  # nothing compared

    def test_nan_and_excess_fail(self):
        self.assertEqual(run(CASES, GOOD, [{"T_K": float("nan"), "p_Pa": 22632.0}, GOOD[1]]), 1)
        self.assertEqual(run(CASES, GOOD, [{"T_K": 230.0, "p_Pa": 22632.0}, GOOD[1]]), 1)


if __name__ == "__main__":
    unittest.main()
