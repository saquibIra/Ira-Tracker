#!/usr/bin/env bash
# Adds an employee to the company organization without needing email (no SMTP yet).
# Run on the server:
#
#   sudo /opt/ira-tracker/deploy/linux/add-user.sh "Asha Kumar" asha@iratechnologies.com [employee|manager|admin]
#
# Prints the login (email + one-time random password) to hand over privately. The
# person signs in at https://<domain>, changes the password (Profile), creates an API
# token and installs the app from https://<domain>/download/.
#
# The company organization is the first one owned by ADMIN_EMAIL (from compose.env);
# set ORG_ID=<uuid> to choose another.
set -euo pipefail

NAME="${1:?usage: add-user.sh \"Full Name\" email [role]}"
EMAIL="${2:?usage: add-user.sh \"Full Name\" email [role]}"
ROLE="${3:-employee}"
case "$ROLE" in employee | manager | admin) ;; *) echo "role must be employee, manager or admin"; exit 1 ;; esac
[[ "$EMAIL" =~ ^[^@[:space:]]+@[^@[:space:]]+\.[^@[:space:]]+$ ]] || { echo "invalid email"; exit 1; }

HERE="$(cd "$(dirname "$0")" && pwd)"
DATA="${DATA_DIR:-/opt/ira-data}"
# shellcheck disable=SC1091
. "$DATA/compose.env"
cd "$HERE"
COMPOSE="${COMPOSE:-docker compose --env-file $DATA/compose.env}"

# Values go in as environment variables, never spliced into PHP code. The body is one
# closure so tinker does not print (and walk) whole models after each line.
$COMPOSE exec -T -e IRA_NAME="$NAME" -e IRA_EMAIL="$EMAIL" -e IRA_ROLE="$ROLE" \
  -e IRA_ADMIN="${ADMIN_EMAIL:-}" -e IRA_ORG="${ORG_ID:-}" scheduler php artisan tinker --execute='
(function () {
    $email = strtolower(trim(getenv("IRA_EMAIL")));
    if (App\Models\User::query()->where("email", $email)->where("is_placeholder", false)->exists()) { echo "ERROR: a user with this email already exists\n"; return; }
    $org = getenv("IRA_ORG")
        ? App\Models\Organization::query()->whereKey(getenv("IRA_ORG"))->firstOrFail()
        : App\Models\User::where("email", strtolower(getenv("IRA_ADMIN")))->firstOrFail()->ownedOrganizations()->orderBy("created_at")->firstOrFail();
    $password = bin2hex(random_bytes(9));
    $user = app(App\Service\UserService::class)->createUser(
        getenv("IRA_NAME"), $email, $password, "Asia/Kolkata", App\Enums\Weekday::Monday, $org->currency ?? "INR"
    );
    Illuminate\Support\Facades\DB::transaction(function () use ($user, $org) {
        $user->email_verified_at = now();
        $user->save();
        app(App\Service\MemberService::class)->addMember($user, $org, App\Enums\Role::from(getenv("IRA_ROLE")));
        $user->current_team_id = $org->getKey();
        $user->save();
        // Remove the new, empty personal organization solidtime made, so the person has
        // only the company and the app needs no organization choice.
        foreach (App\Models\Organization::query()->where("user_id", $user->getKey())->where("personal_team", true)->get() as $o) {
            if (! $o->is($org)) { app(App\Service\DeletionService::class)->deleteOrganization($o, false); }
        }
    });
    echo "Added ".$user->name." to \"".$org->name."\" as ".getenv("IRA_ROLE")."\n";
    echo "Login:    ".$email."\n";
    echo "Password: ".$password."   (ask them to change it under Profile)\n";
})();
null;
'
