# Server: solidtime (local development)

solidtime is the system of record (clients, projects, tasks, members, rates, time entries). We run it unmodified in Docker and talk to it only through its REST API.

Based on solidtime's official example `1-docker-with-database` (see `solidtime/readme.md`).

## First-time setup

```bash
cd server/solidtime
cp .env.example .env
cp laravel.env.example laravel.env
```

Edit the two files for local HTTP use:

| File | Setting | Local value |
|------|---------|-------------|
| `.env` | `APP_DOMAIN` | `localhost` |
| `.env` and `laravel.env` | `DB_PASSWORD` | the same random string in both (`openssl rand -hex 16`) |
| `laravel.env` | `APP_URL` | `"http://localhost:8000"` |
| `laravel.env` | `APP_FORCE_HTTPS` | `"false"` |
| `laravel.env` | `MAIL_MAILER` | `"log"` (no SMTP locally) |
| `laravel.env` | `APP_ENV` | keep `"production"` — `local` crashes (dev-only packages are not in the image) |

Generate keys and paste the three printed values (`APP_KEY`, `PASSPORT_PRIVATE_KEY`, `PASSPORT_PUBLIC_KEY`) into `laravel.env`:

```bash
docker compose run --rm scheduler php artisan self-host:generate-keys
```

Start, migrate, and create the OAuth client used for personal API tokens:

```bash
docker compose up -d
docker compose exec scheduler php artisan migrate --force
docker compose exec scheduler php artisan passport:client --personal --name="API" -n
```

Create your user (prints a random password) and make it super admin:

```bash
docker compose exec scheduler php artisan admin:user:create "Your Name" "you@iratechnologies.com" --verify-email
# then in laravel.env: SUPER_ADMINS="you@iratechnologies.com"
docker compose up -d --force-recreate
```

Open http://localhost:8000, log in, and create an API token under **Profile → API Tokens**.

## Demo data

Creates 3 demo customers, 5 billable projects with tasks, and the internal projects (Internal, Pre-sales, Learning, Bench). Safe to re-run.

```bash
SOLIDTIME_TOKEN=<your token> python3 server/seed/seed_demo.py
```

## Everyday commands

```bash
docker compose up -d          # start
docker compose stop           # stop (data kept)
docker compose logs -f app    # logs
docker compose stop app       # simulate "server offline" for widget testing
```

## Secrets

`.env`, `laravel.env` and `.admin-credentials` contain secrets and are git-ignored. Never commit them.
