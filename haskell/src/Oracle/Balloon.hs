-- | Latex sounding-balloon lift and ascent: the relations used by the
-- CUSF / habhub burst calculator (github.com/adamgreig/cusf-burst-calc,
-- js/calc.js, `calc_update`), restated from first principles:
--
--   gross lift  L = V (rho_air - rho_gas)                  [kg]
--   neck lift   N = L - m_balloon                          [kg]
--   free lift   F = (L - m_balloon - m_payload) g          [N]
--   ascent      v = sqrt (F / (0.5 cd A rho_air)),  A = pi r^2
--               (steady state: free lift = drag)
--   burst alt   z = -H_s ln (V_launch / V_burst)           (isothermal
--               scale height H_s; calc.js default 7238.3 m)
--
-- Defaults from the calculator's index.html: rho_air 1.2050, He 0.1786,
-- H2 0.0899 kg/m^3, g 9.80665, cd 0.25-0.30 by balloon.
--
-- The inverse (radius for a target ascent rate) is solved here by
-- bisection on the monotone residual instead of calc.js's Cardano formula.
-- Note: calc.js's Cardano 'g' term divides only 27d/a by 27, not the whole
-- bracket ((2b^3/a^3 - 9bc/a^2 + 27d/a)/27), so its target-ascent-rate
-- mode does not solve the stated cubic; the forward relations above are
-- unaffected. See docs/verification.md.
module Oracle.Balloon
  ( Lift (..)
  , lift
  , radiusForAscent
  , burstAltitude
  ) where

data Lift = Lift
  { grossKg  :: !Double
  , neckKg   :: !Double
  , freeN    :: !Double
  , ascentMs :: !Double   -- ^ 0 when free lift is zero or negative: the
                           --   balloon does not rise (calc.js would give
                           --   sqrt of a negative, NaN; Wright's Rust
                           --   physics.rs::ascent_rate uses 0, and either
                           --   fails an ascent-rate requirement)
  } deriving (Show)

-- | Lift and ascent rate for a balloon of launch radius r (m).
lift :: Double -> Double -> Double -> Double -> Double -> Double -> Double -> Lift
lift rhoAir rhoGas g cd mBalloon mPayload r = Lift gross neck free v
  where
    vol = 4 / 3 * pi * r ** 3
    area = pi * r * r
    gross = vol * (rhoAir - rhoGas)
    neck = gross - mBalloon
    free = (gross - mBalloon - mPayload) * g
    v | free <= 0 = 0
      | otherwise = sqrt (free / (0.5 * cd * area * rhoAir))

-- | Launch radius giving ascent rate v (bisection; v(r) is increasing once
-- free lift is positive, since F ~ r^3 and drag area ~ r^2).
radiusForAscent :: Double -> Double -> Double -> Double -> Double -> Double -> Double -> Double
radiusForAscent rhoAir rhoGas g cd mBalloon mPayload v = go r0 hi (200 :: Int)
  where
    -- radius where free lift is exactly zero
    r0 = ((mBalloon + mPayload) / ((rhoAir - rhoGas) * 4 / 3 * pi)) ** (1 / 3)
    hi = grow (2 * r0)
    resid r = ascentMs (lift rhoAir rhoGas g cd mBalloon mPayload r) - v
    grow r = if resid r > 0 then r else grow (2 * r)
    go lo h n
      | n <= 0 || h - lo < 1.0e-13 * h = (lo + h) / 2
      | resid m < 0 = go m h (n - 1)
      | otherwise = go lo m (n - 1)
      where m = (lo + h) / 2

-- | Burst altitude (m) from launch volume, burst diameter and scale height.
burstAltitude :: Double -> Double -> Double -> Double
burstAltitude scaleH launchVol burstDiam = -scaleH * log (launchVol / burstVol)
  where burstVol = 4 / 3 * pi * (burstDiam / 2) ** 3
