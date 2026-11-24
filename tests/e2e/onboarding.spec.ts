/**
 * The whole Time Pilot onboarding, as people do it:
 * Super Admin -> organization + admin email -> admin sets a password -> project and
 * employee invitation -> employee sets a password -> desktop app signs in through the
 * browser (OAuth 2.0 + PKCE) and reaches the Autotrack service.
 */
import { test, expect, type Page, type BrowserContext } from '@playwright/test';
import { createHash, randomBytes } from 'node:crypto';
import { createServer } from 'node:http';
import type { AddressInfo } from 'node:net';
import { latestSubject, mailpit } from './lib/mail';

const run = Date.now().toString(36);
const ORG = `Acme ${run}`;
const ADMIN_EMAIL = `tp-admin-${run}@mailinator.com`;
const EMPLOYEE_EMAIL = `tp-employee-${run}@mailinator.com`;
const PASSWORD = `Pilot-${run}-Runway!9`;
const PROJECT = `Website Redesign ${run}`;
const AUTOTRACK = process.env.TP_AUTOTRACK_URL ?? 'http://localhost:8010';
const SCOPES = 'create read update delete';
const mail = mailpit();

type State = Awaited<ReturnType<BrowserContext['storageState']>>;
let welcomeLink = '';
let inviteLink = '';
let adminState: State;
let employeeState: State;

test.describe.configure({ mode: 'serial' });

const b64url = (b: Buffer) => b.toString('base64url');

/** What the desktop app does: listen on 127.0.0.1 (random port) for the browser to return. */
async function loopback() {
    let resolve!: (u: URL) => void;
    const returned = new Promise<URL>((r) => (resolve = r));
    const server = createServer((req, res) => {
        const url = new URL(req.url ?? '/', 'http://127.0.0.1');
        res.writeHead(200, { 'Content-Type': 'text/plain' }).end('signed in');
        if (url.pathname === '/callback') resolve(url);
    });
    await new Promise<void>((r) => server.listen(0, '127.0.0.1', r));
    const port = (server.address() as AddressInfo).port;
    return { redirect: `http://127.0.0.1:${port}/callback`, returned, close: () => server.close() };
}

/** Browser sign-in of the desktop client for a signed-in browser context; returns code and verifier. */
async function desktopAuthorize(ctx: BrowserContext, clientId: string) {
    const verifier = b64url(randomBytes(48));
    const state = b64url(randomBytes(16));
    const lb = await loopback();
    const page = await ctx.newPage();
    const query = new URLSearchParams({
        response_type: 'code',
        client_id: clientId,
        redirect_uri: lb.redirect,
        scope: SCOPES,
        state,
        code_challenge: b64url(createHash('sha256').update(verifier).digest()),
        code_challenge_method: 'S256',
    });
    await page.goto(`/oauth/authorize?${query}`);
    // Time Pilot's own app skips the consent screen: the browser comes straight back.
    const url = await lb.returned;
    lb.close();
    expect(url.searchParams.get('state')).toBe(state);
    const code = url.searchParams.get('code');
    expect(code).toBeTruthy();
    return { code: code!, verifier, redirect: lb.redirect };
}

async function logIn(page: Page, email: string, password: string) {
    await page.goto('/login');
    await page.getByLabel('Email').fill(email);
    await page.getByLabel('Password').fill(password);
    await page.getByRole('button', { name: 'Log in' }).click();
    await page.waitForURL((url) => !url.pathname.startsWith('/login'));
}

test('sign-in page is Time Pilot and public sign-up is closed', async ({ page, request }) => {
    await page.goto('/login');
    await expect(page).toHaveTitle(/Time Pilot/);
    await expect(page.getByText('Time tracking that keeps up with your team.')).toBeVisible();
    await expect(page.getByText("Ask your organization's admin for an invitation")).toBeVisible();
    expect(await page.locator('body').innerText()).not.toMatch(/solidtime/i);
    const register = await request.get('/register', { maxRedirects: 0 });
    expect(register.status()).not.toBe(200);
    const source = await request.get('/source', { maxRedirects: 0 });
    expect(source.status()).toBe(302);
    expect(source.headers()['location']).toContain('github.com');
});

test('Super Admin creates an organization with its admin', async ({ page }) => {
    const email = process.env.TP_SUPERADMIN_EMAIL;
    const password = process.env.TP_SUPERADMIN_PASSWORD;
    test.skip(!email || !password, 'set TP_SUPERADMIN_EMAIL and TP_SUPERADMIN_PASSWORD');
    const since = Date.now();
    await logIn(page, email!, password!);
    await page.goto('/admin/organizations');
    await page.getByRole('button', { name: 'New organization' }).click();
    await page.getByLabel('Organization name').fill(ORG);
    await page.getByLabel('Admin name').fill('Alice Admin');
    await page.getByLabel('Admin email').fill(ADMIN_EMAIL);
    await page.getByRole('button', { name: 'Create and send email' }).click();
    await expect(page.getByText('Organization created')).toBeVisible();
    welcomeLink = await mail.link(ADMIN_EMAIL, /\/welcome\//, since);
    expect(await latestSubject(ADMIN_EMAIL)).toBe(`${ORG} is ready on Time Pilot`);
});

test('the admin sets a password from the email, once', async ({ browser }) => {
    test.skip(!welcomeLink, 'needs the previous step');
    const ctx = await browser.newContext();
    const page = await ctx.newPage();
    await page.goto(welcomeLink);
    await expect(page.getByRole('heading', { name: 'Welcome to Time Pilot' })).toBeVisible();
    await expect(page.getByText(`You are the admin of ${ORG}`)).toBeVisible();
    await page.getByLabel('Password', { exact: true }).fill(PASSWORD);
    await page.getByLabel('Confirm password').fill(PASSWORD);
    await page.getByRole('button', { name: 'Set password and continue' }).click();
    await expect(page).toHaveURL(/\/dashboard/);
    await expect(page.getByText('Your organization is ready')).toBeVisible();
    adminState = await ctx.storageState();
    await ctx.close();

    // The link stops working once the password is set.
    const guest = await browser.newContext();
    const again = await guest.newPage();
    await again.goto(welcomeLink);
    await expect(again).toHaveURL(/\/login/);
    await expect(again.getByText('This link has already been used')).toBeVisible();
    // A tampered link is refused.
    const tampered = await again.request.get(welcomeLink.replace(/signature=[0-9a-f]{4}/, 'signature=0000'));
    expect(tampered.status()).toBe(403);
    await guest.close();
});

test('the admin creates a project and invites an employee', async ({ browser }) => {
    test.skip(!adminState, 'needs the previous step');
    const ctx = await browser.newContext({ storageState: adminState });
    const page = await ctx.newPage();
    await page.goto('/projects');
    await page.getByRole('button', { name: 'Create Project' }).first().click();
    await page.getByLabel('Project name').fill(PROJECT);
    await page.getByRole('button', { name: 'Create Project' }).last().click();
    await expect(page.getByText(PROJECT).first()).toBeVisible();

    const since = Date.now();
    await page.goto('/members');
    await page.getByRole('button', { name: 'Invite member' }).click();
    await page.getByPlaceholder('Member Email').fill(EMPLOYEE_EMAIL);
    await page.getByRole('button', { name: /^Employee/ }).click();
    await page.getByRole('button', { name: 'Invite Member' }).last().click();
    inviteLink = await mail.link(EMPLOYEE_EMAIL, /organization-invitations/, since);
    expect(await latestSubject(EMPLOYEE_EMAIL)).toBe(`Join ${ORG} on Time Pilot`);
    await ctx.close();
});

test('the employee joins by setting a password', async ({ browser }) => {
    test.skip(!inviteLink, 'needs the previous step');
    const ctx = await browser.newContext();
    const page = await ctx.newPage();
    await page.goto(inviteLink);
    await expect(page.getByRole('heading', { name: `Join ${ORG}` })).toBeVisible();
    await expect(page.locator('#email')).toHaveValue(EMPLOYEE_EMAIL);
    await page.getByLabel('Your name').fill('Bob Builder');
    await page.getByLabel('Password', { exact: true }).fill(PASSWORD);
    await page.getByLabel('Confirm password').fill(PASSWORD);
    await page.getByRole('button', { name: `Join ${ORG}` }).click();
    await expect(page).toHaveURL(/\/dashboard/);
    await expect(page.getByText(`Welcome to ${ORG}`)).toBeVisible();
    employeeState = await ctx.storageState();
    await ctx.close();
});

test('the desktop app signs in through the browser; a stolen code is useless', async ({ browser, request }) => {
    test.skip(!employeeState, 'needs the previous step');
    const info = await (await request.get(`${AUTOTRACK}/v1/client`)).json();
    expect(info.desktop_client_id).toBeTruthy();
    const ctx = await browser.newContext({ storageState: employeeState });
    const { code, redirect } = await desktopAuthorize(ctx, info.desktop_client_id);
    await ctx.close();
    // Without the app's PKCE verifier the code cannot be exchanged.
    const stolen = await request.post('/oauth/token', {
        form: {
            grant_type: 'authorization_code',
            client_id: info.desktop_client_id,
            redirect_uri: redirect,
            code_verifier: b64url(randomBytes(48)),
            code,
        },
    });
    expect(stolen.status()).toBe(400);
});

test('desktop tokens: exchange, short lifetime, refresh, API and Autotrack access', async ({ browser, request }) => {
    test.skip(!employeeState, 'needs the previous step');
    const info = await (await request.get(`${AUTOTRACK}/v1/client`)).json();
    const ctx = await browser.newContext({ storageState: employeeState });
    const { code, verifier, redirect } = await desktopAuthorize(ctx, info.desktop_client_id);
    await ctx.close();

    const tok = await request.post('/oauth/token', {
        form: { grant_type: 'authorization_code', client_id: info.desktop_client_id, redirect_uri: redirect, code_verifier: verifier, code },
    });
    expect(tok.ok()).toBeTruthy();
    const t = await tok.json();
    expect(t.expires_in).toBeLessThanOrEqual(12 * 3600);
    expect(t.refresh_token).toBeTruthy();

    const auth = { Authorization: `Bearer ${t.access_token}`, Accept: 'application/json' };
    const memberships = (await (await request.get('/api/v1/users/me/memberships', { headers: auth })).json()).data;
    // Invited people belong only to the organization that invited them.
    expect(memberships).toHaveLength(1);
    expect(memberships[0].organization.name).toBe(ORG);
    expect(memberships[0].role).toBe('employee');
    const orgId = memberships[0].organization.id;

    const projects = (await (await request.get(`/api/v1/organizations/${orgId}/projects`, { headers: auth })).json()).data;
    expect(projects.map((p: { name: string }) => p.name)).toContain(PROJECT);

    const cfg = await request.get(`${AUTOTRACK}/v1/config`, { headers: { ...auth, 'X-TimePilot-Org': orgId } });
    expect(cfg.status()).toBe(200);
    expect((await cfg.json()).ai_provider).toBeTruthy();

    const refreshed = await request.post('/oauth/token', {
        form: { grant_type: 'refresh_token', client_id: info.desktop_client_id, refresh_token: t.refresh_token, scope: SCOPES },
    });
    expect(refreshed.ok()).toBeTruthy();
    const r = await refreshed.json();
    expect(r.access_token).not.toBe(t.access_token);
    const again = await request.get('/api/v1/users/me/memberships', {
        headers: { Authorization: `Bearer ${r.access_token}`, Accept: 'application/json' },
    });
    expect(again.status()).toBe(200);
});

test('the admin signs in to the auto-tracking settings through the browser', async ({ browser }) => {
    test.skip(!adminState, 'needs the admin');
    const ctx = await browser.newContext({ storageState: adminState });
    const page = await ctx.newPage();
    await page.goto(`${AUTOTRACK}/admin/`);
    await page.getByRole('button', { name: 'Sign in with Time Pilot' }).click();
    // Back from the sign-in page: the code is gone from the address bar, the projects show.
    await expect(page.getByRole('heading', { name: 'Project hints' })).toBeVisible();
    expect(page.url()).not.toContain('code=');
    await expect(page.getByText(PROJECT)).toBeVisible();
    await ctx.close();
});

test('an employee cannot open the auto-tracking settings', async ({ browser }) => {
    test.skip(!employeeState, 'needs the employee');
    const ctx = await browser.newContext({ storageState: employeeState });
    const page = await ctx.newPage();
    await page.goto(`${AUTOTRACK}/admin/`);
    await page.getByRole('button', { name: 'Sign in with Time Pilot' }).click();
    await expect(page.getByText('You are not an owner or admin of any organization.')).toBeVisible();
    await ctx.close();
});
