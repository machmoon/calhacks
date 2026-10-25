# Electronics A/B: the Python baseline vs Wright (2026-10-25)

Real runs only, KiCad 10.0.6 kicad-cli. (a) the Python baseline's CP-SAT placer (`baseline/board-eval-2026-10-24-cpsat.json`; servo: `baseline/servo-2026-10-24.json`, power width 1.0 mm run). (b) Wright annealing only (`--no-groups --single-seed`, no model call). (c) Wright full (Claude groups, 12 seeds, 6 quick-routed, best fully routed). b/c from `evals/electronics/run.sh` (raw lines: `eval-2026-10-25.log`), run 00:15-00:30 under a load average of about 250 from other builds, so wall times are inflated. DRC = errors + unconnected. Decap loop: the baseline's own metric vs Wright's total over caps (`wright_place::decap::loops_nm`); the definitions are not proven identical.

| fixture | col | area mm2 | decap loop mm | routed | DRC | ERC | parity | power track vs need | wall s | model calls / tokens in+out |
|---|---|---|---|---|---|---|---|---|---|---|
| servo | a | 2562 (76.2 x 33.7) | not measured | 100 % | 2 (starved_thermal) | 0 | 0 | 1.0 mm vs 1.82 mm needed for 5 A | 38.8 | 0 |
| servo | b | 2340 | 17.3 | 100 % | 0 | 0 | 0 | +6V/SW 1.85, VBAT 0.75 vs 0.71, GND poured | 23.3 | 0 |
| servo | c | 3883 | 23.2 | 100 % | 0 | 0 | 0 | same as b | 47.6 | 1 / 7127+1678 |
| ldo | a | 140.6 | 19.9 | 100 % | 0 | 0 | 0 | not measured | 9.7 | 0 |
| ldo | b | 131.3 | 13.3 | 100 % by Wright, DRC disagrees | 3 (unconnected U1 SOT-223) | 0 | 0 | 0.2 vs 0.08 | 20.7 | 0 |
| ldo | c | 135.0 | 14.7 | same as b | 3 | 0 | 0 | 0.2 vs 0.08 | 29.6 | 1 / 3559+411 |
| blinker | a | 172.1 | 12.4 | 100 % | 0 | 0 | 0 | not measured | 22.6 | 0 |
| blinker | b | 132.0 | 5.4 | 100 % | 0 | 0 | 0 | 0.2 vs 0.08 | 20.5 | 0 |
| blinker | c | 123.3 | 5.3 | 100 % | 0 | 0 | 0 | 0.2 vs 0.08 | 41.2 | 1 / 4747+1020 |
| blinker10 | a | 318.2 | 13.1 | 100 % | 0 | 0 | 0 | not measured | 26.6 | 0 |
| blinker10 | b | 192.0 | 5.0 | 100 % | 0 | 0 | 0 | 0.2 vs 0.08 | 20.6 | 0 |
| blinker10 | c | 196.0 | 9.7 | 100 % | 0 | 0 | 0 | 0.2 vs 0.08 | 41.2 | 1 / 4928+944 |
| ref25 | a | 116.5 | 13.3 | 100 % | 0 | 0 | 0 | not measured | 22.3 | 0 |
| ref25 | b | 90.0 | 9.7 | 100 % | 0 | 0 | 0 | 0.2 vs 0.08 | 16.6 | 0 |
| ref25 | c | 90.0 | 9.7 | 100 % | 0 | 0 | 0 | 0.2 vs 0.08 | 27.7 | 1 / 3989+604 |
| two_chip_usb | a | 305.6 | 40.4 | 100 % | 0 | 0 | 0 | not measured | 22.8 | 0 |
| two_chip_usb | b | 281.8 | 24.9 | 100 % | 0 | 0 | 0 | 0.2 vs 0.08 | 14.1 | 0 |
| two_chip_usb | c | 290.3 | 24.3 | 100 % | 0 | 0 | 0 | 0.2 vs 0.08 | 39.4 | 1 / 5295+951 |
| esp32_devboard | a | 1525.7 | 60.9 | 100 % | 0 | 0 | 0 | not measured | 92.6 | 0 |
| esp32_devboard | b | 2796.8 | 64.1 | 100 % by Wright, DRC disagrees | 2 | 0 | 0 | 0.2 vs 0.08 | 20.2 | 0 |
| esp32_devboard | c | 3024.0 | 63.2 | same as b | 3 | 0 | 0 | 0.2 vs 0.08 | 33.7 | 1 / 7799+1109 |

Where the baseline wins: ESP32 (area half of Wright's, a shorter loop, DRC clean where Wright has 2-3 errors), LDO (DRC clean where Wright has 3 unconnected) and wall time on the LDO. Not run: the full 15-check pass/fail per fixture for (a) (the baseline has no matching check set), and the baseline's power-trace widths on the non-servo fixtures.

Verdict: (c) is not better than (b). It is smaller on blinker only, ties on ref25, and is larger or has a longer loop on servo, blinker10, two_chip_usb and esp32. The placer does not yet hold the agent's edge and near hints (the verifier re-measured 8 of 13 claims holding on the servo demo), so the model call adds time without improving the board. This is reported as measured and not tuned.
