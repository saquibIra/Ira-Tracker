import { defineConfig, devices } from '@playwright/test';

/**
 * End-to-end tests of the Time Pilot onboarding, run against the local stack
 * (server/local) by default:
 *
 *   TP_SUPERADMIN_EMAIL=... TP_SUPERADMIN_PASSWORD=... npx playwright test
 *
 * TP_BASE_URL, TP_AUTOTRACK_URL and TP_MAILPIT_URL point it elsewhere.
 */
export default defineConfig({
    testDir: '.',
    timeout: 120_000,
    expect: { timeout: 15_000 },
    workers: 1,
    reporter: [['list']],
    use: {
        baseURL: process.env.TP_BASE_URL ?? 'http://localhost:8000',
        screenshot: 'only-on-failure',
        trace: 'retain-on-failure',
    },
    projects: [{ name: 'chromium', use: { ...devices['Desktop Chrome'] } }],
});
