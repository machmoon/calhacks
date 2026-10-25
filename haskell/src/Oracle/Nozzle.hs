-- | Ideal-rocket (calorically perfect gas, isentropic, 1-D) nozzle relations.
--
-- Sources:
--   * NACA Report 1135, "Equations, Tables, and Charts for Compressible
--     Flow" (Ames Research Staff, 1953): eq. 44 (p/p0), eq. 43 (T/T0),
--     eq. 80 (A/A*). Table I gives the gamma = 1.4 values the tests use.
--   * G. P. Sutton & O. Biblarz, "Rocket Propulsion Elements", 9th ed.
--     (Wiley 2017), ch. 3: eq. 3-16 (exhaust velocity), eq. 3-30 (thrust
--     coefficient), eq. 3-32 (characteristic velocity c*).
--
-- The Rust engine (wright-rocket/src/performance.rs) does NOT use these: it
-- solves chemical equilibrium (CEA method) and finds the throat as the
-- maximum mass flux. That is the point -- these closed forms are an
-- independent model; they agree with it only as far as a frozen,
-- constant-gamma gas is a good description.
module Oracle.Nozzle
  ( pressureRatio
  , temperatureRatio
  , areaRatio
  , machFromArea
  , machFromPressure
  , cfOpt
  , cf
  , exhaustVelocity
  , cStar
  ) where

-- | p/p0 at Mach m (NACA 1135 eq. 44).
pressureRatio :: Double -> Double -> Double
pressureRatio g m = (1 + (g - 1) / 2 * m * m) ** (-(g / (g - 1)))

-- | T/T0 at Mach m (NACA 1135 eq. 43).
temperatureRatio :: Double -> Double -> Double
temperatureRatio g m = 1 / (1 + (g - 1) / 2 * m * m)

-- | A/A* at Mach m (NACA 1135 eq. 80).
areaRatio :: Double -> Double -> Double
areaRatio g m = (1 / m) * ((2 / (g + 1)) * (1 + (g - 1) / 2 * m * m)) ** ((g + 1) / (2 * (g - 1)))

-- | Mach for a given A/A* on the supersonic (True) or subsonic branch,
-- by bisection (A/A* is monotone on each branch).
machFromArea :: Double -> Double -> Bool -> Double
machFromArea g eps supersonic
  | eps <= 1 = 1
  | supersonic = bisect 1 100 (\m -> areaRatio g m - eps) 200
  | otherwise = bisect 1.0e-9 1 (\m -> eps - areaRatio g m) 200

-- | Bisection on [lo, hi] for an increasing f with f lo < 0 < f hi.
bisect :: Double -> Double -> (Double -> Double) -> Int -> Double
bisect lo hi f n
  | n <= 0 || hi - lo < 1.0e-15 * hi = mid
  | f mid < 0 = bisect mid hi f (n - 1)
  | otherwise = bisect lo mid f (n - 1)
  where mid = (lo + hi) / 2

-- | Mach from the static-to-stagnation pressure ratio (inverts eq. 44).
machFromPressure :: Double -> Double -> Double
machFromPressure g pr = sqrt (2 / (g - 1) * (pr ** (-((g - 1) / g)) - 1))

-- | Momentum part of the thrust coefficient for exit/chamber pressure
-- ratio pr (Sutton eq. 3-30, first term; the whole Cf when pe = pa).
cfOpt :: Double -> Double -> Double
cfOpt g pr = sqrt (2 * g * g / (g - 1) * (2 / (g + 1)) ** ((g + 1) / (g - 1)) * (1 - pr ** ((g - 1) / g)))

-- | Thrust coefficient (Sutton eq. 3-30): pe/pc, pa/pc and A_e/A_t.
cf :: Double -> Double -> Double -> Double -> Double
cf g pePc paPc eps = cfOpt g pePc + (pePc - paPc) * eps

-- | Exhaust velocity for chamber temperature t0 and gas constant r
-- (J/(kg K)), expanded to pressure ratio pr (Sutton eq. 3-16).
exhaustVelocity :: Double -> Double -> Double -> Double -> Double
exhaustVelocity g r t0 pr = sqrt (2 * g / (g - 1) * r * t0 * (1 - pr ** ((g - 1) / g)))

-- | Characteristic velocity c* (Sutton eq. 3-32).
cStar :: Double -> Double -> Double -> Double
cStar g r t0 = sqrt (g * r * t0) / (g * sqrt ((2 / (g + 1)) ** ((g + 1) / (g - 1))))
