export interface EnsureTechnicalAccountResult {
  created: boolean;
  userId: string;
  emailHash: string;
}

export interface WaitForOtpOptions {
  after?: Date | string | number;
  timeoutMs?: number;
}

export function ensureTechnicalAccount(
  source?: NodeJS.ProcessEnv,
): Promise<EnsureTechnicalAccountResult>;

export function waitForOtp(
  options?: WaitForOtpOptions,
  source?: NodeJS.ProcessEnv,
): Promise<string>;
