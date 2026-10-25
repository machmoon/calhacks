-- | Closed-form structures: the formulas behind Wright's structural checks
-- (VTOL boom and spar, rocket tank walls, bulkheads and fins) and behind
-- the FEA validation table in docs/verification.md, restated from the
-- primary sources with nothing taken from the Rust:
--
--   hollow circle        I = pi (od^4 - id^4) / 64,  A = pi (od^2 - id^2) / 4
--   cantilever, end load delta = P L^3 / (3 E I)
--                        (Roark, Formulas for Stress and Strain, Table 8.1 case 1a)
--   bending              sigma = M c / I
--   first bending mode   f1 = (beta1 L)^2 / (2 pi) sqrt (E I / (m' L^4)),
--                        beta1 L = 1.8751, m' = mass per length
--                        (Blevins, Formulas for Natural Frequency, Table 8-1)
--   thick cylinder       hoop at the bore p (ro^2 + ri^2) / (ro^2 - ri^2),
--                        axial (closed ends) p ri^2 / (ro^2 - ri^2), radial -p
--                        (Lame; Roark Table 13.5 case 1c), von Mises from the
--                        three principals
--   circular plate under uniform q, radius a, D = E t^3 / (12 (1 - nu^2)):
--     simply supported   y = q a^4 (5 + nu) / (64 D (1 + nu)),
--                        sigma at the centre 3 (3 + nu) q a^2 / (8 t^2)
--                        (Roark Table 11.2 case 10a)
--     clamped            y = q a^4 / (64 D), centre 3 (1 + nu) q a^2 / (8 t^2),
--                        edge 3 q a^2 / (4 t^2)      (Roark Table 11.2 case 10b)
--   axial shell buckling sigma_cl = E t / (R sqrt (3 (1 - nu^2)))
--                        (Timoshenko & Gere, Theory of Elastic Stability, eq. 11-13)
--   bending knock-down   gamma = 1 - 0.731 (1 - exp (-phi)), phi = sqrt (R / t) / 16
--                        (NASA SP-8007, Buckling of Thin-Walled Circular
--                        Cylinders, 1968, eq. 6)
--   fin flutter          Vf = a sqrt (G / (K AR^3 p (lambda + 1) / (2 (AR + 2) (t/c)^3))),
--                        K = 39.3 / 14.696 with p and G in the same units
--                        (NACA TN 4197 as printed, with the constant Apogee
--                        Peak of Flight #615 corrected; the same source the
--                        Rust cites, so this one is a transcription check)
--   bulkhead thickness   t = a sqrt (3 (3 + nu) p sf / (8 allow))
--                        (case 10a solved for t at sf x p against allow)
module Oracle.Beam
  ( tubeI
  , tubeArea
  , cantileverTip
  , bendingStress
  , cantileverFirstMode
  , Lame (..)
  , lame
  , plateSimple
  , plateClamped
  , shellBucklingClassical
  , sp8007Knockdown
  , flutterSpeed
  , bulkheadThickness
  ) where

-- | Second moment of area of a hollow circle (m^4) from outer and inner
-- diameters; an inner diameter of 0 is a solid rod.
tubeI :: Double -> Double -> Double
tubeI od idm = pi * (od ** 4 - idm ** 4) / 64

-- | Section area of a hollow circle, m^2.
tubeArea :: Double -> Double -> Double
tubeArea od idm = pi * (od * od - idm * idm) / 4

-- | Tip deflection of a cantilever under an end load: P, L, E, I.
cantileverTip :: Double -> Double -> Double -> Double -> Double
cantileverTip p l e i = p * l ** 3 / (3 * e * i)

-- | Bending stress at distance c from the neutral axis: M, c, I.
bendingStress :: Double -> Double -> Double -> Double
bendingStress m c i = m * c / i

-- | First bending frequency (Hz) of a uniform cantilever: E, I, mass per
-- length, L.
cantileverFirstMode :: Double -> Double -> Double -> Double -> Double
cantileverFirstMode e i mPerL l = (1.8751 ** 2 / (2 * pi)) * sqrt (e * i / (mPerL * l ** 4))

-- | Lame's stresses at the bore of a closed-end thick cylinder.
data Lame = Lame
  { hoop     :: !Double
  , axial    :: !Double
  , radial   :: !Double
  , vonMises :: !Double
  } deriving (Show)

-- | p, inner radius, outer radius.
lame :: Double -> Double -> Double -> Lame
lame p ri ro = Lame h a r vm
  where
    k = ro * ro - ri * ri
    h = p * (ro * ro + ri * ri) / k
    a = p * ri * ri / k
    r = -p
    vm = sqrt (((h - a) ** 2 + (a - r) ** 2 + (r - h) ** 2) / 2)

flexuralRigidity :: Double -> Double -> Double -> Double
flexuralRigidity e nu t = e * t ** 3 / (12 * (1 - nu * nu))

-- | Simply supported circular plate under uniform q: E, nu, q, a, t ->
-- (centre deflection, centre stress).
plateSimple :: Double -> Double -> Double -> Double -> Double -> (Double, Double)
plateSimple e nu q a t =
  ( q * a ** 4 * (5 + nu) / (64 * flexuralRigidity e nu t * (1 + nu))
  , 3 * (3 + nu) * q * a * a / (8 * t * t) )

-- | Clamped circular plate under uniform q: E, nu, q, a, t ->
-- (centre deflection, centre stress, edge stress).
plateClamped :: Double -> Double -> Double -> Double -> Double -> (Double, Double, Double)
plateClamped e nu q a t =
  ( q * a ** 4 / (64 * flexuralRigidity e nu t)
  , 3 * (1 + nu) * q * a * a / (8 * t * t)
  , 3 * q * a * a / (4 * t * t) )

-- | Classical axial buckling stress of a thin cylinder: E, nu, mid-wall R, t.
shellBucklingClassical :: Double -> Double -> Double -> Double -> Double
shellBucklingClassical e nu r t = e * t / (r * sqrt (3 * (1 - nu * nu)))

-- | SP-8007 knock-down on the classical stress for bending: R, t.
sp8007Knockdown :: Double -> Double -> Double
sp8007Knockdown r t = 1 - 0.731 * (1 - exp (-(sqrt (r / t) / 16)))

-- | Fin flutter speed: speed of sound, air pressure, shear modulus, semi-span,
-- root chord, tip chord, thickness (all SI).
flutterSpeed :: Double -> Double -> Double -> Double -> Double -> Double -> Double -> Double
flutterSpeed a p g span root tip t = a * sqrt (g / denom)
  where
    area = 0.5 * (root + tip) * span
    ar = span * span / area
    lam = tip / root
    denom = (39.3 / 14.696) * ar ** 3 * p * (lam + 1) / (2 * (ar + 2) * (t / root) ** 3)

-- | Thickness of a simply supported circular bulkhead: nu, radius, p,
-- safety factor, allowable stress.
bulkheadThickness :: Double -> Double -> Double -> Double -> Double -> Double
bulkheadThickness nu a p sf allow = a * sqrt (3 * (3 + nu) * p * sf / (8 * allow))
