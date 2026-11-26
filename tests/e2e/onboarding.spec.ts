/**
 * The whole Time Pilot onboarding, as people do it (email is off, so links are copied):
 * Super Admin -> organization + admin link -> admin sets a password -> project and
 * employee invitation link -> employee sets a password -> desktop app signs in through the
 * browser (OAuth 2.0 + PKCE) and reaches the Autotrack service -> a forgotten password is
 * reset with a link from the Super Admin.
 */
import { test, expect, type Page, type BrowserContext } from '@playwright/test';
import { createHash, randomBytes } from 'node:crypto';
import { createServer } from 'node:http';
import type { AddressInfo } from 'node:net';

const run = Date.now().toString(36);
const ORG = `Acme ${run}`;
const ADMIN_EMAIL = `tp-admin-${run}@example.com`;
const EMPLOYEE_EMAIL = `tp-employee-${run}@example.com`;
const PENDING_EMAIL = `tp-pending-${run}@example.com`;
const PASSWORD = `Pilot-${run}-Runway!9`;
const NEW_PASSWORD = `Pilot-${run}-Glide!7`;
const PROJECT = `Website Redesign ${run}`;
const AUTOTRACK = process.env.TP_AUTOTRACK_URL ?? 'http://localhost:8010';
const SCOPES = 'create read update delete';
const CLIPBOARD = ['clipboard-read', 'clipboard-write'];

type State = Awaited<ReturnType<BrowserContext['storageState']>>;
let welcomeLink = '';
let inviteLink = '';
let pendingLinkApi = ''; // the link endpoint of an invitation nobody accepts
let superAdminState: State;
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

/** The link shown in the Super Admin's "Password link" dialog. */
async function passwordLinkFromDialog(page: Page): Promise<string> {
    const input = page.getByTestId('password-link');
    await expect(input).toHaveValue(/\/welcome\//);
    return input.inputValue();
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
    // No email: a forgotten password is reset with a link from the admin.
    await expect(page.getByText('Forgot your password? Ask your admin.')).toBeVisible();
    expect(await page.locator('body').innerText()).not.toMatch(/solidtime/i);
    const register = await request.get('/register', { maxRedirects: 0 });
    expect(register.status()).not.toBe(200);
    const source = await request.get('/source', { maxRedirects: 0 });
    expect(source.status()).toBe(302);
    expect(source.headers()['location']).toContain('github.com');
});

test('Super Admin creates an organization and gets the admin link', async ({ browser }) => {
    const email = process.env.TP_SUPERADMIN_EMAIL;
    const password = process.env.TP_SUPERADMIN_PASSWORD;
    test.skip(!email || !password, 'set TP_SUPERADMIN_EMAIL and TP_SUPERADMIN_PASSWORD');
    const ctx = await browser.newContext({ permissions: CLIPBOARD });
    const page = await ctx.newPage();
    await logIn(page, email!, password!);
    await page.goto('/admin/organizations');
    // Email is off: nothing offers to send one.
    await expect(page.getByRole('button', { name: 'Resend admin email' })).toHaveCount(0);
    await page.getByRole('button', { name: 'New organization' }).click();
    await page.getByLabel('Organization name').fill(ORG);
    await page.getByLabel('Admin name').fill('Alice Admin');
    await page.getByLabel('Admin email').fill(ADMIN_EMAIL);
    await page.getByRole('button', { name: 'Create organization' }).click();
    await expect(page.getByText('Organization created')).toBeVisible();
    // The link to send to the admin opens right away, with a copy button.
    await expect(page.getByRole('heading', { name: 'Password link for Alice Admin' })).toBeVisible();
    await expect(page.getByText(`Send this link to ${ADMIN_EMAIL} yourself`)).toBeVisible();
    welcomeLink = await passwordLinkFromDialog(page);
    await page.getByRole('button', { name: 'Copy', exact: true }).click();
    expect(await page.evaluate(() => navigator.clipboard.readText())).toBe(welcomeLink);
    await page.getByRole('button', { name: 'Done' }).click();
    superAdminState = await ctx.storageState();
    await ctx.close();
});

test('the admin sets a password from the link, once', async ({ browser }) => {
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
    const ctx = await browser.newContext({ storageState: adminState, permissions: CLIPBOARD });
    const page = await ctx.newPage();
    await page.goto('/projects');
    await page.getByRole('button', { name: 'Create Project' }).first().click();
    await page.getByLabel('Project name').fill(PROJECT);
    await page.getByRole('button', { name: 'Create Project' }).last().click();
    await expect(page.getByText(PROJECT).first()).toBeVisible();

    await page.goto('/members');
    await page.getByRole('button', { name: 'Invite member' }).click();
    await page.getByPlaceholder('Member Email').fill(EMPLOYEE_EMAIL);
    await page.getByRole('button', { name: /^Employee/ }).click();
    await page.getByRole('button', { name: 'Invite Member' }).last().click();
    // The link to share opens right after inviting.
    await expect(page.getByText(`Send this link to ${EMPLOYEE_EMAIL}`)).toBeVisible();
    const shown = page.getByTestId('invite_link');
    await expect(shown).toHaveValue(/\/organization-invitations\//);
    inviteLink = await shown.inputValue();
    await page.getByTestId('invite_link_copy').click();
    await expect(page.getByTestId('invite_link_copy')).toHaveText('Copied');
    expect(await page.evaluate(() => navigator.clipboard.readText())).toBe(inviteLink);
    await page.getByRole('button', { name: 'Done' }).click();

    // A second invitation stays pending.
    await page.getByRole('button', { name: 'Invite member' }).click();
    await page.getByPlaceholder('Member Email').fill(PENDING_EMAIL);
    await page.getByRole('button', { name: /^Employee/ }).click();
    await page.getByRole('button', { name: 'Invite Member' }).last().click();
    await expect(page.getByText(`Send this link to ${PENDING_EMAIL}`)).toBeVisible();
    await expect(page.getByTestId('invite_link')).toHaveValue(/\/organization-invitations\//);
    await page.getByRole('button', { name: 'Done' }).click();
    await expect(page.getByTestId('invite_link')).toBeHidden();
    // The list on screen shows the new invitation without reloading the page.
    await expect(page.getByText(PENDING_EMAIL, { exact: true })).toBeVisible();

    // Later: Invitations tab -> the invitation's menu -> Copy invite link (newest first).
    await page.getByRole('tab', { name: 'Invitations' }).click();
    await page.getByRole('button', { name: 'Actions for the invitation' }).first().click();
    await expect(page.getByRole('menuitem', { name: 'Resend Invitation' })).toHaveCount(0);
    const linkResponse = page.waitForResponse((r) => /\/invitations\/[^/]+\/link$/.test(new URL(r.url()).pathname));
    await page.getByRole('menuitem', { name: 'Copy invite link' }).click();
    pendingLinkApi = new URL((await linkResponse).url()).pathname;
    await expect(page.getByText(`Send this link to ${PENDING_EMAIL}`)).toBeVisible();
    await expect(page.getByTestId('invite_link')).toHaveValue(/\/organization-invitations\//);
    await page.getByRole('button', { name: 'Done' }).click();
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

    // Invitation links are for admins: an employee is refused, a stranger must sign in.
    expect(pendingLinkApi).toContain(`/api/v1/organizations/${orgId}/invitations/`);
    expect((await request.get(pendingLinkApi, { headers: auth })).status()).toBe(403);
    expect((await request.get(pendingLinkApi, { headers: { Accept: 'application/json' } })).status()).toBe(401);

    const cfg = await request.get(`${AUTOTRACK}/v1/config`, { headers: { ...auth, 'X-TimePilot-Org': orgId } });
    expect(cfg.status()).toBe(200);
    expect((await cfg.json()).ai_provider).toBeTruthy();

    // The AI (production: Gemini) recognizes the project from a window title, as for the app.
    const project = projects.find((p: { name: string }) => p.name === PROJECT);
    const classified = await request.post(`${AUTOTRACK}/v1/classify`, {
        headers: { ...auth, 'X-TimePilot-Org': orgId },
        multipart: { app: 'Figma', title: `${PROJECT} – homepage hero – Figma` },
        timeout: 60_000,
    });
    expect(classified.status(), await classified.text()).toBe(200);
    const answer = await classified.json();
    expect(answer.model).toBeTruthy();
    expect(answer.project_id).toBe(project.id);

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

test('a forgotten password: the Super Admin sends a new link', async ({ browser }) => {
    test.skip(!superAdminState || !employeeState, 'needs the Super Admin and the employee');
    const admin = await browser.newContext({ storageState: superAdminState });
    const panel = await admin.newPage();
    await panel.goto(`/admin/users?tableSearch=${encodeURIComponent(EMPLOYEE_EMAIL)}`);
    await panel.getByRole('row', { name: EMPLOYEE_EMAIL }).getByRole('button', { name: 'Password link' }).click();
    await expect(panel.getByText('Bob Builder already has a password.')).toBeVisible();
    const resetLink = await passwordLinkFromDialog(panel);
    await panel.getByRole('button', { name: 'Done' }).click();

    // An organization admin's link can be found again under Organizations > Admin link.
    await panel.goto('/admin/organizations');
    await panel.getByRole('button', { name: 'Admin link' }).click();
    const pick = panel.getByRole('dialog', { name: 'Admin link' });
    await pick.getByRole('combobox', { name: /Organization/ }).click();
    await panel.keyboard.type(ORG);
    await pick.getByRole('option', { name: ORG, exact: true }).click();
    await pick.getByRole('button', { name: 'Show link' }).click();
    await expect(panel.getByRole('heading', { name: 'Password link for Alice Admin' })).toBeVisible();
    await expect(panel.getByText('Alice Admin already has a password.')).toBeVisible();
    expect(await passwordLinkFromDialog(panel)).toMatch(/\/welcome\//);
    await admin.close();

    const ctx = await browser.newContext();
    const page = await ctx.newPage();
    await page.goto(resetLink);
    await expect(page.getByRole('heading', { name: 'Choose a new password' })).toBeVisible();
    await page.getByLabel('Password', { exact: true }).fill(NEW_PASSWORD);
    await page.getByLabel('Confirm password').fill(NEW_PASSWORD);
    await page.getByRole('button', { name: 'Save password and sign in' }).click();
    await expect(page).toHaveURL(/\/dashboard/);
    await expect(page.getByText('Your password has been changed.')).toBeVisible();
    await ctx.close();

    // The new password works, the link does not work twice.
    const fresh = await browser.newContext();
    const login = await fresh.newPage();
    await logIn(login, EMPLOYEE_EMAIL, NEW_PASSWORD);
    await expect(login).toHaveURL(/\/dashboard/);
    await fresh.close();
    const guest = await browser.newContext();
    const again = await guest.newPage();
    await again.goto(resetLink);
    await expect(again.getByText('This link has already been used')).toBeVisible();
    await guest.close();
});
