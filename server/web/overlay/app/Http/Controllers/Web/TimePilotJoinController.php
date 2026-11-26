<?php

declare(strict_types=1);

namespace App\Http\Controllers\Web;

use App\Actions\Fortify\PasswordValidationRules;
use App\Enums\Role;
use App\Enums\Weekday;
use App\Models\OrganizationInvitation;
use App\Models\User;
use App\Service\TimezoneService;
use App\Service\UserService;
use Illuminate\Auth\Events\Verified;
use Illuminate\Http\RedirectResponse;
use Illuminate\Http\Request;
use Illuminate\Support\Facades\Auth;
use Illuminate\Support\Facades\DB;
use Illuminate\Support\Facades\Hash;
use Illuminate\Support\Facades\URL;
use Illuminate\Support\Str;
use Inertia\Inertia;
use Inertia\Response;

/**
 * Time Pilot onboarding: people join by choosing a password from the link they were
 * emailed, instead of filling in a sign-up form and then confirming their email. The
 * link already proves that they own the address, so the account is created verified.
 *
 *  - Employees: invitation email -> OrganizationInvitationController::accept -> join page.
 *  - Organization admins created by the Super Admin: welcome email -> welcome page.
 */
class TimePilotJoinController extends Controller
{
    use PasswordValidationRules;

    /** Signed, short-lived link to the set-password page of an invitation. */
    public static function joinUrl(OrganizationInvitation $invitation): string
    {
        return URL::temporarySignedRoute(
            'timepilot.join',
            now()->addMinutes(config()->integer('timepilot.join_link_minutes', 120)),
            ['invitation' => $invitation->getKey()],
            absolute: false
        );
    }

    /**
     * The link of an invitation, the same one solidtime's invitation email carries
     * (OrganizationInvitationMail): it leads a new person to the set-password page.
     */
    public static function invitationUrl(OrganizationInvitation $invitation): string
    {
        return URL::to(URL::signedRoute(
            'organization-invitations.accept',
            ['invitation' => $invitation->getKey()],
            now()->addDays(90),
            false
        ));
    }

    /**
     * Signed link to choose a password: the first one of a new organization admin, or a new
     * one for someone who forgot theirs (made by the Super Admin). It stops working once a
     * password is set, because its check value covers the password hash.
     */
    public static function welcomeUrl(User $user): string
    {
        return URL::to(URL::temporarySignedRoute(
            'timepilot.welcome',
            now()->addDays(config()->integer('timepilot.admin_invite_days', 7)),
            ['user' => $user->getKey(), 'check' => self::welcomeCheck($user)],
            absolute: false
        ));
    }

    private static function welcomeCheck(User $user): string
    {
        $material = $user->getKey().'|'.$user->email.'|'.($user->password ?? '');

        return substr(hash_hmac('sha256', $material, (string) config('app.key')), 0, 32);
    }

    public function showInvitation(Request $request, OrganizationInvitation $invitation): Response|RedirectResponse
    {
        if ($this->hasAccount($invitation->email)) {
            return $this->signInInstead();
        }
        $organization = $invitation->organization->name;

        return Inertia::render('TimePilot/SetPassword', [
            'title' => __('Join :organization', ['organization' => $organization]),
            'intro' => __(':organization tracks time with Time Pilot. Choose a password to finish joining.', [
                'organization' => $organization,
            ]),
            'email' => Str::lower($invitation->email),
            'name' => '',
            'action' => $request->getRequestUri(),
            'submitLabel' => __('Join :organization', ['organization' => $organization]),
        ]);
    }

    public function acceptInvitation(
        Request $request,
        OrganizationInvitation $invitation,
        UserService $userService
    ): RedirectResponse {
        $data = $this->validated($request);
        $role = Role::tryFrom($invitation->role);
        abort_if($role === null || $role === Role::Owner || $role === Role::Placeholder, 403);
        $email = Str::lower($invitation->email);
        if ($this->hasAccount($email)) {
            return $this->signInInstead();
        }
        $organization = $invitation->organization;

        $user = DB::transaction(function () use ($invitation, $userService, $data, $email, $organization, $role): User {
            // createUser() adds the membership of every accepted invitation for this address.
            if ($invitation->accepted_at === null) {
                $invitation->accepted_at = now();
                $invitation->save();
            }

            return $userService->createUser(
                name: $data['name'],
                email: $email,
                password: $data['password'],
                timezone: $data['timezone'],
                weekStart: Weekday::Monday,
                currency: null,
                verifyEmail: true,
                organization: $organization,
                role: $role,
            );
        });

        Auth::login($user);
        $request->session()->regenerate();

        return redirect()->route('dashboard')
            ->with('bannerText', __('Welcome to :organization. Install the Time Pilot desktop app to start tracking.', [
                'organization' => $organization->name,
            ]))
            ->with('bannerStyle', 'success');
    }

    public function showWelcome(Request $request, User $user, string $check): Response|RedirectResponse
    {
        if (! hash_equals(self::welcomeCheck($user), $check)) {
            return $this->linkUsed();
        }
        $organization = $user->currentOrganization;
        if ($user->password !== null) {
            return Inertia::render('TimePilot/SetPassword', [
                'title' => __('Choose a new password'),
                'intro' => __('Your admin sent you this link to reset your Time Pilot password.'),
                'email' => $user->email,
                'name' => $user->name,
                'action' => $request->getRequestUri(),
                'submitLabel' => __('Save password and sign in'),
            ]);
        }

        return Inertia::render('TimePilot/SetPassword', [
            'title' => __('Welcome to Time Pilot'),
            'intro' => $organization !== null && $organization->user_id === $user->getKey()
                ? __('You are the admin of :organization. Choose a password to sign in.', [
                    'organization' => $organization->name,
                ])
                : __('Choose a password to sign in.'),
            'email' => $user->email,
            'name' => $user->name,
            'action' => $request->getRequestUri(),
            'submitLabel' => __('Set password and continue'),
        ]);
    }

    public function completeWelcome(Request $request, User $user, string $check): RedirectResponse
    {
        if (! hash_equals(self::welcomeCheck($user), $check)) {
            return $this->linkUsed();
        }
        $data = $this->validated($request);
        $firstPassword = $user->password === null;
        $firstVerification = $user->email_verified_at === null;

        $user->name = $data['name'];
        $user->password = Hash::make($data['password']);
        $user->setRememberToken(Str::random(60)); // "remember me" elsewhere ends with the old password
        $user->timezone = $data['timezone'];
        if ($firstVerification) {
            $user->email_verified_at = now();
        }
        $user->save();
        if ($firstVerification) {
            event(new Verified($user));
        }

        Auth::login($user);
        $request->session()->regenerate();

        return redirect()->route('dashboard')
            ->with('bannerText', $firstPassword
                ? __('Your organization is ready. Add your projects, then invite your team under Members.')
                : __('Your password has been changed.'))
            ->with('bannerStyle', 'success');
    }

    /**
     * @return array{name: string, password: string, timezone: string}
     */
    private function validated(Request $request): array
    {
        $data = $request->validate([
            'name' => ['required', 'string', 'max:255'],
            'password' => $this->passwordRules(),
            'timezone' => ['nullable', 'string', 'max:64'],
        ]);

        return [
            'name' => trim((string) $data['name']),
            'password' => (string) $data['password'],
            'timezone' => $this->timezone(isset($data['timezone']) ? (string) $data['timezone'] : null),
        ];
    }

    /**
     * The browser's zone, as solidtime's own sign-up accepts it: legacy names such as
     * Asia/Calcutta (still reported by some browsers) map to the current one; anything
     * unknown becomes UTC and can be changed in the profile.
     */
    private function timezone(?string $browser): string
    {
        if ($browser === null || $browser === '') {
            return 'UTC';
        }
        $zones = app(TimezoneService::class);
        if ($zones->isValid($browser)) {
            return $browser;
        }

        return $zones->mapLegacyTimezone($browser) ?? 'UTC';
    }

    private function hasAccount(string $email): bool
    {
        return User::query()
            ->where('email', Str::lower($email))
            ->where('is_placeholder', '=', false)
            ->exists();
    }

    private function signInInstead(): RedirectResponse
    {
        return redirect()->route('login')
            ->with('bannerText', __('You already have a Time Pilot account. Sign in to continue.'))
            ->with('bannerStyle', 'info');
    }

    private function linkUsed(): RedirectResponse
    {
        return redirect()->route('login')
            ->with('bannerText', __('This link has already been used. Sign in with your password, or choose "Forgot your password?".'))
            ->with('bannerStyle', 'info');
    }
}
