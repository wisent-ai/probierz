# Pipeline: Tama (reguły) → probierz (dowody) → stado (compute)

Jeden obraz dla użytkownika: feature ląduje w repo, hooki pilnują zapisu,
`find-violations` audituje istniejący stan, `tama clean` naprawia dług,
probierz pisze i odpala testy, a gate wpuszcza na main tylko zielone.

```
commit → hooki Tama (write-time) → find-violations/clean (hygiene)
→ PUSH → prepush-gate: affected → probierz ci (e2e) → receipt E2/E3
→ zielono: main; czerwono: blokada z powodami
```

## Komendy użytkownika

| Co | Komenda |
|----|---------|
| Audyt repo pod reguły | `node hooks-rotator/src/cli.mjs find-violations --repo <path> [--tree dir] [--owner gh] [--me] [--json]` |
| Naprawa przez agenta | `node hooks-rotator/src/cli.mjs clean --repo <path> [--model codex|kimi] [--dry-run]` |
| Pokrycie journeys apki | `probierz status <appId> [--text]` |
| Całość naraz | `probierz overview [--text]` |
| Autonomiczny manifest | `probierz author-manifest <appId> --desc <co robi> --repo <path> --target <t> --specs` |
| Autonomiczny spec | `probierz author-spec <appId> <journey> --target <t> --desc <cel>` |
| Zdalny run | `probierz stado run <target> --app <id> --host stado:gcp|azure|aws|any|spot|local|t4` |
| Flota | `probierz hosts`, `wc status`, `deploy/stado-up.sh <target>` |

## Gate: instalacja i tryby

- `probierz gate-install <appId> --repo <path>` — hook pre-push (łańcuchuje istniejący; backup jako `pre-push.before-probierz-gate`).
- Domyślnie hook odpala `--ci` (świeży dowód przy pushu). `PROBIERZ_GATE_NO_CI=1` = evaluate-only.

## Runbook

**Gate blokuje push** — `probierz status <appId> --text` pokaże powody (stale/untested/E-za-niski/identity mismatch).
Naprawa: `probierz ci <base>` (świeży dowód), potem push ponownie.

**Flota leży (0 running)** — `wc status` pokaże `0 running, N queued`. Cloud Function tickuje, ale nie ma konsumenta z pojemnością.
Naprawa: `deploy/stado-up.sh <target>` (lokalny agent przez launchd, logi `~/.stado/logs/`). CPU-joby wymagają local-kind agenta; GPU-joby — wolnego slotu accel w GCF (`--gpu-type nvidia-tesla-t4 --spot`).

**Puszczono spec-gaming (np. asercja na błąd harnessa)** — `tama clean` ma detektor rund (odrzuca `.source`, data:-URL, rename-dodge). W author-spec brief TUI zakazuje asercji na błędy launcha; w razie czego kasujemy spec ręcznie i przeprowadzamy rundę ponownie.

**Konfiguracja stado** — `stado.config.json` (`wc config init|show|validate`): backend stanu (`gcs|azure|s3|local`), providerzy, regiony, projekt. Kolejność: env > plik > default.

**Rejestr floty** — `gs://wisent-compute/registry.json` (targets: kind local/gcp, hostnames znormalizowane lowercase, slots, opcjonalnie disk_cleanup z `mode: off`).

## Nightly

`com.probierz.nightly` (launchd, 03:17) — `deploy/nightly-probierz.sh`: find-violations na kluczowych repo + overview do `~/.stado/nightly/nightly.log`. Repo listę zmienia `PROBIERZ_NIGHTLY_REPOS`.

## Stan na 2026-07-24

- Gate zainstalowany: tama-desktop, oko, skarbiec, jeden, hooks-rotator.
- Agent lokalny: `lukasz-macbook` (launchd, broadcasting do `gs://stado/capacity/`).
- Blockery: desktop:mac (Accessibility dla Mac2), oko backend (`OKO_E2E_*`), dispatch floty GPU (0 wolnych slotów lokalnie; spot+t4 w kolejce).
