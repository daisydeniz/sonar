---
description: Run evaluation and compare with baseline.
---
Delegate to the `eval` subagent: build the CLI in release mode, run
`sonar eval --fixtures`, plus `--silver $ARGUMENTS` if a directory is given
and `--golden` if $SONAR_GOLDEN_DIR is set. Summarize metrics vs
eval/reports/BASELINE.md and flag regressions per docs/EVAL.md.
