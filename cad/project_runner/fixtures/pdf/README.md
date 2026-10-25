# PDF input regression fixtures

These original synthetic documents test ingestion, not aircraft engineering. They contain no third-party CAD or research data.

- `vtol-requirements.pdf`: two readable pages with a simple vector sketch and intentionally non-default `Span=1900 mm`, `WingletHeight=85 mm`, `TailDihedralAngle=32 deg`. Both rasterized pages were visually inspected.
- `image-only.pdf`: one embedded bitmap without a text layer; preparation must supply its raster and explicitly report no OCR.
- `encrypted.pdf`: password-protected copy of the text fixture, deliberately rejected.
- `five-pages.pdf`: exceeds the four-page input limit and must be rejected without truncation.

Created with ReportLab and Pillow; encryption uses pypdf. These are frozen test inputs. The PDF preparation layer does not infer design values, run training, or claim that its diagrams have been interpreted.

```sh
python3 cad/project_runner/pdf_input.py prepare \
  cad/project_runner/fixtures/pdf/vtol-requirements.pdf out/pdf-input-smoke
python3 -m unittest discover -s cad/project_runner -p test_pdf_input.py -v
```

Runtime selection preserves virtual-environment executable paths: explicit `WRIGHT_PDF_PYTHON`, capable configured `WRIGHT_PYTHON`, current Python, then the existing Codex bundled PDF runtime. It never downloads packages. The current backend uses pypdf text extraction and PDFium rasterization, bounded to one 20 MiB PDF, four pages, 1,200 pixels per page, 8 MiB per image and 16 MiB total. Cached originals, text and every page image are verified against saved hashes.
