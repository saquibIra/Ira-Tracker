// Small text edits to solidtime files for Time Pilot. Each edit must match exactly the
// expected number of times, otherwise the build stops and names the edit, so an
// upstream change can never be skipped silently.
//
//   node patches.mjs <solidtime source dir>
import { readFileSync, writeFileSync } from 'node:fs';
import { join } from 'node:path';

const root = process.argv[2];
if (!root) {
    console.error('usage: node patches.mjs <source dir>');
    process.exit(2);
}

const TEAL = {
    50: '240, 253, 250',
    100: '204, 251, 241',
    200: '153, 246, 228',
    300: '94, 234, 212',
    400: '45, 212, 191',
    500: '20, 184, 166',
    600: '13, 148, 136',
    700: '15, 118, 110',
    800: '17, 94, 89',
    900: '19, 78, 74',
    950: '4, 47, 46',
};

/** @type {{file: string, find?: string | RegExp, replace?: string | ((m: string) => string), append?: string, count?: number, why: string}[]} */
const patches = [
    // ---- brand colour: the accent scale drives buttons, links, charts and focus -----
    ...Object.entries(TEAL).map(([step, rgb]) => ({
        file: 'resources/js/packages/ui/styles.css',
        find: new RegExp(`--color-accent-${step}: [0-9, ]+; /\\* sky-${step} \\*/`),
        replace: `--color-accent-${step}: ${rgb}; /* Time Pilot teal-${step} */`,
        why: `accent ${step} -> Time Pilot teal`,
    })),
    {
        // Dark mode: a hint of night-sky blue in surfaces and borders instead of pure grey.
        file: 'resources/js/packages/ui/styles.css',
        find: /(--color-(?:bg|border)-[a-z]+: oklch\(0\.\d+) 0 0\)/g,
        replace: (m) => m.replace(/ 0 0\)$/, ' 0.012 250)'),
        count: 10,
        why: 'dark surfaces tinted navy',
    },

    {
        // The product name comes from APP_NAME (emails, page titles), not a fixed "solidtime".
        file: 'config/app.php',
        find: "    'name' => 'solidtime',",
        replace: "    'name' => env('APP_NAME', 'Time Pilot'),",
        why: 'app name from APP_NAME',
    },

    // ---- page shell: browser and OS colours ------------------------------------------
    {
        file: 'resources/views/app.blade.php',
        find: 'content="#000000"',
        replace: 'content="#0D9488"',
        count: 2,
        why: 'tile and theme colour',
    },
    {
        file: 'resources/views/app.blade.php',
        find: 'safari-pinned-tab.svg" color="#000000"',
        replace: 'safari-pinned-tab.svg" color="#0D9488"',
        why: 'pinned tab colour',
    },

    // ---- product name in visible text ------------------------------------------------
    {
        file: 'resources/js/Pages/Profile/Partials/ThemeForm.vue',
        find: 'Choose how you want solidtime to look on your device',
        replace: 'Choose how Time Pilot looks on this device',
        why: 'theme setting text',
    },
    {
        file: 'resources/js/Pages/Teams/Partials/ExportData.vue',
        find: 'Export your solidtime organization data.',
        replace: 'Export your Time Pilot organization data.',
        why: 'export text',
    },
    {
        file: 'resources/js/Pages/Teams/Partials/ImportData.vue',
        find: 'different solidtime instance.',
        replace: 'different Time Pilot or solidtime instance.',
        why: 'import text',
    },

    {
        // Invite-only: the sign-in page explains how to get an account instead of a sign-up link.
        file: 'resources/js/Pages/Auth/Login.vue',
        find: `            <Link
                class="py-8 text-text-secondary text-sm font-medium opacity-90 hover:opacity-100 transition"
                :href="route('register')">
                No account yet? <span class="text-text-primary">Register here!</span>
            </Link>`,
        replace: `            <p class="py-8 text-center text-sm text-text-secondary lg:text-left">
                New to Time Pilot? Ask your organization's admin for an invitation.
            </p>`,
        why: 'sign-in page: no sign-up link',
    },

    {
        // New projects are visible to everyone in the organization by default, so employees
        // (and auto-tracking) can use them right away; admins can still make one private.
        file: 'resources/js/packages/ui/src/Project/ProjectCreateModal.vue',
        find: '    is_public: false,',
        replace: '    is_public: true,',
        count: 2,
        why: 'new projects visible to the organization',
    },

    // ---- invitations: new people set a password instead of a full sign-up -------------
    {
        file: 'app/Http/Controllers/Web/OrganizationInvitationController.php',
        find: `            return redirect(route('register'))
                ->with('registration_email', $email)
                ->with('bannerText', __('Please create an account to finish joining the :organization organization.', [
                    'organization' => $organization->name,
                ]))
                ->with('bannerStyle', 'info');`,
        replace: `            // Time Pilot: choose a password on the invitation itself (see TimePilotJoinController).
            return redirect(TimePilotJoinController::joinUrl($invitation));`,
        why: 'invitation for a new person opens the set-password page',
    },
    {
        file: 'app/Http/Controllers/Web/OrganizationInvitationController.php',
        find: 'use App\\Enums\\Role;\n',
        replace: 'use App\\Enums\\Role;\nuse App\\Http\\Controllers\\Web\\TimePilotJoinController;\n',
        why: 'import the join controller',
    },
    {
        file: 'routes/web.php',
        append: "\n// Time Pilot: joining by invitation, admin welcome, source link.\nrequire __DIR__.'/timepilot.php';\n",
        why: 'load the Time Pilot routes',
    },

    {
        file: 'app/Mail/OrganizationInvitationMail.php',
        find: "->subject(__('Organization Invitation'));",
        replace: "->subject(__('Join :organization on Time Pilot', ['organization' => $this->invitation->organization->name]));",
        why: 'invitation email subject',
    },

    // ---- OAuth: Time Pilot's own apps skip the consent screen -------------------------
    {
        file: 'app/Models/Passport/Client.php',
        find: '    protected static function newFactory(): Factory',
        replace: `    /**
     * Time Pilot: its own apps (desktop app, auto-tracking admin page) skip the "authorize
     * this app" screen. They are listed by id in TIMEPILOT_FIRST_PARTY_CLIENTS by the
     * installer; every other OAuth client still asks.
     *
     * @param  array<int, \\Laravel\\Passport\\Scope>  $scopes
     */
    public function skipsAuthorization(\\Illuminate\\Contracts\\Auth\\Authenticatable $user, array $scopes): bool
    {
        return in_array((string) $this->getKey(), config('timepilot.first_party_clients', []), true);
    }

    protected static function newFactory(): Factory`,
        why: 'first-party OAuth clients',
    },

    {
        // Desktop sessions: short access tokens renewed by a refresh token (Passport's
        // default is a year). Personal access tokens keep their own 12-month setting.
        file: 'app/Providers/AuthServiceProvider.php',
        find: '        // Passport::tokensExpireIn(now()->addDays(15));\n        // Passport::refreshTokensExpireIn(now()->addDays(30));',
        replace: '        Passport::tokensExpireIn(now()->addHours(12));\n        Passport::refreshTokensExpireIn(now()->addDays(90));',
        why: 'OAuth token lifetimes',
    },

    // ---- Super Admin panel ------------------------------------------------------------
    {
        file: 'app/Providers/Filament/AdminPanelProvider.php',
        find: "'primary' => Color::Amber,",
        replace: "'primary' => Color::hex('#0D9488'),",
        why: 'admin panel colour',
    },
    {
        file: 'app/Providers/Filament/AdminPanelProvider.php',
        find: "            ->path('admin')\n",
        replace: `            ->path('admin')
            ->brandName('Time Pilot Admin')
            ->brandLogo(fn () => asset('images/timepilot-logo.png'))
            ->darkModeBrandLogo(fn () => asset('images/timepilot-logo-light.png'))
            ->brandLogoHeight('2rem')
            ->favicon(fn () => asset('favicons/favicon-32x32.png'))
`,
        why: 'admin panel brand',
    },

    // ---- email look --------------------------------------------------------------------
    {
        file: 'resources/views/vendor/mail/html/themes/default.css',
        find: '#2d3748',
        replace: '#0D9488',
        count: 6,
        why: 'email button and panel colour',
    },
    {
        file: 'resources/views/vendor/mail/html/themes/default.css',
        find: 'color: #3869d4;',
        replace: 'color: #0F766E;',
        why: 'email link colour',
    },
    // ---- email off: admins copy and share the links ---------------------------------
    {
        // The frontend hides email-only buttons while email is off.
        file: 'app/Http/Middleware/HandleInertiaRequests.php',
        find: "            'is_impersonating' => app(ImpersonateManager::class)->isImpersonating(),\n",
        replace:
            "            'is_impersonating' => app(ImpersonateManager::class)->isImpersonating(),\n            'timepilot' => ['mail' => \\App\\Service\\TimePilotOnboardingService::mailEnabled()], // Time Pilot\n",
        why: 'share whether email is on',
    },
    {
        file: 'routes/api.php',
        find: "            Route::post('/invitations/{invitation}/resend', [InvitationController::class, 'resend'])->name('resend')->middleware('check-organization-blocked');\n",
        replace:
            "            Route::post('/invitations/{invitation}/resend', [InvitationController::class, 'resend'])->name('resend')->middleware('check-organization-blocked');\n            Route::get('/invitations/{invitation}/link', [\\App\\Http\\Controllers\\Api\\V1\\TimePilotInvitationLinkController::class, 'show'])->name('link'); // Time Pilot\n",
        why: 'invitation link endpoint',
    },
    {
        // Super Admin: a "choose a password" link for anyone who forgot theirs.
        file: 'app/Filament/Resources/UserResource.php',
        find: '                EditAction::make(),\n',
        replace: '                EditAction::make(),\n                \\App\\Filament\\TimePilotPasswordLink::userAction(), // Time Pilot\n',
        why: 'password link on users',
    },
    {
        // After inviting, show the link to share.
        file: 'resources/js/Components/Common/Member/MemberInviteModal.vue',
        find: `            () => {
                addTeamMemberForm.reset();
                emit('close');
                show.value = false;
            }`,
        replace: `            () => {
                const invited = addTeamMemberForm.email; // Time Pilot: show the link to share
                addTeamMemberForm.reset();
                emit('close');
                show.value = false;
                openInviteLink({ email: invited });
            }`,
        why: 'invite: show the link afterwards',
    },
    {
        file: 'resources/js/Components/Common/Member/MemberInviteModal.vue',
        find: "import { useNotificationsStore } from '@/utils/notification';\n",
        replace: "import { useNotificationsStore } from '@/utils/notification';\nimport { openInviteLink } from '@/utils/timepilotInviteLink';\n",
        why: 'invite: import the link dialog',
    },
    {
        file: 'resources/js/Components/Common/Invitation/InvitationMoreOptionsDropdown.vue',
        find: "import { TrashIcon, ArrowPathIcon } from '@heroicons/vue/20/solid';\n",
        replace:
            "import { TrashIcon, ArrowPathIcon, LinkIcon } from '@heroicons/vue/20/solid';\nimport { usePage } from '@inertiajs/vue3';\n",
        why: 'invitation menu: imports',
    },
    {
        file: 'resources/js/Components/Common/Invitation/InvitationMoreOptionsDropdown.vue',
        find: `    resend: [];
}>();
`,
        replace: `    resend: [];
    link: [];
}>();

// Time Pilot: "Resend Invitation" only when email is on; the link can always be copied.
const mailEnabled =
    (usePage().props as { timepilot?: { mail?: boolean } }).timepilot?.mail === true;
`,
        why: 'invitation menu: link event',
    },
    {
        file: 'resources/js/Components/Common/Invitation/InvitationMoreOptionsDropdown.vue',
        find: `        <DropdownMenuContent class="min-w-[150px]" align="end">
            <DropdownMenuItem
                data-testid="invitation_delete"
                class="flex items-center space-x-3 cursor-pointer"
                @click="emit('resend')">`,
        replace: `        <DropdownMenuContent class="min-w-[150px]" align="end">
            <DropdownMenuItem
                data-testid="invitation_link"
                class="flex items-center space-x-3 cursor-pointer"
                @click="emit('link')">
                <LinkIcon class="w-5 text-icon-active" />
                <span>Copy invite link</span>
            </DropdownMenuItem>
            <DropdownMenuItem
                v-if="mailEnabled"
                data-testid="invitation_resend"
                class="flex items-center space-x-3 cursor-pointer"
                @click="emit('resend')">`,
        why: 'invitation menu: copy link item',
    },
    {
        file: 'resources/js/Components/Common/Invitation/InvitationTableRow.vue',
        find: '<InvitationMoreOptionsDropdown @delete="deleteInvitation" @resend="resendInvitation" />',
        replace:
            '<InvitationMoreOptionsDropdown\n                @delete="deleteInvitation"\n                @resend="resendInvitation"\n                @link="openInviteLink({ id: invitation.id, email: invitation.email })" />',
        why: 'invitation row: open the link dialog',
    },
    {
        file: 'resources/js/Components/Common/Invitation/InvitationTableRow.vue',
        find: "import { useInvitationsStore } from '@/utils/useInvitations';\n",
        replace: "import { useInvitationsStore } from '@/utils/useInvitations';\nimport { openInviteLink } from '@/utils/timepilotInviteLink';\n",
        why: 'invitation row: import',
    },
    {
        file: 'resources/js/Pages/Members.vue',
        find: `        <InvitationTable v-if="activeTab === 'invitations'"></InvitationTable>\n`,
        replace: `        <InvitationTable v-if="activeTab === 'invitations'"></InvitationTable>\n        <TimePilotInviteLinkModal />\n`,
        why: 'members page: link dialog',
    },
    {
        file: 'resources/js/Pages/Members.vue',
        find: "import InvitationTable from '@/Components/Common/Invitation/InvitationTable.vue';\n",
        replace:
            "import InvitationTable from '@/Components/Common/Invitation/InvitationTable.vue';\nimport TimePilotInviteLinkModal from '@/Components/Common/Invitation/TimePilotInviteLinkModal.vue';\n",
        why: 'members page: import',
    },
    {
        // Without email a reset email never arrives: point people to their admin instead.
        file: 'resources/js/Pages/Auth/Login.vue',
        find: `                <Link
                    v-if="canResetPassword"
                    :href="route('password.request')"
                    class="underline text-sm text-text-secondary hover:text-text-primary rounded-md focus:outline-none focus:ring-2 focus:ring-offset-2 focus:ring-indigo-500">
                    Forgot your password?
                </Link>`,
        replace: `                <Link
                    v-if="canResetPassword && mailEnabled"
                    :href="route('password.request')"
                    class="underline text-sm text-text-secondary hover:text-text-primary rounded-md focus:outline-none focus:ring-2 focus:ring-offset-2 focus:ring-indigo-500">
                    Forgot your password?
                </Link>
                <span v-else class="text-sm text-text-secondary">
                    Forgot your password? Ask your admin.
                </span>`,
        why: 'sign-in page: forgot password without email',
    },
    {
        file: 'resources/js/Pages/Auth/Login.vue',
        find: "import TextInput from '@/packages/ui/src/Input/TextInput.vue';\n",
        replace:
            "import TextInput from '@/packages/ui/src/Input/TextInput.vue';\n\nconst mailEnabled =\n    (usePage().props as { timepilot?: { mail?: boolean } }).timepilot?.mail === true;\n",
        why: 'sign-in page: email switch',
    },
];

let failed = 0;
for (const p of patches) {
    const path = join(root, p.file);
    const text = readFileSync(path, 'utf8');
    if (p.append !== undefined) {
        writeFileSync(path, text.replace(/\s*$/, '\n') + p.append);
        console.log(`patched: ${p.why}`);
        continue;
    }
    const want = p.count ?? 1;
    let found;
    if (typeof p.find === 'string') {
        found = text.split(p.find).length - 1;
    } else {
        const rx = new RegExp(p.find.source, p.find.flags.includes('g') ? p.find.flags : p.find.flags + 'g');
        found = (text.match(rx) || []).length;
    }
    if (found !== want) {
        console.error(`PATCH FAILED (${p.why}): ${p.file} matched ${found} times, expected ${want}`);
        failed++;
        continue;
    }
    const next =
        typeof p.find === 'string'
            ? text.split(p.find).join(typeof p.replace === 'string' ? p.replace : p.replace(p.find))
            : text.replace(p.find, p.replace);
    writeFileSync(path, next);
    console.log(`patched: ${p.why}`);
}
if (failed) {
    process.exit(1);
}
