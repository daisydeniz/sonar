# Sonar (kod adı)

Büyük yerel sample kütüphanelerini indeksleyen ve anında aranabilir yapan macOS uygulaması:
metin, filtreler, **ses benzerliği** ve **doğal dille arama**. Referans: Sononym.

Bu repo, uygulamayı Claude Code ile **otonom** geliştiren bir ajan sistemi olarak kuruldu.

## Yapı
| Yol | İçerik |
|---|---|
| `CLAUDE.md` | Orkestratör (tech lead) talimatları ve döngü |
| `.claude/agents/` | Alt ajanlar: architect, rust-core, dsp-analysis, search-ranking, swift-ui, eval, reviewer |
| `.claude/commands/` | `/next` (bir görev), `/status` (Türkçe özet), `/eval` |
| `.claude/settings.json` | İzinler (otonom çalışma için allowlist + yasaklar) |
| `docs/` | PRODUCT, ARCHITECTURE, DECISIONS (ADR), ROADMAP (görev kuyruğu), EVAL, STATUS |
| `core/` | Rust çekirdek (workspace: `sonar-core`, `sonar-cli`) |
| `app/` | macOS uygulaması (T0.4'te oluşturulacak) |
| `scripts/` | `bootstrap.sh`, `check.sh`, `autopilot.sh` |

## Başlatma (MacBook)
```bash
git clone <repo> sonar && cd sonar
scripts/bootstrap.sh          # Xcode, rustup, xcodegen, cargo-deny, claude kontrol/kurulum
scripts/check.sh              # tüm kalite kapıları
scripts/autopilot.sh          # otonom döngü (her görev = yeni Claude oturumu)
```
İzlemek için: `docs/STATUS.md`, `git log`, `logs/`. Özet için Claude Code içinde `/status`.

Durdurma: `Ctrl-C`. Tek görev: `claude` → `/next` (veya `/next T1.2`).

## Senden beklenenler
`docs/STATUS.md` → "Needs human": golden set etiketleme (T3.7), Ableton testi (T6.10),
Apple Developer / ödeme / fiyat (T7.4).
