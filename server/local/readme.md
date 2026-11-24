# Local stack

The Time Pilot web platform (built from `../web`), Postgres, Mailpit and the Autotrack service, for development and the end-to-end test.

| Address | What |
|---|---|
| http://localhost:8000 | web platform (Super Admin panel at `/admin`) |
| http://localhost:8025 | Mailpit: every email the platform sends |
| http://localhost:8010 | Autotrack service (settings page at `/admin/`) |

## First run

```bash
cd server/local
cp .env.example .env                       # database name, user, password
cp laravel.env.example laravel.env         # web settings (email goes to Mailpit)
docker run --rm solidtime/solidtime:0.23.0 php artisan self-host:generate-keys >> laravel.env
cp ../autotrack/.env.example ../autotrack/.env   # add GEMINI_API_KEY for live AI (optional)

docker compose up -d --build
docker compose exec app php artisan migrate --force
docker compose exec app php artisan passport:client --personal --name=API -n
docker compose exec app php artisan admin:user:create "Time Pilot Admin" superadmin@timepilot.local --verify-email
docker compose exec app php artisan timepilot:oauth-clients --admin-redirect=http://localhost:8010/admin/
```

The last command prints `{"desktop": "...", "admin": "..."}`. Put both ids in `laravel.env` as `TIMEPILOT_FIRST_PARTY_CLIENTS="<desktop>,<admin>"`, and in `../autotrack/.env` as `TIMEPILOT_DESKTOP_CLIENT_ID`, `TIMEPILOT_ADMIN_CLIENT_ID` and `TIMEPILOT_PUBLIC_URL=http://localhost:8000`. Then `docker compose up -d` again.

Sign in at http://localhost:8000 as the Super Admin (password printed by `admin:user:create`) and create an organization under `/admin` → Organizations → **New organization**. Its emails, with their links, appear in Mailpit.

`TRUSTED_HOSTS="app"` (in `laravel.env.example`) lets the Autotrack container call the web platform by its Docker name.

## End-to-end test

```bash
cd tests/e2e && npm ci && npx playwright install chromium
TP_SUPERADMIN_EMAIL=superadmin@timepilot.local TP_SUPERADMIN_PASSWORD=... npx playwright test
```
