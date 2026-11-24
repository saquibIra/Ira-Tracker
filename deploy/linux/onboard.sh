#!/usr/bin/env bash
# Self-registration without email (the server has no SMTP yet). install.sh runs this
# every minute from cron while REGISTRATION=on; each run checks four times, 15 s apart,
# so a new account works about 15 seconds after registering.
#
# solidtime makes every new account confirm its email address and puts it into a new
# personal organization of its own. For people who registered with an allowed address
# (REGISTRATION_DOMAINS: comma-separated domains, or * for any address; default: the
# domain of ADMIN_EMAIL) this job:
#   1. marks the address as verified,
#   2. adds them to the company organization as an employee and makes it their current one,
#   3. deletes the empty personal organization solidtime created at registration, so the
#      app signs them straight into the company.
# Other addresses stay unverified and can't use the site, the API or the app.
#
# Only unverified accounts are looked at, so someone an admin removed later is not added
# back. Without SMTP nobody proves they own the address: anyone who types an allowed
# address (with *, anyone at all) gets in as an employee. Admins see everyone under
# Members; configure SMTP and switch to REGISTRATION=invite-only to close this.
set -euo pipefail

HERE="$(cd "$(dirname "$0")" && pwd)"
DATA="${DATA_DIR:-/opt/ira-data}"
# shellcheck disable=SC1091
. "$DATA/compose.env"
cd "$HERE"
COMPOSE="${COMPOSE:-docker compose --env-file $DATA/compose.env}"
DOMAINS="${REGISTRATION_DOMAINS:-${ADMIN_EMAIL#*@}}"
PASSES="${ONBOARD_PASSES:-4}"
now() { date '+%Y-%m-%dT%H:%M:%S%z'; }

# One run at a time (a slow start must not overlap the next minute).
exec 9>"$DATA/.onboard.lock"
flock -n 9 || exit 0

# Values go in as environment variables, never spliced into PHP code. One closure, so
# tinker does not print whole models.
if ! out="$($COMPOSE exec -T -e IRA_DOMAINS="$DOMAINS" -e IRA_ADMIN="${ADMIN_EMAIL:-}" \
  -e IRA_ORG="${ORG_ID:-}" -e IRA_PASSES="$PASSES" scheduler php artisan tinker --execute='
(function () {
    $domains = array_values(array_filter(array_map(
        fn ($d) => strtolower(trim($d)), explode(",", (string) getenv("IRA_DOMAINS"))
    )));
    if ($domains === []) { echo "ERROR: no REGISTRATION_DOMAINS\n"; return; }
    $any = in_array("*", $domains, true);
    $org = getenv("IRA_ORG")
        ? App\Models\Organization::query()->whereKey(getenv("IRA_ORG"))->firstOrFail()
        : App\Models\User::query()->where("email", strtolower((string) getenv("IRA_ADMIN")))
            ->where("is_placeholder", false)->firstOrFail()
            ->ownedOrganizations()->orderBy("created_at")->firstOrFail();
    $passes = max(1, (int) getenv("IRA_PASSES"));
    for ($pass = 0; $pass < $passes; $pass++) {
        if ($pass > 0) { sleep(15); }
        $pending = App\Models\User::query()
            ->where("is_placeholder", false)
            ->whereNull("email_verified_at")
            ->when(! $any, fn ($q) => $q->where(function ($q) use ($domains) {
                foreach ($domains as $d) { $q->orWhere("email", "like", "%@".$d); }
            }))
            ->orderBy("created_at")->limit(50)->get();
        foreach ($pending as $user) {
            $domain = strtolower(substr((string) strrchr($user->email, "@"), 1));
            if (! $any && ! in_array($domain, $domains, true)) { continue; }
            Illuminate\Support\Facades\DB::transaction(function () use ($user, $org) {
                $user->email_verified_at = now();
                $user->save();
                $isMember = App\Models\Member::query()->where("organization_id", $org->getKey())
                    ->where("user_id", $user->getKey())->exists();
                if (! $isMember) {
                    app(App\Service\MemberService::class)->addMember($user, $org, App\Enums\Role::Employee);
                }
                $user->current_team_id = $org->getKey();
                $user->save();
                $own = App\Models\Organization::query()->where("user_id", $user->getKey())
                    ->where("personal_team", true)->get();
                foreach ($own as $o) {
                    if ($o->is($org)) { continue; }
                    $empty = App\Models\Member::query()->where("organization_id", $o->getKey())->count() === 1
                        && ! App\Models\TimeEntry::query()->where("organization_id", $o->getKey())->exists()
                        && ! App\Models\Project::query()->where("organization_id", $o->getKey())->exists()
                        && ! App\Models\Client::query()->where("organization_id", $o->getKey())->exists();
                    if ($empty) {
                        app(App\Service\DeletionService::class)->deleteOrganization($o, false);
                    }
                }
            });
            echo "onboarded ".$user->email." into \"".$org->name."\" as employee\n";
        }
    }
})();
null;
' 2>&1)"; then
  echo "$(now) onboarding failed:"
  printf '%s\n' "$out" | tail -20
  exit 1
fi

# Log only what this job did (cron appends it to $DATA/onboard.log); solidtime's own
# log lines are left out.
printf '%s\n' "$out" | grep -E '^(onboarded|ERROR)' | sed "s/^/$(now) /" || true
