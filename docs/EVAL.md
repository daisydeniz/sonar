# Evaluation

Search quality is the product. Every analysis or ranking change is measured.

## Data tiers
| Tier | Source | In git? | Use |
|---|---|---|---|
| **Fixtures** | `sonar fixtures` (synthetic, seeded) | Generator only | Exact truth for DSP (BPM, key, pitch, loop/one-shot, envelope). Runs in `check.sh`. |
| **Silver** | Real files whose names carry labels (`..._124bpm_Fmin_...`, `Kick_...`) | No | Large, noisy labels for BPM/key/category. Filename is hidden from the analyzer during eval. |
| **Golden** | ~300 hand-labeled real samples from the founder's library (`SONAR_GOLDEN_DIR`) | No (only `eval/golden/*.example.*` templates) | Final authority for category, similarity and text queries. |

Golden files (local, in `$SONAR_GOLDEN_DIR`):
- `labels.csv`: `path,category,kind,bpm,key,note,notes` (empty = unknown)
- `similar.jsonl`: `{"query": "path", "relevant": ["path", ...]}`
- `text_queries.jsonl`: `{"q": "dark metallic kick", "relevant": ["path", ...]}`

Templates: `eval/golden/*.example.*`.

## Metrics
| Area | Metric | Target |
|---|---|---|
| Loop/one-shot | accuracy | ≥ 95 % |
| BPM (loops) | acc ±1 BPM; octave-error rate reported separately | ≥ 90 % |
| Key | exact + MIREX weighted score | ≥ 80 % weighted |
| Note (tonal one-shots) | exact pitch class / exact MIDI | ≥ 90 % |
| Category | top-1 accuracy, macro-F1, confusion matrix | ≥ 90 % top-1 |
| Similarity | P@10, MRR | P@10 ≥ 0.7 |
| Text / NL search | NDCG@10, recall@50 | track, no regressions |
| Latency | search p50/p95 at 500k docs | p95 < 50 ms |
| Throughput | scan files/s, analysis files/s | ≥ 2,000 / ≥ 100 |

## Running
```
sonar eval --fixtures            # always; part of check.sh
sonar eval --silver <dir>        # real files with labeled names
sonar eval --golden              # uses $SONAR_GOLDEN_DIR
```
Reports: `eval/reports/<date>-<gitsha>.{json,md}` (gitignored except
`eval/reports/BASELINE.md`, which records the current accepted numbers).

## Regression rule
A change that lowers any tracked metric by more than 1 point (or raises p95
latency by > 10 %) must be reverted or explicitly accepted with an ADR.
`sonar eval --fixtures --check` enforces this for fixtures inside
`scripts/check.sh` (non-zero exit on regression). BASELINE.md therefore
needs a machine-readable block (e.g. a fenced JSON section) next to the
human-readable table.
