# data/air: measured and published data for `wright-air`

Copied unchanged on 2026-10-24 from the Python baseline
(`engine/silkscreen/aircraft/data/`), where each table was built from the
sources below. Nothing here is fitted or estimated; a field nobody published
is `null`, and `wright-air` rejects a row whose role needs it when the catalog
loads (it is never read as zero).

| file | what | source |
|---|---|---|
| `props_uiuc.json` | 95 propellers: static runs (CT, CP vs RPM) and dynamic runs (CT, CP vs advance ratio J at several RPMs) | UIUC Propeller Database, Vols 1-4 (Brandt, Deters, Ananda, Dantsker, Selig), https://m-selig.ae.illinois.edu/props/propDB.html, downloaded 2026-10-24. Definitions: CT = T/(rho n^2 D^4), CP = P/(rho n^3 D^5), J = V/(n D), n in rev/s. |
| `airfoils.json` | 21 sections: coordinates, and CL, CD, CM tables over alpha -8..18 deg at Re 3e4..5e5 | coordinates from the UIUC Airfoil Coordinates Database (Selig), https://m-selig.ae.illinois.edu/ads/coord_database.html, or the NACA 4-digit definition; polars from NeuralFoil "xlarge" (Sharpe, MIT licence, https://github.com/peterdsharpe/NeuralFoil), run 2026-10-24. NeuralFoil's own confidence per point is kept in `confidence`. |
| `ardupilot_params.json` | 155 ArduPlane parameters: names, ranges, value labels | ArduPilot `apm.pdef.json` (ArduPilot autotest server, master), https://autotest.ardupilot.org/Parameters/ArduPlane/apm.pdef.json, downloaded 2026-10-24. Interface metadata only (ArduPilot is GPL-3.0; no code is used). |
| `catalog.json` | motors, props, ESCs, cells, packs, carbon tubes, servos, flight controllers, GPS, receivers, airspeed sensors, telemetry, filaments, hardware | makers' and retailers' pages, read 2026-10-24: **every row carries the `url` its numbers came from** (and `spec_url` when specs and price are on different pages), `retrieved`, `confidence` ("verified" = read on the cited page) and `notes` naming any second source. Prices are what the cited page showed on `retrieved`. |

Rules the loader enforces (`crates/wright-air/src/catalog.rs`):

* a row without `url` and `retrieved` is rejected;
* a motor without Kv, resistance, no-load current, mass or current rating is
  rejected (its operating point cannot be computed); likewise a cell without
  capacity, voltage, mass, current or resistance, a prop without measured
  data, a part that goes in the pod or the wing without all three dimensions;
* a missing price is kept, and fails the budget check of any design that
  uses the part.

Fields added 2026-10-24 (after the copy):

* `stock` on every row: `in_stock`, `unstated`, `made_to_order`, `pre_order`,
  `backorder`, `out_of_stock` or `discontinued`, as the cited page showed it on
  `retrieved` (quoted in `stock_evidence`). A row without a stock line is
  `unstated`, never `in_stock`. The spec's `requirements.stock` policy
  (`in_stock`, `listed` default, `lead_time`, `restock`) decides what the search
  may choose and what `parts_orderable` passes; `discontinued` never passes.
* `hub` on props and `prop_interface` on motors: centre bore / shaft
  (`bore_mm`), bolt circle (`bolts`, `screw`, `circle_mm`), supplied adapter
  bores (`adapters_mm`) and the `source` page (T-Motor 2D drawing PDFs, HQProp
  / APC / Gemfan product pages). `lift_prop_mount` fails when either side is
  missing.
