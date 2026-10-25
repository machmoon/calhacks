# Wright: product brief

Written 2026-10-24 for Cal Hacks 2026 by the product/UX pass. Sources are linked
inline. Every quote comes from a page that was fetched, except where it says
"(search snippet)". Gap: Reddit (r/diydrones, r/fpv, r/rocketry,
r/highaltitude) blocks automated fetching, and The Rocketry Forum returned
403, so those communities are under-represented. Read them by hand before
making any claim about them specifically.

## The promise, in one sentence

Describe the vehicle you want, and Wright gives you one you can go build:
files to print, parts to order, wiring and settings to load, and a list of
checks that says, with numbers, why it will fly. When a check has no evidence,
it fails instead of passing quietly.

## Who it is for

### 1. Dana, the quadplane builder (hobby, ArduPilot)

She has built two quads and wants a VTOL for long flights. She can use eCalc
and Mission Planner. She is afraid of the first transition.

- The most common crash is reversed surfaces or bad parameters at the first
  transition: "tumbles out of the sky right at the beginning of transition"
  ([ArduPilot 65883](https://discuss.ardupilot.org/t/quadplane-flips-during-transition/65883),
  [31535](https://discuss.ardupilot.org/t/loss-of-control-after-transition-to-forward-flight-solved/31535),
  [142096](https://discuss.ardupilot.org/t/quadplane-nearly-crashed-after-the-transition/142096)).
- She doesn't know which Q_ parameters to set: "what parameters i have to
  setup for first flight test?"
  ([75675](https://discuss.ardupilot.org/t/quadplane-help-for-first-time-setup/75675),
  [13452](https://discuss.ardupilot.org/t/my-quadranger-refuse-to-hover/13452)).
- Hover current and weight surprises: 90 A in Q_HOVER on a 22 kg build
  ([101077](https://discuss.ardupilot.org/t/quad-motors-current-higher-than-expected/101077)).
  Wrong eCalc inputs, too
  ([36761](https://discuss.ardupilot.org/t/unable-to-take-off-new-hexarotor-build-help-please/36761)).
- Parts that don't fit: "there wasn't enough room beneath the wing spars"
  ([Nano Goblin 39539](https://discuss.ardupilot.org/t/nano-goblin-vtol-project-help-advice-needed/39539)).
- Printed airframes need printer-specific settings. Designer-supplied
  profiles fail on other machines
  ([Prusa forum, LW-PLA](https://forum.prusa3d.com/forum/original-prusa-i3-mk3s-mk3-how-do-i-print-this-printing-help/slicer-settings-for-lw-pla-rc-aircraft)).

### 2. Marcus, propulsion lead on a collegiate rocket team

He has two years until he graduates, a competition rulebook, and a team that
loses half its knowledge every June.

- The rules set both a floor and a ceiling: dynamic stability of at least
  1.5 calibres, and the rocket fails if it is over 4 static or over 6 dynamic.
  Rail exit is 100 ft/s, or lower only with multiple simulations
  ([2025 IREC DTEG §10](https://www.soundingrocket.org/uploads/9/0/6/4/9064598/2025_irec_dteg_v1.1.3_10-22-24.pdf)).
- The simulations disagree: about 11,450 ft (OpenRocket) vs 9,224 ft
  (RASAero II) for the same rocket
  ([TRF 186458](https://www.rocketryforum.com/threads/openrocket-and-rasaero-ii-predicted-altitude-difference.186458/),
  search snippet). Simulated CG is rarely right until it is overwritten with
  the real measured mass
  ([TRF 1926820](https://www.rocketryforum.com/posts/1926820/), search snippet).
- Liquid engines fail at start: propellant pools and lights all at once
  ([spaceq, Launch Canada teams](https://spaceq.ca/launch-canada-student-teams-hard-parts/),
  [UW WISP](https://engineering.wisc.edu/blog/wisp-puts-uw-on-the-map-with-states-first-liquid-rocket/)).
  "We thought it would work… It did not work."
- Documentation is heavy: progress reports, CSV hot-fire data, and safety sheets
  ([DTEG](https://www.soundingrocket.org/uploads/9/0/6/4/9064598/2025_irec_dteg_v1.1.3_10-22-24.pdf),
  [FAR FAQ](https://friendsofamateurrocketry.org/launch-contest-faq/)).
- Knowledge leaves when people graduate: "a huge loss in our knowledge
  transfer" ([UMD](https://aero.umd.edu/news/story/building-a-bigger-better-rocket-team)).

### 3. Ms. Okafor, physics teacher running a school balloon launch

This is a one-off project with a budget, students, and one flight they can't
afford to lose.

- She isn't sure about the fill: "you can easily get the neck lift
  measurement wrong (especially if this is a first time)"
  ([UKHAS list](https://groups.google.com/g/ukhas/c/vU4S_KDfBXw)). The
  recommended aim is an ascent rate of about 5 m/s
  ([SondeHub calc](https://sondehub.org/calc/)).
- Losing the payload: GPS lost lock at launch
  ([Felix](https://felixonline.co.uk/articles/2018-11-23-imperial-s-space-society-launches-high-altitude-weather-balloon/)).
  A GPS that stops tracking at about 12.5 km until it is set to airborne mode
  ([SparkFun](https://community.sparkfun.com/t/gps-modules-not-tracking-above-12-5km-in-altitude/41785)).
  Predictions are good to 5 to 10 miles
  ([Wyoming Space Grant FAQ](https://wyomingspacegrant.org/balloonprogram/faq/)).
- Parachute sizes from vendors are optimistic: "A Cd of 1.5 is pure LaLa
  Land" ([UKHAS list](https://groups.google.com/g/ukhas/c/giEVDwTUJ6I)).
- The rules: Part 101 weight and density limits
  ([eCFR Part 101](https://www.ecfr.gov/current/title-14/chapter-I/subchapter-F/part-101)).
  ATC notice 6 to 24 h before launch, including the forecast impact point
  ([14 CFR 101.37](https://www.law.cornell.edu/cfr/text/14/101.37)). APRS
  needs a ham licence (College of Charleston poster, search snippet).

## Jobs to be done (ranked)

1. **Before I spend money or build anything, tell me whether it will fly,
   and show me why.** This means feasibility (hover current, stability
   calibres, ascent rate) with the inputs visible.
2. **Give me the exact parts to buy and confirm they fit together.** The
   parts have to fit physically, electrically, and within budget.
3. **Give me settings I can load:** ArduPilot params, slicer profiles for my
   printer, fill and neck lift, ejection charge.
4. **Catch the mistake that would cost me the vehicle on the first flight:**
   reversed surfaces, GPS mode, recovery redundancy, a tracker left switched off.
5. **Write down the reasoning so the next person (or the competition) can
   check it.** That covers reports, safety sheets, the 101.37 notice, and what
   the team knows after its members graduate.

## What earns trust (from the sources)

People trust a tool because it agrees with a measurement or has a track
record, not because it shows more decimal places. "I've done 50+ flights
using it…" ([UKHAS predictor thread](https://groups.google.com/g/ukhas/c/GeHVv2trHfs)).
eCalc matched a measured 36 min
([ArduPilot 117451](https://discuss.ardupilot.org/t/endurance-of-drone/117451)).
Simulations are trusted after real masses override them (TRF). Distrust comes
from errors nobody can explain (XFLR5, eCalc being "so far off",
[IntoFPV](https://intofpv.com/t-55-min-35-km-6-lr)). So every Wright number
needs its inputs and its source, and wherever possible a measured comparison
(the three evals against real vehicles).

## The 5 moments where Wright has to earn trust

1. **The first check report.** It has to show failures. A first design that
   passes everything looks rigged. Each failure needs a number, a limit, and a
   source ("8.1 mm at hover thrust vs 6.0 mm limit"). A check without evidence
   is shown as a failure, in words.
2. **The user's correction.** When someone circles a part and says "too
   thin", the agents have to name what they understood ("circled the left
   boom") and tie it to a check. Then the check has to change, visibly, with
   the delta.
3. **The parts list.** Every line needs a real product URL, a price, and a
   retrieved date. A homepage link reads as made up.
4. **The hand-off files.** G-code that opens in the user's slicer, a
   .param file that loads in Mission Planner, and a wiring diagram whose pins
   match the board's real pinout.
5. **The comparison with a real vehicle.** "Our design vs the 130-mile build
   / the Berkeley rocket / Pat's balloon, under the same checks." This is the
   track-record substitute for a new tool.

## What to cut (for the demo and for v1)

- **Don't show all three domains in 5 minutes.** Lead with VTOL. Rocket and
  balloon go on one closing slide showing each one's eval result.
- **Hide the spend meter by default during the demo.** It answers a question
  judges don't ask in minute one. Keep it in the header but make it quieter.
- **Take raw tool-call JSON out of the main view.** Agent sentences and check
  deltas carry the story. Tool args stay one click away.
- **Leave out regulations and paperwork generation for now.** Mention them as
  the next step (Part 101/107 checks, the 101.37 notice draft, DTEG report
  sections). They are clearly wanted, but they aren't built.
- **Don't claim anything the build doesn't do.** Use the real slicer name
  (PrusaSlicer 2.9.6, not Orca) and real renders.
