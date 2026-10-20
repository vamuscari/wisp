#!/usr/bin/env bash
set -euo pipefail

node_path=$1
probe_path=$2
request=$3
export WISP_MUX_BASH_PROBE=1
"$node_path" "$probe_path" request "$request"
exit $?
