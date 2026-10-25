# Wright desktop overlay

Run from the Wright repository: `cd app && npm ci && npm run tauri dev`.
The shell has a separate Cargo workspace and lockfile under `src-tauri/`.
It finds Wright at the repository containing this app; `WRIGHT_ROOT` overrides
that path (also use it when distributing a compiled app to another checkout).
Build the backend with `cargo build --release -p wright-cli` from the repository
root. Native CAD also needs the FreeCAD and Python dependencies described in
`../docs/cad-astra-subscription.md` and a signed-in Codex subscription for planning.

Open the overlay from the workbench, drop one PDF onto the prompt, wait for its
attachment chip, and submit. PDF input goes directly to the native CAD runner;
the dashboard shows streamed renders, native artifacts and check results.
Current prepared VTOL support accepts one PDF up to 20 MiB and four pages.
Unsupported requirements and failed engineering checks remain visible.

This imports the existing Pluely-based desktop shell, including its settings and
legacy pages. The CAD path uses Wright subprocesses and does not require the
legacy silkscreen Python engine. Legacy PCB pages still require their separate engine.
See LICENSE, NOTICE.md and UPSTREAM-README.md for retained attribution.
