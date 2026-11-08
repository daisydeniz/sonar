---
description: Run one iteration of the autonomous build loop (pick next ROADMAP task, delegate, check, review, eval, commit).
---
Run exactly one iteration of "The loop" defined in CLAUDE.md.

1. Read docs/STATUS.md and docs/ROADMAP.md. If `Loop state` is
   `ROADMAP COMPLETE`, say so and stop.
2. Select the task (or the task id given here: $ARGUMENTS).
3. Execute steps 2–7 of the loop: plan, delegate to the owner subagent(s),
   `scripts/check.sh` green, reviewer APPROVE, eval if analysis/search
   changed, update ROADMAP + STATUS, commit on the task branch, then merge
   per the Git section of CLAUDE.md (ff-merge to main, push, milestone tag).
   Do not ask for approval at any point.
4. End your reply with one line:
   `RESULT: DONE <task-id>` or `RESULT: SKIPPED <task-id> <reason>` or
   `RESULT: BLOCKED <reason>` or `RESULT: COMPLETE`.
