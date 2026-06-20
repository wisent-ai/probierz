# wisent-tester

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

## Layout

```
packages/
  web/            Playwright config + example web specs
  electron/       Playwright Electron config + sample app fixture
  mobile/         WDIO configs (ios/android) + Appium service + smoke spec
  desktop-native/ WDIO configs (mac/win) + smoke spec
apps/             drop your .app / .apk / .ipa / .exe here (gitignored)
```

Each package ships a runnable example/smoke test and is configured entirely via
environment variables — no hardcoded targets. Fill in selectors in the
`test/specs/*.e2e.ts` files for your own app.
