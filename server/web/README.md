# Time Pilot web platform

The web platform is **solidtime** (https://github.com/solidtime-io/solidtime, AGPL-3.0) with Time Pilot's brand and onboarding. It is built from the official image, so the changes stay small and easy to carry to new solidtime versions.

```
Dockerfile     official solidtime image + overlay + patches, frontend rebuilt
overlay/       files added to, or replacing files in, the solidtime source tree
patches.mjs    small text edits; each must match exactly or the build stops
upstream/      build configs the published image leaves out (same solidtime tag)
```

```bash
docker build -t time-pilot/web:dev .        # local; production builds it on the server
```

## What Time Pilot changes

| Change | Where |
|---|---|
| Brand: logo, teal theme, navy dark mode, favicons, app name from `APP_NAME` | `overlay/resources/js/Components/*`, `overlay/public/*`, `patches.mjs` (accent colours, `config/app.php`) |
| Sign-in pages: split layout with the product story; "ask your admin" instead of a sign-up link | `AuthenticationCard.vue`, patch to `Login.vue` |
| Sidebar card: desktop app, auto-tracking settings, source code | `UpdateSidebarNotification.vue` |
| Super Admin: **New organization** (with its admin as owner) and **Resend admin email** | `ListOrganizations.php`, `TimePilotOnboardingService.php`, `TimePilotAdminWelcomeMail.php` |
| Invitations: people without an account set a password on the link (account verified by the link) | `TimePilotJoinController.php`, `Pages/TimePilot/SetPassword.vue`, patch to `OrganizationInvitationController.php` |
| Invite-only: `/register` leads to sign-in when sign-up is off | `routes/timepilot.php` |
| OAuth: Time Pilot's own clients skip the consent screen; 12 h access / 90 day refresh tokens | patches to `Client.php`, `AuthServiceProvider.php`; `timepilot:oauth-clients` |
| New projects visible to the organization by default | patch to `ProjectCreateModal.vue` |
| Emails: logo, colours, invitation text and subject | `overlay/resources/views/...`, patches |
| Admin panel brand | patch to `AdminPanelProvider.php` |
| AGPL source link | `/source` route (`TIMEPILOT_SOURCE_URL`) |

Settings (in `laravel.env`): `APP_NAME`, `TIMEPILOT_FIRST_PARTY_CLIENTS` (set by the installer), `TIMEPILOT_SOURCE_URL`, `TIMEPILOT_ADMIN_INVITE_DAYS` (7), `TIMEPILOT_JOIN_LINK_MINUTES` (120).

## Updating solidtime

1. Read the solidtime release notes (database migrations, breaking changes).
2. Change `SOLIDTIME_VERSION` in the Dockerfile and refresh `upstream/` from the same tag:
   `for f in eslint.config.mjs jsconfig.json postcss.config.js tailwind.config.js tsconfig.json; do curl -sfLo upstream/$f https://raw.githubusercontent.com/solidtime-io/solidtime/v<version>/$f; done`
3. `docker build .` — a `PATCH FAILED` line names the edit whose upstream text changed; compare each replaced file in `overlay/` with its new upstream version too.
4. Run the end-to-end test (`tests/e2e`) against the local stack before deploying.

## Licence

This is a modified version of solidtime and is licensed under the GNU Affero General Public License v3.0. People who use the platform over the network must be able to get its source code: the **Source code** link on the sign-in pages and in the sidebar points to `TIMEPILOT_SOURCE_URL` (this repository). Keep that link and this notice.
