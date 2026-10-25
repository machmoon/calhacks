-- | Rocket aerodynamics, recovery and closure relations behind wright-rocket's
-- checks (crates/wright-rocket/src/aero.rs, domain.rs, vehicle.rs),
-- restated from the primary sources with nothing taken from the Rust:
--
--   von Karman nose    r(x) = R sqrt ((theta - sin 2theta / 2) / pi),
--                      theta = acos (1 - 2 x / L): the Haack series with
--                      C = 0, the minimum-drag body of given length and
--                      diameter (Haack 1941; Crowell, The Descriptive
--                      Geometry of Nose Cones, 1996, "LD-Haack")
--   stagnation CD      0.85 x q, q = 1 + M^2/4 + M^4/40 (M <= 1) or
--                      1.84 - 0.76/M^2 + 0.166/M^4 + 0.035/M^6 (M > 1):
--                      the pitot (stagnation over free-stream dynamic)
--                      pressure, isentropic series below M 1 and the
--                      Rayleigh pitot fit above, times a 0.85 flat-face
--                      factor (OpenRocket, core/src/net/sf/openrocket/
--                      aerodynamics/BarrowmanCalculator.java
--                      `calculateStagnationCD`; Hoerner, Fluid-Dynamic Drag,
--                      3-12 for the series)
--   nose pressure drag the NASA TR-R-100 free-flight curve for a fineness-3
--                      von Karman nose, linear in M between the published
--                      points, scaled to fineness f as
--                      stag (cd3 / stag)^(ln (f + 1) / ln 4), so f = 3
--                      returns the data (OpenRocket SymmetricComponentCalc
--                      .java `calculateNoseInterpolator`, "Extrapolate for
--                      fineness ratio"; the table as OpenRocket's
--                      `vonKarmanInterpolator` has it)
--   chute descent      v = sqrt (2 m g / (rho Cd A)), A = pi D^2 / 4: drag
--                      equals weight at terminal velocity (Knacke, Parachute
--                      Recovery Systems Design Manual, eq. 5-1)
--   closure screws     per-screw ultimate capacity by mode: screw shear
--                      0.6 F_tu A_root (shear/tensile 0.6 for alloy steel,
--                      NASA-STD-5020 / Shigley), wall bearing d t F_bru,
--                      tear-out 2 e t F_su with e = 2 d (Bruhn, Analysis and
--                      Design of Flight Vehicle Structures, D1.6); count =
--                      the fewest even >= 6 that hold 1.5 x MEOP on the
--                      bore area in bearing at F_bry
--
-- Constants for the closure (the design's, listed in wright-rocket rules.rs
-- with their sources; restated here so the row is a transcription check):
-- 1/4-20 UNC root area 0.0269 in^2 (Machinery's Handbook), A574 alloy
-- steel F_tu 180 ksi, 6061-T6 F_bru 607 MPa, F_bry 386 MPa, F_su 207 MPa
-- (MMPDS e/D = 2).
module Oracle.Aero
  ( vonKarmanRadius
  , stagnationCd
  , trR100Fr3
  , noseCdFr3
  , noseCd
  , descentRate
  , Closure (..)
  , closureCapacity
  , closureScrews
  ) where

-- | Radius at x along a von Karman nose of length l and base radius r.
vonKarmanRadius :: Double -> Double -> Double -> Double
vonKarmanRadius x l r = r * sqrt ((th - sin (2 * th) / 2) / pi)
  where th = acos (max (-1) (min 1 (1 - 2 * x / l)))

-- | Stagnation-pressure drag coefficient of a blunt face at Mach m.
stagnationCd :: Double -> Double
stagnationCd m = 0.85 * q
  where
    q | m <= 1 = 1 + m * m / 4 + m ** 4 / 40
      | otherwise = 1.84 - 0.76 / (m * m) + 0.166 / m ** 4 + 0.035 / m ** 6

-- | NASA TR-R-100 (NTRS 19630004995) pressure drag of a fineness-3 von
-- Karman nose, (Mach, CD) on the body reference area.
trR100Fr3 :: [(Double, Double)]
trR100Fr3 =
  [ (0.9, 0), (0.95, 0.010), (1.0, 0.027), (1.05, 0.055), (1.1, 0.070)
  , (1.2, 0.081), (1.4, 0.095), (1.6, 0.097), (2.0, 0.091), (3.0, 0.083) ]

-- | The fineness-3 curve, flat beyond its ends, linear between points.
noseCdFr3 :: Double -> Double
noseCdFr3 m = go trR100Fr3
  where
    go ((m0, c0) : rest@((m1, c1) : _))
      | m <= m0 = c0
      | m <= m1 = c0 + (m - m0) / (m1 - m0) * (c1 - c0)
      | otherwise = go rest
    go [(_, c)] = c
    go [] = 0

-- | Nose pressure drag at Mach m for a nose of fineness f.
noseCd :: Double -> Double -> Double
noseCd m f
  | cd3 <= 0 = 0
  | otherwise = stag * (cd3 / stag) ** (log (f + 1) / log 4)
  where
    cd3 = noseCdFr3 m
    stag = stagnationCd m

-- | Terminal descent rate, m/s: mass (kg), g, chute diameter (m), Cd, rho.
descentRate :: Double -> Double -> Double -> Double -> Double -> Double
descentRate mass g d cd rho = sqrt (2 * mass * g / (rho * cd * (pi * d * d / 4)))

-- | Per-screw ultimate capacity, N, by failure mode.
data Closure = Closure
  { screwShear  :: !Double
  , wallBearing :: !Double
  , wallTearout :: !Double
  } deriving (Show)

screwD, screwRootArea, a574TensilePa, al6061BearingUltPa, al6061BearingYieldPa, al6061ShearUltPa :: Double
screwD = 0.25 * 0.0254
screwRootArea = 0.0269 * 0.0254 * 0.0254
a574TensilePa = 180 * 6.894757e6
al6061BearingUltPa = 607e6
al6061BearingYieldPa = 386e6
al6061ShearUltPa = 207e6

-- | Capacity of one 1/4-20 screw through a 6061-T6 wall of this thickness (m).
closureCapacity :: Double -> Closure
closureCapacity wall = Closure
  { screwShear = 0.6 * a574TensilePa * screwRootArea
  , wallBearing = screwD * wall * al6061BearingUltPa
  , wallTearout = 2 * 2 * screwD * wall * al6061ShearUltPa
  }

-- | Screws per bulkhead: the fewest even count, at least 6, whose wall
-- bearing at 1.5 x MEOP on the bore stays under the bearing yield.
closureScrews :: Double -> Double -> Double -> Int
closureScrews innerD wall meop = n + n `mod` 2
  where
    f = 1.5 * meop * pi * innerD * innerD / 4
    per = screwD * wall * al6061BearingYieldPa
    n = max 6 (ceiling (f / per))
