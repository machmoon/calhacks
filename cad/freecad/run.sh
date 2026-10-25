#!/bin/sh
# Run the typed native worker (default), or a Python integration script.
set -eu
wright_freecad_app=${WRIGHT_FREECAD_APP:-"$HOME/Applications/FreeCAD.app"}
wright_freecad_resources="$wright_freecad_app/Contents/Resources"
if [ ! -x "$wright_freecad_resources/bin/python" ]; then
    echo "FreeCAD runtime missing at $wright_freecad_app; set WRIGHT_FREECAD_APP" >&2
    exit 1
fi
wright_freecad_here=$(CDPATH= cd -- "$(dirname -- "$0")" && pwd)
export PYTHONPATH="$wright_freecad_resources/lib:$wright_freecad_here${PYTHONPATH:+:$PYTHONPATH}"
if [ "$#" -eq 0 ]; then
    set -- "$wright_freecad_here/worker.py"
fi
exec "$wright_freecad_resources/bin/python" "$@"
