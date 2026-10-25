-- | The U.S. Standard Atmosphere, 1976 (NOAA-S/T 76-1562, NASA-TM-X-74335),
-- 0 to 84.852 km geopotential, written independently of the Rust crates.
--
-- Method (deliberately different from wright-balloon / wright-rocket, which
-- integrate base pressures upward from sea level): each layer's base
-- pressure is the *published* value, so an error in either the law or the
-- layer table shows up as a disagreement.
--
-- Sources:
--   * US76 eq. 18 (geopotential), eqs. 23 (T), 33a/33b (P), 42 (rho),
--     50 (speed of sound), 51 (Sutherland viscosity); Table 4 (layers).
--   * Constants US76 Table 2 / Sec. 1.2: g0' = 9.80665 m^2/(s^2 m'),
--     r0 = 6 356 766 m, R* = 8.31432e3 J/(kmol K), M0 = 28.9644 kg/kmol,
--     beta = 1.458e-6, S = 110.4 K, gamma = 1.4.
--   * Base pressures: ICAO Doc 7488/3 (1993) Table D, identical to US76
--     below 80 km, as transcribed in the `ambiance` package
--     (github.com/airinnova/ambiance, src/ambiance/ambiance.py,
--     CONST.LAYER_SPEC_PROP).
module Oracle.Atmosphere
  ( Air (..)
  , Layer (..)
  , layers
  , g0, r0, rStar, m0, rAir
  , geopotential
  , atGeopotential
  , atGeometric
  ) where

data Air = Air
  { airH   :: !Double  -- ^ geopotential altitude, m'
  , airT   :: !Double  -- ^ K
  , airP   :: !Double  -- ^ Pa
  , airRho :: !Double  -- ^ kg/m^3
  , airMu  :: !Double  -- ^ Pa s
  , airA   :: !Double  -- ^ m/s
  } deriving (Show)

data Layer = Layer
  { lH :: !Double  -- ^ base geopotential altitude, m'
  , lT :: !Double  -- ^ base temperature, K
  , lL :: !Double  -- ^ lapse rate, K/m'
  , lP :: !Double  -- ^ base pressure, Pa (published)
  } deriving (Show)

g0, r0, rStar, m0, rAir :: Double
g0 = 9.80665
r0 = 6356766
rStar = 8.31432e3
m0 = 28.9644
rAir = rStar / m0

-- | US76 Table 4 (H_b, T_b, L_b) with ICAO/US76 base pressures.
layers :: [Layer]
layers =
  [ Layer 0       288.15 (-6.5e-3) 1.01325e5
  , Layer 11000   216.65 0.0       2.26320e4
  , Layer 20000   216.65 1.0e-3    5.47487e3
  , Layer 32000   228.65 2.8e-3    8.68014e2
  , Layer 47000   270.65 0.0       1.10906e2
  , Layer 51000   270.65 (-2.8e-3) 6.69384e1
  , Layer 71000   214.65 (-2.0e-3) 3.95639e0
  ]

topH :: Double
topH = 84852

-- | US76 eq. 18: H = r0 Z / (r0 + Z).
geopotential :: Double -> Double
geopotential z = r0 * z / (r0 + z)

layerFor :: Double -> Layer
layerFor h = last (head layers : takeWhile (\l -> lH l <= h) layers)

-- | Air at geopotential altitude h (m'), 0 <= h <= 84 852.
atGeopotential :: Double -> Air
atGeopotential h0 = Air h t p rho mu a
  where
    h = max 0 (min topH h0)
    Layer hb tb lb pb = layerFor h
    dh = h - hb
    t = tb + lb * dh                                         -- eq. 23
    p | lb == 0   = pb * exp (-(g0 * m0 * dh) / (rStar * tb)) -- eq. 33b
      | otherwise = pb * (tb / t) ** ((g0 * m0) / (rStar * lb)) -- eq. 33a
    rho = p * m0 / (rStar * t)                               -- eq. 42
    mu = 1.458e-6 * t ** 1.5 / (t + 110.4)                   -- eq. 51
    a = sqrt (1.4 * rStar * t / m0)                          -- eq. 50

-- | Air at geometric altitude z (m).
atGeometric :: Double -> Air
atGeometric = atGeopotential . geopotential
