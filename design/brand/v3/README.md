# Wright mark v3 (Pat, 2026-10-24 ~21:30)

Pat: "make the logo chatgpt, claude, and azure in the form of devins logo and make it black", then "replace openai with creao ai".

- Form: Cognition/Devin's logomark (pointy-top hexagon cells joined by pinched necks; read from cognition.ai's inline SVG). Three cells in a triangle instead of Devin's seven.
- Cells: top = Creao (triangle + ring, simplified from creao.ai's apple-touch-icon), bottom-left = Claude (11-ray spark), bottom-right = Azure (two-blade A). Glyphs are simplified redraws, not the companies' artwork, knocked out of one black fill.
- Files: `mark.svg` (black), `mark-white.svg`, `mark-512.png`; generator `build_mark.py.txt` (run with python3 -I, arg = ink colour).
- Applied 21:37 to the Wright desktop app: `app/src-tauri/icons/app-icon.png` (black mark on a white 824 px tile, Apple template) -> `npx tauri icon`, and `app/src/assets/wright-mark.png` (sidebar). Old icons backed up in the session scratchpad `v3/old-icons/`. Tray icons unchanged (they are the status orb, not the logo).
- Trademark note: the three glyphs are other companies' marks. Fine as a hackathon "built with" mark; replace before using Wright's logo outside the event.
