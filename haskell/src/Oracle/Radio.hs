-- | Link geometry and free-space loss behind the balloon's tracking checks
-- (crates/wright-balloon/src/radio.rs), from the textbook relations:
--
--   free-space loss    FSPL = 20 log10 (4 pi d / lambda), lambda = c / f
--                      (Friis; equivalently 32.45 + 20 log10 d_km + 20 log10 f_MHz)
--   slant range        s^2 = a^2 + b^2 - 2 a b cos theta over a spherical
--                      Earth, a = R + h_station, b = R + h_payload,
--                      theta = ground distance / R (law of cosines)
--   elevation          tan el = (b cos theta - a) / (b sin theta): the
--                      vertical and horizontal components of the ray at the
--                      station. The Rust takes the arcsine of the vertical
--                      component over the slant range instead; the two agree
--                      exactly because s^2 is the sum of the two squares.
--   radio horizon      d_h = sqrt (2 k R h_station) + sqrt (2 k R h_payload),
--                      k = 4/3 the standard-refraction Earth factor
--   half-wave dipole   G(el) = peak + 20 log10 |cos (pi/2 cos th) / sin th|,
--                      th = pi/2 - |el| the angle from the axis (Balanis,
--                      Antenna Theory, eq. 4-62), floored at the null depth
--                      the design assumes
--
-- GNSS coexistence with the payload's own transmitters (the same radio.rs,
-- used by analysis.rs for the `Coexist` rows):
--
--   near-field clamp   Friis is a far-field law; inside the radian sphere
--                      r < lambda / 2 pi the loss is held at its value on
--                      the sphere, 20 log10 2 = 6.02 dB (the Rust clamps the
--                      distance the same way)
--   patch rejection    10 log10 (1 + Q^2 (f/f0 - f0/f)^2), Q = f0 / BW: the
--                      magnitude response of a single series-RLC resonator
--                      of -3 dB bandwidth BW (Pozar, Microwave Engineering,
--                      6.1, eq. 6.9 with |1 + 2jQ dw/w0| written exactly)
--   thermal floor      -174 dBm/Hz + 10 log10 B + NF: k T at 290 K is
--                      10 log10 (1.380649e-23 x 290 x 1e3) = -173.98 dBm/Hz,
--                      rounded to the engineering -174 as the Rust does
--   blocking limit     P1dB of the LNA minus a linear-operation back-off
--   in-band limit      the floor plus the receiver's reacquisition headroom
--                      (specified signal level minus reacquisition
--                      sensitivity): the noise rise it still reacquires under
--   harmonics in L1    {n in 2..20 : |n f - f0| <= B / 2}
--   coupled power      P_tx + G_tx - FSPL(sep, f), then minus the patch
--                      rejection for the fundamental; harmonics 60 dB under
--                      the carrier (the design's assumption, passed in)
module Oracle.Radio
  ( fsplDb
  , fsplNearDb
  , Geometry (..)
  , geometry
  , dipoleGainDbi
  , patchRejectionDb
  , thermalFloorDbm
  , blockingLimitDbm
  , inbandLimitDbm
  , harmonicsIn
  , coupledDbm
  ) where

c0 :: Double
c0 = 299792458

-- | Free-space path loss, dB, for a distance (m) and frequency (MHz).
fsplDb :: Double -> Double -> Double
fsplDb dM fMhz = 20 * logBase 10 (4 * pi * dM * fMhz * 1e6 / c0)

-- | Free-space loss with the distance held at the radian sphere lambda/2pi
-- (the far-field edge) when closer.
fsplNearDb :: Double -> Double -> Double
fsplNearDb dM fMhz = fsplDb (max dM (lambda / (2 * pi))) fMhz
  where lambda = c0 / (fMhz * 1e6)

-- | Out-of-band rejection, dB, of a resonator centred on f0 with bandwidth
-- bw (both MHz) at frequency f (MHz).
patchRejectionDb :: Double -> Double -> Double -> Double
patchRejectionDb f0 bw f = 10 * logBase 10 (1 + (q * x) ^ (2 :: Int))
  where
    q = f0 / bw
    x = f / f0 - f0 / f

-- | Receiver noise floor, dBm, in a bandwidth (MHz) with a noise figure (dB).
thermalFloorDbm :: Double -> Double -> Double
thermalFloorDbm bwMhz nfDb = -174 + 10 * logBase 10 (bwMhz * 1e6) + nfDb

-- | Largest interferer before gain compression, dBm: P1dB, back-off.
blockingLimitDbm :: Double -> Double -> Double
blockingLimitDbm p1db backoff = p1db - backoff

-- | Largest in-band interferer that still lets the receiver reacquire, dBm:
-- bandwidth (MHz), noise figure, specified signal level, reacquisition
-- sensitivity (dBm).
inbandLimitDbm :: Double -> Double -> Double -> Double -> Double
inbandLimitDbm bwMhz nfDb signal reacq = thermalFloorDbm bwMhz nfDb + (signal - reacq)

-- | Harmonic orders 2..20 of f that fall within bw/2 of f0 (all MHz).
harmonicsIn :: Double -> Double -> Double -> [Int]
harmonicsIn f0 bw f = [n | n <- [2 .. 20], abs (fromIntegral n * f - f0) <= bw / 2]

-- | Power coupled into a receiver at a separation (m), dBm: transmitter
-- power (dBm), its antenna gain (dBi), separation, frequency (MHz).
coupledDbm :: Double -> Double -> Double -> Double -> Double
coupledDbm txDbm gainDbi sepM fMhz = txDbm + gainDbi - fsplNearDb sepM fMhz

data Geometry = Geometry
  { slantM        :: !Double
  , elevationRad  :: !Double
  , withinHorizon :: !Bool
  } deriving (Show)

-- | Earth radius, k factor, ground distance, station altitude, payload
-- altitude (m). A coincident station and payload look straight up.
geometry :: Double -> Double -> Double -> Double -> Double -> Geometry
geometry rE k ground h1 h2 = Geometry s el (ground <= horizon)
  where
    th = ground / rE
    a = rE + h1
    b = rE + h2
    s = sqrt (max 0 (a * a + b * b - 2 * a * b * cos th))
    el | s <= 0 = pi / 2
       | otherwise = atan2 (b * cos th - a) (b * sin th)
    horizon = sqrt (2 * k * rE * max 0 h1) + sqrt (2 * k * rE * max 0 h2)

-- | Gain of a vertical half-wave dipole toward elevation el (rad), dBi:
-- floor, peak gain, elevation.
dipoleGainDbi :: Double -> Double -> Double -> Double
dipoleGainDbi floorDbi peak el = max floorDbi (peak + 20 * logBase 10 (max 1e-9 g))
  where
    th = pi / 2 - abs el
    sn = sin th
    g | sn < 1e-6 = 0
      | otherwise = abs (cos (pi / 2 * cos th) / sn)
