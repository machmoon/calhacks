# CADSwarm review and Wright adaptation

Reviewed https://cadswarm.design/ and its linked repository at commit
`e8a053a7405a1dbfdca78553dd7ef15da25a97be`:
https://github.com/SachchitBalamurugan/cadswarm/tree/e8a053a7405a1dbfdca78553dd7ef15da25a97be

## Useful ideas

The worker owns a persistent document, serializes native mutations and wraps
them in undo transactions. Shared dimensions drive dependent parts. These are
useful for quick follow-up changes. The worker explicitly notes that FreeCAD
is not thread-safe; multiple agents do not imply concurrent kernel mutation.
Source: `cadswarm/worker/freecad_worker.py` (lifecycle and mutate), and GUI queue
handling in `cadswarm/gui/panel.py`. The website's team panel is explicitly an
illustration, not a timing benchmark for our geometry.

Wright already has parameter objects, local native execution and live display.
Its missing piece was reuse of the still-open GUI document for subsequent typed
edits. Implemented independently in `cad/freecad/live_edits.py` and
`cad/project_runner/live_edit.py`: local file queue, GUI-thread timer, bounded
numeric parameters, stale-revision rejection, one batched recompute, undo
transaction, immutable revision snapshot and actual viewport. No external code
or package was copied. No clear reuse license was found in the inspected tree;
package.xml says "See repository". No CADSwarm installer/database/network service
was installed. Ownership/task collaboration may be useful later but would not
make this deterministic single-document recipe build faster by itself.

## Measurements and limitations

Fresh live build `runs/cad-persistent-01`: **50.80 s**.
Same-session edit `runs/cad-persistent-edit-02`: **11.01 s**, changing Span from
1900 to 1850 mm and WingletHeight from 85 to 95 mm together. One recompute;
zero model calls; saved native snapshot and updated viewport. These are different
operations under changing host load, not an equivalent-work speedup ratio.
This improvement targets follow-up edits, not the first build's kernel cost.

Native negative controls reject stale revisions and unsupported property edits;
the session and original native-file hash remain unchanged. Other agents/user
edits to parameters invalidate the next request instead of being overwritten.
These checks concern software edit integrity, not collision/fit/flight validity.
STEP and engineering checks are explicitly not updated. There is no distributed
locking or general arbitrary-script API in this adaptation.

The first edit launcher exited before submission because its shell process group
ended; it produced no edit evidence. The detached second invocation above is the
measured run. The production overlay already manages the runner's lifetime.

`~/Downloads/VTOL-edit-prompt.txt` contains a ready-to-paste follow-up bound to the
measured live session. After closing FreeCAD, start a new demo and use its cad
folder in WRIGHT_EDIT_SESSION. Keep the fixed initial-build prompt unchanged.

## Final validation

31 project-runner tests passed. `cad/freecad/test_live_edits_native.py` passed
real FreeCAD stale-revision and unsupported-edit controls, plus a deliberately
forced post-recompute failure proving transaction rollback restores both the
parameters and a dependent solid. The original file and session revision remain
unchanged. The timing-test GUI process subsequently exited; an attempted later
negative control against that closed session produced no result and is not
counted as evidence. The isolated native controls above are the verified result.
The client now reports a closed session explicitly. The Downloads edit example
must be updated to a new live run path after that session is closed.
