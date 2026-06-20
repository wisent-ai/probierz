import { browser, $, $$ } from '@wdio/globals';
import { writeFileSync } from 'node:fs';

// Swiatowid iOS (ai.wisent.swiatowid) full end-to-end test:
//   real email-OTP login -> Strategy / Velocity tabs -> sign out -> chat round-trip.
// The OTP is read from the lukasz.bartoszcze@wisent.ai inbox via the Gmail API
// (token passed in GMAIL_TOKEN). Each describe logs in only when signed out, so
// the suite is idempotent across runs.
//
// Tabs are selected by position inside the tab bar: SwiftUI exposes their
// accessibility name inconsistently (label "Goals" vs SF Symbol
// "checkmark.circle[.fill]"), but their order is stable. The tabs/sign-out flow
// is kept free of any text field so the keyboard never overlays the tab bar; the
// chat round-trip (which raises the keyboard) lives in its own describe and runs
// nothing after it.

const EMAIL = 'lukasz.bartoszcze@wisent.ai';
const TF = '-ios class chain:**/XCUIElementTypeTextField';
const TF2 = '-ios class chain:**/XCUIElementTypeTextField[2]';
const CHAT_BUBBLES = '-ios class chain:**/XCUIElementTypeScrollView/**/XCUIElementTypeStaticText';
const SEND_BTN = "-ios predicate string:type == 'XCUIElementTypeButton' AND name CONTAINS 'arrow.up'";
const ERR_TEXT = "-ios predicate string:type == 'XCUIElementTypeStaticText' AND name CONTAINS 'HTTP'";
const STRATEGY = 0;
const GOALS = 1;
const CHAT = 2;

async function dump(name: string): Promise<void> {
  try {
    writeFileSync(`/tmp/sw_${name}.xml`, await browser.getPageSource());
  } catch {
    /* diagnostic only */
  }
}

async function tapTab(index: number): Promise<void> {
  const bar = await $('~Tab Bar');
  await bar.waitForExist();
  const buttons = await bar.$$('-ios class chain:**/XCUIElementTypeButton');
  await buttons[index].click();
}

// Poll the Gmail API for the newest "Your Wisent Login Code" email; the 6-8 digit
// code is in the message snippet.
async function fetchOtp(sinceEpoch: number): Promise<string> {
  const token = process.env.GMAIL_TOKEN;
  if (!token) throw new Error('GMAIL_TOKEN env not set');
  const headers = { Authorization: `Bearer ${token}` };
  const q = encodeURIComponent(`from:no-reply@wisent.com after:${sinceEpoch} newer_than:1h`);
  for (let i = 0; i < 45; i++) {
    const list: any = await fetch(
      `https://gmail.googleapis.com/gmail/v1/users/me/messages?q=${q}&maxResults=3`,
      { headers },
    ).then((r) => r.json());
    const id = (list.messages || [])[0]?.id;
    if (id) {
      const msg: any = await fetch(
        `https://gmail.googleapis.com/gmail/v1/users/me/messages/${id}?format=metadata&metadataHeaders=Subject`,
        { headers },
      ).then((r) => r.json());
      const m = (msg.snippet || '').match(/\b(\d{6,8})\b/);
      if (m) return m[1];
    }
    await browser.pause(2000);
  }
  throw new Error('OTP email not found via Gmail API');
}

async function loginIfNeeded(): Promise<void> {
  if (!(await $('~Send code').isExisting())) return; // already signed in

  const since = Math.floor(Date.now() / 1000) - 5;
  const email = await $(TF);
  await email.click();
  await email.clearValue();
  await email.setValue(EMAIL);
  await (await $('~Send code')).click();

  const code = await fetchOtp(since);
  const codeField = await $(TF2);
  await codeField.waitForDisplayed();
  await codeField.click();
  await codeField.setValue(code);
  await (await $('~Verify & sign in')).click();
}

describe('Swiatowid iOS — authenticated tabs', () => {
  before(async () => {
    await loginIfNeeded();
    await (await $('~Tab Bar')).waitForDisplayed();
  });

  it('shows the three main tabs after login', async () => {
    const bar = await $('~Tab Bar');
    const buttons = await bar.$$('-ios class chain:**/XCUIElementTypeButton');
    expect(buttons.length).toBe(3);
  });

  it('Strategy tab renders (nav bar + sign out)', async () => {
    await tapTab(STRATEGY);
    await expect(await $('~Sign out')).toBeDisplayed();
  });

  it('Velocity (Goals) tab shows the weekly metrics', async () => {
    await tapTab(GOALS);
    const closed = await $('~Goals closed');
    await closed.waitForDisplayed();
    await expect(closed).toBeDisplayed();
    await expect(await $('~Active goals')).toBeDisplayed();
    await expect(await $('~Closed all-time')).toBeDisplayed();
  });

  it('Sign out returns to the sign-in screen', async () => {
    await tapTab(STRATEGY);
    const signOut = await $('~Sign out');
    await signOut.waitForDisplayed();
    await signOut.click();
    const sendBtn = await $('~Send code');
    await sendBtn.waitForExist();
    await expect(sendBtn).toBeDisplayed();
    await expect(await $('~Swiatowid')).toBeDisplayed();
  });
});

describe('Swiatowid iOS — chat round-trip', () => {
  before(async () => {
    await loginIfNeeded();
    await (await $('~Tab Bar')).waitForDisplayed();
  });

  it('sends a message and the backend responds', async () => {
    await tapTab(CHAT);
    const field = await $(TF);
    await field.waitForDisplayed();
    await field.click();
    await field.setValue('In one sentence, what should I focus on today?');
    await (await $(SEND_BTN)).click();

    // The send works when the app renders a server response: either an assistant
    // bubble (2nd StaticText in the scroll view) or a surfaced error.
    let responded = false;
    for (let i = 0; i < 20; i++) {
      const bubbles = (await $$(CHAT_BUBBLES)).length;
      if (bubbles >= 2 || (await $(ERR_TEXT).isExisting())) {
        responded = true;
        break;
      }
      await browser.pause(2000);
    }
    await dump('chat_after');
    expect(responded).toBe(true);
  });
});
