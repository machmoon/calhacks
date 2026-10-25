# Desktop overlay in Wright

The maintained desktop shell is now `app/` in this repository. Do not depend on
or edit a separate app checkout for the Wright CAD workflow. Run `npm ci` then
`npm run tauri dev` in `app/`. The app has its own Cargo workspace and lockfile.
The native bridge resolves the containing repository; `WRIGHT_ROOT` can override
it for a packaged binary or another backend checkout.

Imported features include PDF drag/drop preparation, attachment validation,
PDF-to-native-CAD routing, duplicate submission prevention, dashboard handoff,
streamed render/probe progress, STL viewing and artifact opening. Existing shell
settings, skins and licenses are retained. Legacy PCB engine pages are retained
but their external Python engine is not imported or required for native CAD.

Validation during integration:
- Production TypeScript/Vite build passed (existing bundle-size warnings).
- 83 focused CAD, PDF and overlay regression tests passed.
- Broader suite: 1735 passed initially; the sole failure was the omitted blank
  `.env.example`. Restoring it passed all 33 tests in that configuration group.
- The desktop Rust test build passed, including all four Wright bridge tests
  and the regression that checks the co-located native PDF runner.
- The actual two-page VTOL fixture prepared successfully in this checkout,
  with text and page raster evidence.
- The preceding real desktop runs and timing reports are documented in
  `cad-astra-subscription.md`; this migration does not imply new flight validation.

Current PDF support is the prepared VTOL recipe: one PDF, at most 20 MiB and
four pages. Unsupported requirements stop the run. Passing workflow checks is
separate from engineering qualification, which still has unresolved findings.
