<?php

declare(strict_types=1);

namespace App\Console\Commands;

use Illuminate\Console\Command;
use Laravel\Passport\Client;
use Laravel\Passport\ClientRepository;
use Laravel\Passport\Passport;

/**
 * Time Pilot: creates (once) and updates the OAuth clients of its own apps and prints
 * their ids as JSON, so the installer can run it on every deploy.
 *
 *  - Desktop app: public client with PKCE; redirects to a loopback address on any port
 *    (RFC 8252), e.g. http://127.0.0.1:53682/callback.
 *  - Auto-tracking admin page: public client with PKCE; redirects back to the page.
 */
class TimePilotOAuthClients extends Command
{
    protected $signature = 'timepilot:oauth-clients
        {--admin-redirect= : Redirect URI of the auto-tracking admin page}';

    protected $description = 'Create or update the Time Pilot OAuth clients and print their ids as JSON';

    public const string DESKTOP = 'Time Pilot Desktop';

    public const string ADMIN = 'Time Pilot Auto-tracking Admin';

    public function handle(ClientRepository $clients): int
    {
        $adminRedirect = (string) $this->option('admin-redirect');
        if ($adminRedirect === '' || filter_var($adminRedirect, FILTER_VALIDATE_URL) === false) {
            $this->error('Pass --admin-redirect=https://<domain>/autotrack/admin/');

            return self::FAILURE;
        }

        $desktop = $this->ensure($clients, self::DESKTOP, ['http://127.0.0.1/callback']);
        $admin = $this->ensure($clients, self::ADMIN, [$adminRedirect]);

        $this->line((string) json_encode([
            'desktop' => (string) $desktop->getKey(),
            'admin' => (string) $admin->getKey(),
        ]));

        return self::SUCCESS;
    }

    /**
     * @param  array<int, string>  $redirectUris
     */
    private function ensure(ClientRepository $clients, string $name, array $redirectUris): Client
    {
        /** @var Client|null $client */
        $client = Passport::client()->newQuery()
            ->where('name', $name)
            ->where('revoked', false)
            ->whereNull('secret')
            ->orderBy('created_at')
            ->first();

        if ($client === null) {
            return $clients->createAuthorizationCodeGrantClient($name, $redirectUris, confidential: false);
        }
        if ($client->redirect_uris !== $redirectUris) {
            $client->forceFill(['redirect_uris' => $redirectUris])->save();
        }

        return $client;
    }
}
