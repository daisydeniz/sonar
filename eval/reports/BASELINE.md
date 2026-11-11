# Accepted baseline metrics

Written by `sonar eval --write-baseline`; `sonar eval --check` fails when a tracked metric drops by more than 1 point (or a count that must stay at 0 rises). The JSON block at the end is what `--check` reads; edit the numbers only by re-running the eval.

## fixtures

120 files, generated (sonar fixtures, default seed), revision 8587d00 (2026-11-11)

| Metric | Value | Target |
|---|---:|---|
| Loop/one-shot accuracy (%) | 100.00 | >= 95 |
| BPM accuracy, +-1 BPM, loops (%) | 97.06 | >= 90 |
| One-shots given a BPM (count) | 0 | 0 |
| Key exact (%) | 100.00 | report only |
| Key MIREX weighted (%) | 100.00 | >= 80 |
| Note exact pitch class (%) | 100.00 | >= 90 |
| Note exact MIDI (%) | 100.00 | >= 90 |
| Unexpected decode errors (count) | 0 | 0 |

```json
{
  "schema_version": 1,
  "tiers": {
    "fixtures": {
      "date": "2026-11-11",
      "git_sha": "8587d00",
      "files": 120,
      "source": "generated (sonar fixtures, default seed)",
      "metrics": {
        "bpm.accuracy_pct": 97.05882352941177,
        "bpm.one_shots_with_bpm": 0.0,
        "decode.unexpected_errors": 0.0,
        "key.exact_pct": 100.0,
        "key.mirex_pct": 100.0,
        "kind.accuracy_pct": 100.0,
        "note.midi_pct": 100.0,
        "note.pitch_class_pct": 100.0
      }
    }
  }
}
```
