# Solver locations for the stages (sim, cfd, fea). Source this from the
# scripts; the stages read the variables. Defaults point at the shared venv
# and tools logged in docs/TODO.md "Downloads log"; override with the env.
# ~/.wright is preferred when it exists (a copy that survives a reboot;
# /private/tmp is cleared at boot).
_wright_scratch="/private/tmp/claude-501/-Users-patliu/e9f305f9-7d6c-4ecb-a4fc-7e5345001431/scratchpad"
if [ -z "${WRIGHT_PYTHON:-}" ]; then
  for p in "$HOME/.wright/venv312/bin/python" "$_wright_scratch/venv312/bin/python"; do
    [ -x "$p" ] && { export WRIGHT_PYTHON="$p"; break; }
  done
fi
if [ -z "${WRIGHT_SU2:-}" ]; then
  for p in "$HOME/.wright/tools/su2/bin/SU2_CFD" "$_wright_scratch/tools/su2/bin/SU2_CFD"; do
    [ -x "$p" ] && { export WRIGHT_SU2="$p"; break; }
  done
fi
# Finder-launched desktop apps may not inherit Homebrew's Azure CLI path.
export PATH="$HOME/.cargo/bin:$HOME/.elan/bin:$HOME/.ghcup/bin:/opt/homebrew/bin:/usr/local/bin:$PATH"
# Runs database (prior efforts): the Azure Cosmos DB account from
# docs/azure-db.md. The token comes from the signed-in Azure CLI (`az login`);
# when az is missing or signed out, wright falls back to data/runs-db.jsonl.
# Set WRIGHT_RUNS_URI=off (or a .jsonl path) before sourcing to override.
export WRIGHT_RUNS_URI="${WRIGHT_RUNS_URI:-cosmos://wright-runs-pl4676/wright/runs}"
# Private CAD / PCB / manufacturing artifacts, authorized through `az login`.
# No account key or SAS token is stored here. Set WRIGHT_BLOB_URL=off to disable.
export WRIGHT_BLOB_URL="${WRIGHT_BLOB_URL:-https://wrightfilesfb102aa8.blob.core.windows.net/wright-runs}"
