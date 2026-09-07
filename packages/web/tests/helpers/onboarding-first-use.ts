import { type Locator, type Page, type TestInfo } from '@playwright/test';

export function requireEnvironment(name: string): string {
  const value = process.env[name]?.trim();
  if (!value) throw new Error(`${name} is required for this real first-use journey`);
  return value;
}

export function requireProductBaseUrl(sourceName: string): URL {
  const raw = requireEnvironment(sourceName);
  let url: URL;
  try {
    url = new URL(raw);
  } catch {
    throw new Error(`${sourceName} must be an absolute URL`);
  }
  const loopback = ['localhost', '127.0.0.1', '::1', '[::1]'].includes(url.hostname);
  if ((url.protocol !== 'https:' && !(loopback && url.protocol === 'http:'))
    || url.username || url.password || url.search || url.hash) {
    throw new Error(`${sourceName} must be a credential-free HTTPS URL (HTTP is allowed only for loopback)`);
  }
  return url;
}

export function chromiumPublicationOnly(browserName: string, testInfo: TestInfo): void {
  testInfo.skip(browserName !== 'chromium', 'The release-bound publication journey records one canonical Chromium evidence bundle');
}

export async function clearProductStorage(page: Page, prefixes: string[]): Promise<void> {
  await page.evaluate((ownedPrefixes) => {
    for (const key of Object.keys(localStorage)) {
      if (ownedPrefixes.some((prefix) => key.startsWith(prefix))) localStorage.removeItem(key);
    }
  }, prefixes);
}

export async function journeyProgress(page: Page, prefix: string): Promise<Record<string, unknown>> {
  return page.evaluate((ownedPrefix) => {
    const key = Object.keys(localStorage).find((candidate) => candidate.startsWith(`${ownedPrefix}.progress.`));
    if (!key) throw new Error(`No persisted journey progress for ${ownedPrefix}`);
    const parsed = JSON.parse(localStorage.getItem(key) || 'null');
    if (!parsed || typeof parsed !== 'object') throw new Error(`Invalid persisted journey progress for ${ownedPrefix}`);
    return parsed;
  }, prefix);
}

export function requireJsonObject(name: string): Record<string, string | number | boolean | string[]> {
  const raw = requireEnvironment(name);
  let value: unknown;
  try {
    value = JSON.parse(raw);
  } catch {
    throw new Error(`${name} must be a JSON object`);
  }
  if (!value || Array.isArray(value) || typeof value !== 'object') throw new Error(`${name} must be a JSON object`);
  return value as Record<string, string | number | boolean | string[]>;
}

export async function fillAccessibleControl(
  page: Page,
  label: string,
  value: string | number | boolean | string[],
): Promise<void> {
  const candidates = page.getByLabel(label, { exact: true });
  const visible: Locator[] = [];
  for (let index = 0; index < await candidates.count(); index += 1) {
    const candidate = candidates.nth(index);
    if (await candidate.isVisible()) visible.push(candidate);
  }
  if (visible.length !== 1) throw new Error(`Expected one visible control labelled ${JSON.stringify(label)}, found ${visible.length}`);
  const control = visible[0];
  const type = await control.getAttribute('type');
  const role = await control.getAttribute('role');
  if (type === 'file') {
    await control.setInputFiles(Array.isArray(value) ? value : [String(value)]);
  } else if (type === 'checkbox' || type === 'radio') {
    if (Boolean(value)) await control.check();
    else await control.uncheck();
  } else if (role === 'combobox') {
    await control.fill(String(value));
    await control.press('ArrowDown');
    await control.press('Enter');
  } else {
    await control.fill(String(value));
  }
}
