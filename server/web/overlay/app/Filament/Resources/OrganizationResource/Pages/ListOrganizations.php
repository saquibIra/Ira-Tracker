<?php

declare(strict_types=1);

namespace App\Filament\Resources\OrganizationResource\Pages;

use App\Filament\Resources\OrganizationResource;
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
 * gets an email to set a password and becomes the owner.
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
                ->modalDescription('Creates the organization and emails its admin a link to set a password. The admin becomes the owner; you do not become a member.')
                ->modalSubmitActionLabel('Create and send email')
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
                        Notification::make()
                            ->title('Organization created')
                            ->body("{$organization->name} is ready. An email was sent to {$data['admin_email']}.")
                            ->success()
                            ->send();
                    } catch (Throwable $exception) {
                        report($exception);
                        Notification::make()
                            ->title('Could not finish')
                            ->body($exception->getMessage())
                            ->danger()
                            ->persistent()
                            ->send();
                    }
                }),
            Action::make('resendAdminEmail')
                ->label('Resend admin email')
                ->icon('heroicon-o-envelope')
                ->color('gray')
                ->modalDescription('Sends the admin a new set-password link, or a sign-in link if they already chose a password.')
                ->modalSubmitActionLabel('Send email')
                ->schema([
                    Select::make('organization_id')
                        ->label('Organization')
                        ->options(fn (): array => Organization::query()
                            ->where('personal_team', '=', false)
                            ->orderBy('name')
                            ->pluck('name', 'id')
                            ->all())
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
