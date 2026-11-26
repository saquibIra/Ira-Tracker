<?php

declare(strict_types=1);

namespace App\Filament;

use App\Http\Controllers\Web\TimePilotJoinController;
use App\Models\Organization;
use App\Models\User;
use Closure;
use Filament\Actions\Action;
use Filament\Forms\Components\TextInput;

/**
 * Time Pilot: a modal in the Super Admin panel that shows a fresh "choose a password" link
 * with a copy button, for the Super Admin to share (email is off): the first password of a
 * new organization admin, or a new one for someone who forgot theirs.
 */
final class TimePilotPasswordLink
{
    /** Row action on Users. */
    public static function userAction(): Action
    {
        return self::modal(
            Action::make('passwordLink')
                ->label('Password link')
                ->icon('heroicon-o-key')
                ->color('gray')
                ->hidden(fn (User $record): bool => $record->is_placeholder),
            fn (User $record): User => $record,
        );
    }

    /** The owner's link of the organization given as the "organization" argument. */
    public static function organizationAdminAction(string $name): Action
    {
        return self::modal(
            Action::make($name),
            fn (array $arguments): User => Organization::query()
                ->findOrFail($arguments['organization'] ?? null)
                ->owner,
        );
    }

    /**
     * @param  Closure(mixed ...): User  $user  evaluated by the action (gets $record, $arguments)
     */
    private static function modal(Action $action, Closure $user): Action
    {
        $days = config()->integer('timepilot.admin_invite_days', 7);

        return $action
            ->modalHeading(fn (Action $action): string => 'Password link for '.$action->evaluate($user)->name)
            ->modalDescription(function (Action $action) use ($user, $days): string {
                $person = $action->evaluate($user);
                if ($person->password === null) {
                    return "Send this link to {$person->email} yourself (email, chat, ...). "
                        ."They open it to choose a password and sign in. It works once and expires in {$days} days.";
                }

                return "{$person->name} already has a password. If they forgot it, send them this link to choose a new one. "
                    ."It works once and expires in {$days} days; their current password keeps working until then.";
            })
            ->fillForm(fn (Action $action): array => [
                'link' => TimePilotJoinController::welcomeUrl($action->evaluate($user)),
            ])
            ->schema([
                TextInput::make('link')
                    ->label('Link')
                    ->readOnly()
                    ->copyable(copyMessage: 'Copied')
                    ->extraInputAttributes(['data-testid' => 'password-link']),
            ])
            ->modalSubmitAction(false)
            ->modalCancelActionLabel('Done');
    }
}
