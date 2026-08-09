<!-- wisent-banner:start -->
<p align="center">
  <img src="assets/readme-banner.webp" alt="probierz by Wisent" width="100%">
</p>
<!-- wisent-banner:end -->

<!-- wisent-readme-signals:start -->
[![Source](https://img.shields.io/badge/GitHub-Source-181717?logo=github)](https://github.com/wisent-ai/probierz) [![Issues](https://img.shields.io/badge/GitHub-Issues-181717?logo=github)](https://github.com/wisent-ai/probierz/issues) [![Wisent](https://img.shields.io/badge/Wisent-Website-0B0B0B)](https://wisent.ai) [![Discord](https://img.shields.io/badge/Discord-Join-5865F2?logo=discord&logoColor=white)](https://discord.gg/qRjpkthq54) [![LinkedIn](https://img.shields.io/badge/LinkedIn-Follow-0A66C2?logo=linkedin&logoColor=white)](https://www.linkedin.com/company/wisent-ai/) [![X](https://img.shields.io/badge/X-Follow-000000?logo=x&logoColor=white)](https://x.com/wisentai) [![Enterprise](https://img.shields.io/badge/Enterprise-Book%20a%20call-0B0B0B?logo=calendly)](https://calendly.com/lbartoszcze)
<!-- wisent-readme-signals:end -->

# Probierz

Cross-platform test automation toolkit. One TypeScript/Node monorepo (npm
workspaces) covering **web**, **mobile (iOS + Android)** and **desktop (Electron
+ native macOS/Windows)**.

| Target | Tool | Package |
|--------|------|---------|
| Web (Chromium / Firefox / WebKit + emulated mobile) | Playwright | `packages/web` |
| Desktop — Electron | Playwright (`_electron`) | `packages/electron` |
| Mobile — iOS / Android | WebdriverIO + Appium (XCUITest / UiAutomator2) | `packages/mobile` |
| Desktop — native macOS / Windows | WebdriverIO + Appium Mac2 / WinAppDriver | `packages/desktop-native` |

## Setup

```bash
npm run setup     # installs deps + Playwright browsers
```

Mobile/native also need platform SDKs: Xcode + simulators (iOS), Android SDK +
emulator (Android), and WinAppDriver + Developer Mode (Windows native).

## Running

```bash
# Web — point at any site
BASE_URL=https://example.com npm run test:web

# Electron — defaults to the bundled sample app; override with your own entry
ELECTRON_APP_MAIN=/path/to/app/main.js npm run test:electron

# Mobile
APP_ANDROID=/abs/path/app.apk npm run test:mobile:android
APP_IOS=/abs/path/App.app   npm run test:mobile:ios
# or test an installed app:  APP_PACKAGE=com.your.app  /  BUNDLE_ID=com.your.app

# Native desktop
MAC_BUNDLE_ID=com.apple.TextEdit npm run test:desktop:mac
WIN_APP='Microsoft.WindowsCalculator_8wekyb3d8bbwe!App' npm run test:desktop:win
```

Appium servers for the WebdriverIO suites start automatically (except Windows,
where you run WinAppDriver yourself on `127.0.0.1:4723`).

## Agent surface (read-only)

Probierz ships a read-only agent surface — a CLI and a stdio MCP server — that
discovers surfaces and specs and emits run commands, without ever executing a
suite (a live run needs Chromium/Appium/a simulator and stays a manual step).
Both are backed by one source of truth, `agent/lib.mjs`.

```bash
probierz list                 # the four surfaces + tool, npm script, targets, env
probierz specs [surface]      # e2e/spec files discovered on disk
probierz describe <spec>      # static outline (describe/it titles) of a spec
probierz cmd <target>         # the exact shell command to run a target yourself

probierz-mcp                  # the stdio JSON-RPC MCP server (node agent/mcp.mjs)
```

MCP tools: `probierz_list_surfaces`, `probierz_list_specs`,
`probierz_describe_spec`, `probierz_run_command`. The surface is federated into
the ecosystem aggregator `las`; see `skills/probierz/SKILL.md`.

## Layout

```
packages/
  web/            Playwright config + example web specs
  electron/       Playwright Electron config + sample app fixture
  mobile/         WDIO configs (ios/android) + Appium service + smoke spec
  desktop-native/ WDIO configs (mac/win) + smoke spec
agent/            read-only agent surface: lib.mjs (source of truth) + cli.mjs + mcp.mjs
skills/probierz/  SKILL.md — how agents should use probierz
apps/             drop your .app / .apk / .ipa / .exe here (gitignored)
```

Each package ships a runnable example/smoke test and is configured entirely via
environment variables — no hardcoded targets. Fill in selectors in the
`test/specs/*.e2e.ts` files for your own app.

## Project status and support

Probierz is public development source. Local execution, the evidence contract, and gate evaluation are available under the Apache License 2.0. No stable hosted service or supported public binary release is currently promised.

- Source and issues: [`wisent-ai/probierz`](https://github.com/wisent-ai/probierz)
- Security reports: [private GitHub Security Advisory](https://github.com/wisent-ai/probierz/security/advisories/new)
- License: Apache License 2.0; see [`LICENSE`](LICENSE)