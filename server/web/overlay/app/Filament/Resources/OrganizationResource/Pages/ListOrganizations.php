<?php

declare(strict_types=1);

namespace App\Filament\Resources\OrganizationResource\Pages;

use App\Filament\Resources\OrganizationResource;
use App\Filament\TimePilotPasswordLink;
use App\Models\Organization;
use App\Service\TimePilotOnboardingService;
use Brick\Money\IsoCurrencyProvider;
use Filament\Actions\Action;
use Filament\Actions\CreateAction;
use Filament\Forms\Components\Select;
use Filament\Forms\Components\TextInput;
use Filament\Notifications\Notification;
use Filament\Resources\Pages\ListRecords;
use Throwable;

/**
 * Time Pilot: the Super Admin creates customer organizations here, each with an admin who
 * becomes the owner. The Super Admin then copies the admin's set-password link and sends it
 * to them (or it is emailed, when email is on).
 */
class ListOrganizations extends ListRecords
{
    protected static string $resource = OrganizationResource::class;

    protected function getHeaderActions(): array
    {
        return [
            Action::make('newOrganization')
                ->label('New organization')
                ->icon('heroicon-s-plus')
                ->modalHeading('New organization')
                ->modalDescription('Creates the organization and its admin, who becomes the owner (you do not become a member). Next you get a link to send to the admin, to choose a password.')
                ->modalSubmitActionLabel('Create organization')
                ->schema([
                    TextInput::make('organization')
                        ->label('Organization name')
                        ->required()
                        ->maxLength(255),
                    TextInput::make('admin_name')
                        ->label('Admin name')
                        ->required()
                        ->maxLength(255),
                    TextInput::make('admin_email')
                        ->label('Admin email')
                        ->email()
                        ->required()
                        ->maxLength(255),
                    Select::make('currency')
                        ->label('Currency')
                        ->options(self::currencies())
                        ->default(config('app.localization.default_currency', 'USD'))
                        ->searchable()
                        ->required(),
                ])
                ->action(function (array $data): void {
                    try {
                        $organization = app(TimePilotOnboardingService::class)->createOrganizationWithAdmin(
                            (string) $data['organization'],
                            (string) $data['admin_name'],
                            (string) $data['admin_email'],
                            (string) $data['currency'],
                        );
                    } catch (Throwable $exception) {
                        report($exception);
                        Notification::make()
                            ->title('Could not finish')
                            ->body($exception->getMessage())
                            ->danger()
                            ->persistent()
                            ->send();

                        return;
                    }
                    Notification::make()
                        ->title('Organization created')
                        ->body("{$organization->name} is ready. Send the link to its admin.")
                        ->success()
                        ->send();
                    $this->replaceMountedAction('adminLink', ['organization' => $organization->getKey()]);
                }),
            Action::make('findAdminLink')
                ->label('Admin link')
                ->icon('heroicon-o-link')
                ->color('gray')
                ->modalHeading('Admin link')
                ->modalDescription('A new link for an organization\'s admin: to choose their first password, or a new one if they forgot it.')
                ->modalSubmitActionLabel('Show link')
                ->schema([
                    Select::make('organization_id')
                        ->label('Organization')
                        ->options(fn (): array => self::customerOrganizations())
                        ->searchable()
                        ->required(),
                ])
                ->action(function (array $data): void {
                    $this->replaceMountedAction('adminLink', ['organization' => $data['organization_id']]);
                }),
            Action::make('resendAdminEmail')
                ->label('Resend admin email')
                ->icon('heroicon-o-envelope')
                ->color('gray')
                ->visible(fn (): bool => TimePilotOnboardingService::mailEnabled())
                ->modalDescription('Sends the admin a new set-password link, or a sign-in link if they already chose a password.')
                ->modalSubmitActionLabel('Send email')
                ->schema([
                    Select::make('organization_id')
                        ->label('Organization')
                        ->options(fn (): array => self::customerOrganizations())
                        ->searchable()
                        ->required(),
                ])
                ->action(function (array $data): void {
                    $organization = Organization::query()->findOrFail($data['organization_id']);
                    try {
                        app(TimePilotOnboardingService::class)->sendAdminEmail($organization);
                        Notification::make()
                            ->title('Email sent')
                            ->body("Sent to {$organization->owner->email}.")
                            ->success()
                            ->send();
                    } catch (Throwable $exception) {
                        report($exception);
                        Notification::make()
                            ->title('Could not send the email')
                            ->body($exception->getMessage())
                            ->danger()
                            ->persistent()
                            ->send();
                    }
                }),
            CreateAction::make()
                ->label('Create for existing user')
                ->color('gray')
                ->icon('heroicon-o-plus'),
        ];
    }

    /** The admin's password link; opened after "New organization" and "Admin link". */
    public function adminLinkAction(): Action
    {
        return TimePilotPasswordLink::organizationAdminAction('adminLink');
    }

    /**
     * Organizations of customers (not the personal ones solidtime creates per account).
     *
     * @return array<string, string>
     */
    private static function customerOrganizations(): array
    {
        return Organization::query()
            ->where('personal_team', '=', false)
            ->orderBy('name')
            ->pluck('name', 'id')
            ->all();
    }

    /**
     * @return array<string, string>
     */
    private static function currencies(): array
    {
        $options = [];
        foreach (IsoCurrencyProvider::getInstance()->getAvailableCurrencies() as $currency) {
            $options[$currency->getCurrencyCode()] = $currency->getName().' ('.$currency->getCurrencyCode().')';
        }

        return $options;
    }
}
