#!/bin/sh
set -eu
exec python3 -B scripts/lint_rules.py "$@"
