<?php

declare(strict_types=1);

namespace App\Service;

use App\Enums\Weekday;
use App\Http\Controllers\Web\TimePilotJoinController;
use App\Mail\TimePilotAdminWelcomeMail;
use App\Models\Organization;
use App\Models\User;
use Illuminate\Support\Facades\DB;
use Illuminate\Support\Facades\Mail;
use Illuminate\Support\Str;

/**
 * Time Pilot: the Super Admin creates a customer organization together with its admin.
 * The admin becomes the owner; the Super Admin never becomes a member. The admin sets a
 * password from TimePilotJoinController::welcomeUrl(), shared by the Super Admin (or
 * emailed, when email is on).
 */
class TimePilotOnboardingService
{
    public function __construct(private readonly OrganizationService $organizations) {}

    public function createOrganizationWithAdmin(
        string $organizationName,
        string $adminName,
        string $adminEmail,
        ?string $currency
    ): Organization {
        $email = Str::lower(trim($adminEmail));

        $organization = DB::transaction(function () use ($organizationName, $adminName, $email, $currency): Organization {
            $admin = User::query()
                ->where('email', $email)
                ->where('is_placeholder', '=', false)
                ->first();
            if ($admin === null) {
                $admin = new User;
                $admin->name = trim($adminName);
                $admin->email = $email;
                $admin->password = null; // chosen from the welcome link
                $admin->timezone = 'UTC'; // replaced by the browser's zone on the welcome page
                $admin->week_start = Weekday::Monday;
                $admin->save();
            }

            $organization = $this->organizations->createOrganization(trim($organizationName), $admin, false, $currency);
            if ($admin->current_team_id === null) {
                $admin->currentOrganization()->associate($organization);
                $admin->save();
            }

            return $organization;
        });

        if (self::mailEnabled()) {
            $this->sendAdminEmail($organization);
        }

        return $organization;
    }

    /**
     * Whether email is delivered. While it is off (MAIL_MAILER=log), the Super Admin and the
     * organization admins copy the set-password and invitation links and share them.
     */
    public static function mailEnabled(): bool
    {
        return ! in_array(config('mail.default'), ['log', 'array'], true);
    }

    /**
     * Sends (or sends again) the owner's email: a set-password link while they have no
     * password yet, otherwise a sign-in link.
     */
    public function sendAdminEmail(Organization $organization): void
    {
        /** @var User $admin */
        $admin = $organization->owner;
        $needsPassword = $admin->password === null;
        $url = $needsPassword ? TimePilotJoinController::welcomeUrl($admin) : route('login');

        Mail::to($admin->email)->send(new TimePilotAdminWelcomeMail($organization, $admin, $url, $needsPassword));
    }
}
