---
description: Run evaluation and compare with baseline.
---
Delegate to the `eval` subagent: build the CLI in release mode, run
`sonar eval --fixtures`, plus `scripts/eval-real.sh` (silver `Samples/`,
golden `eval/golden/`; `--silver $ARGUMENTS` if a directory is given). Summarize metrics vs
eval/reports/BASELINE.md and flag regressions per docs/EVAL.md.
