<?php

declare(strict_types=1);

namespace App\Mail;

use App\Models\Organization;
use App\Models\User;
use Illuminate\Bus\Queueable;
use Illuminate\Mail\Mailable;
use Illuminate\Queue\SerializesModels;

/**
 * Sent to the admin of an organization the Super Admin created: a link to set the first
 * password, or a sign-in link if the person already has an account.
 */
class TimePilotAdminWelcomeMail extends Mailable
{
    use Queueable, SerializesModels;

    public function __construct(
        private readonly Organization $organization,
        private readonly User $admin,
        private readonly string $url,
        private readonly bool $setPassword,
    ) {}

    public function build(): self
    {
        return $this->markdown('emails.timepilot-admin-welcome', [
            'organizationName' => $this->organization->name,
            'adminName' => $this->admin->name,
            'url' => $this->url,
            'setPassword' => $this->setPassword,
            'days' => config()->integer('timepilot.admin_invite_days', 7),
        ])->subject(__(':organization is ready on Time Pilot', ['organization' => $this->organization->name]));
    }
}
