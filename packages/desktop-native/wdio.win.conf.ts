import { shared } from './wdio.shared.conf';
import type { Options } from '@wdio/types';

/**
 * Windows native apps via WinAppDriver (Windows only).
 * Prerequisites: install WinAppDriver and run it on 127.0.0.1:4723, enable
 * Windows Developer Mode. Set WIN_APP to the AppId / path to the .exe.
 *   e.g. WIN_APP=Microsoft.WindowsCalculator_8wekyb3d8bbwe!App
 * WinAppDriver is started separately (it is not an Appium driver), so no
 * appium service is registered here.
 */
export const config: Options.Testrunner = {
  ...shared,
  hostname: '127.0.0.1',
  port: 4723,
  path: '/',
  capabilities: [
    {
      platformName: 'windows',
      app: (() => {
        // The app under test is the caller's to state; none is picked for them.
        const app = process.env.WIN_APP;
        if (!app) {
          throw new Error('WIN_APP is required: set it to the AppId or .exe path of the app to test, e.g. Microsoft.WindowsCalculator_8wekyb3d8bbwe!App');
        }
        return app;
      })(),
      'appium:deviceName': 'WindowsPC',
    },
  ],
} as Options.Testrunner;
