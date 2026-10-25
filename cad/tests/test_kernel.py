"""Real B-rep tests. Run with the existing build123d/OCP Python environment."""
import tempfile
import unittest
from pathlib import Path
from wright_cad import Assembly, Loc
from wright_cad import kernel


class KernelTests(unittest.TestCase):
    def test_contact_overlap_clearance_and_step_roundtrip(self):
        from build123d import import_step
        with tempfile.TemporaryDirectory() as tmp:
            result = kernel.build("common.block", {"x": 10, "y": 10, "z": 10}, "box", tmp)
            self.assertAlmostEqual(result["volume_mm3"], 1000, places=5)
            for separation, expected in ((10, True), (5, False), (12, False)):
                assembly = Assembly("contact_test")
                assembly.add("first", "common.block")
                assembly.add("second", "common.block", at=Loc(x=separation), touches=("first",))
                instances = assembly.to_json()["instances"]
                for inst in instances:
                    inst.update(key="box", cache=tmp)
                out = Path(tmp) / str(separation)
                report = kernel.assemble("contact_test", instances, str(out))
                self.assertEqual(report["passed"], expected)
                self.assertEqual(report["pairs_total"], 1)
                self.assertEqual(len(report["clashes"]), int(separation == 5))
                self.assertEqual(len(report["floating"]), int(separation == 12))
                self.assertTrue(import_step(out / "assembly.step").is_valid)
                self.assertTrue((out / "assembly.glb").stat().st_size > 0)

    def test_cache_reuse_and_parameter_invalidation(self):
        from wright_cad.build import build_plan
        with tempfile.TemporaryDirectory() as tmp:
            a = Assembly("cache_test")
            a.add("first", "common.block", x=10, y=10, z=10)
            a.add("second", "common.block", at=Loc(x=20), x=10, y=10, z=10)
            plan = a.to_json()
            first = build_plan(plan, tmp)
            self.assertEqual(first["cache"], {"hits": 1, "misses": 1})
            second = build_plan(plan, tmp)
            self.assertEqual(second["cache"], {"hits": 2, "misses": 0})
            plan["instances"][1]["params"]["x"] = 12
            changed = build_plan(plan, tmp)
            self.assertEqual(changed["cache"], {"hits": 1, "misses": 1})
            self.assertGreater(changed["mass_kg"], first["mass_kg"])

    def test_hollow_loft_and_diagonal_tube(self):
        import math
        with tempfile.TemporaryDirectory() as tmp:
            def rings(a, b):
                return [[[a,a,z],[b,a,z],[b,b,z],[a,b,z]] for z in (0,20)]
            shell = kernel.build("common.loft", {"rings":rings(0,10), "inner":rings(1,9)}, "shell", tmp)
            self.assertAlmostEqual(shell["volume_mm3"], (100-64)*20, places=5)
            tube = kernel.build("common.axial_tube", {"start":[0,0,0],"end":[30,40,0],
                                "radius":5,"bore_radius":4}, "tube", tmp)
            self.assertAlmostEqual(tube["volume_mm3"], math.pi*9*50, places=5)
            for actual, expected in zip(tube["cg_mm"], (15,20,0)):
                self.assertAlmostEqual(actual, expected, places=6)

    def test_tabbed_fin_volume_and_invalid_tab(self):
        with tempfile.TemporaryDirectory() as tmp:
            params = dict(root=150,tip=90,span=75,sweep=44,thickness=2.4,
                          tab_length=100,tab_height=8,tab_offset=25)
            fin = kernel.build("airframe.trapezoid_fin",params,"fin",tmp)
            self.assertAlmostEqual(fin["volume_mm3"],((150+90)*75/2+100*8)*2.4,places=5)
            params["tab_offset"] = 60
            with self.assertRaises(ValueError):
                kernel.build("airframe.trapezoid_fin",params,"bad_fin",tmp)

    def test_rib_rejects_profile_crossing_and_bore_breakout(self):
        import json
        profile = json.loads((Path(__file__).resolve().parents[2]/"data/air/airfoils.json").read_text())["airfoils"]["sd7062"]["coordinates"]
        with tempfile.TemporaryDirectory() as tmp:
            with self.assertRaisesRegex(ValueError,"profile"):
                kernel.build("aircraft.rib",{"profile":profile,"bores":[]},"bad_profile",tmp)
            repaired = [list(p) for p in profile]
            repaired[0] = repaired[-1] = [(a+b)/2 for a,b in zip(profile[0],profile[-1])]
            rib = kernel.build("aircraft.rib",{"profile":repaired,"bores":[]},"closed",tmp)
            self.assertTrue(rib["valid"])
            with self.assertRaisesRegex(ValueError,"bore"):
                kernel.build("aircraft.rib",{"profile":repaired,"bores":[[50,0,100]]},"bad_bore",tmp)

    def test_mount_seat_is_material_removed_not_collision_exemption(self):
        import math
        with tempfile.TemporaryDirectory() as tmp:
            mount = kernel.build("common.mount_block", {}, "mount", tmp)
            self.assertAlmostEqual(mount["volume_mm3"], 40*20*20-math.pi*6.35**2*40, places=5)
            self.assertIn("seat_0", mount["joints"])
            with self.assertRaisesRegex(ValueError,"does not intersect"):
                kernel.build("common.mount_block", {"bores":[[[0,100,100],[40,100,100],2]]}, "miss", tmp)

    def test_loft_reduces_only_redundant_sections(self):
        from build123d import Solid, Wire
        from wright_cad.library import load
        def ring(z, size):
            return [[0,0,z],[size,0,z],[size,size,z],[0,size,z]]
        straight = [ring(z,10) for z in (0,5,10,15,20)]
        shape = load("common.loft")(rings=straight)
        self.assertEqual(len(shape.faces()),6)
        self.assertAlmostEqual(shape.volume,2000,places=5)
        kinked = [ring(0,10),ring(10,20),ring(20,10)]
        kink = load("common.loft")(rings=kinked)
        self.assertGreater(len(kink.faces()),6)
        self.assertAlmostEqual(kink.volume,2*10*(100+200+400)/3,places=5)

    def test_invalid_density_refused(self):
        with tempfile.TemporaryDirectory() as tmp:
            for density in (-1, 0, float("nan")):
                with self.subTest(density=density), self.assertRaises(ValueError):
                    kernel.build("common.block", {}, "bad", tmp, density=density)


if __name__ == "__main__":
    unittest.main()
