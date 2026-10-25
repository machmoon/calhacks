"""Regenerate Wright icons from app-icon.svg; no external checkout required.

Run: python3 app/src-tauri/icons/gen_app_icon.py
The same source produces the app bundle and the sidebar/welcome image.
"""
from pathlib import Path
import shutil
import subprocess
import tempfile

ROOT = Path(__file__).resolve().parent
APP = ROOT.parents[1]


def main():
    source = ROOT / "app-icon.svg"
    subprocess.run(["npx", "tauri", "icon", str(source), "-o", str(ROOT)],
                   cwd=APP, check=True)
    with tempfile.TemporaryDirectory(prefix="wright-icons-") as directory:
        subprocess.run(["npx", "tauri", "icon", str(source), "-o", directory,
                        "-p", "512", "-p", "1024"], cwd=APP, check=True)
        shutil.copyfile(Path(directory) / "512x512.png",
                        APP / "src/assets/wright-mark.png")
        shutil.copyfile(Path(directory) / "1024x1024.png", ROOT / "app-icon.png")


if __name__ == "__main__":
    main()
