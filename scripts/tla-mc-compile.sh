#!/bin/bash
# sweep.py's binary for the compiled leg of the gate: tla-mc -compile, the
# cache in $TLAMC_CACHE (set by the workflow; defaults to ~/.cache/tla-mc)
exec "${TLAMC:?set TLAMC to the tla-mc binary}" -compile "$@"
