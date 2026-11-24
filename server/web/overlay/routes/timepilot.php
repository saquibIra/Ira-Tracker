<?php

declare(strict_types=1);

/*
 * Time Pilot routes, loaded at the end of routes/web.php (so inside the "web" group).
 */

use App\Enums\RegistrationMode;
use App\Http\Controllers\Web\TimePilotJoinController;
use Illuminate\Http\Request;
use Illuminate\Support\Facades\Route;
use Laravel\Fortify\Http\Controllers\RegisteredUserController;

// People join by invitation. With sign-up off, the sign-up page leads to sign-in instead of
// showing a form that would be refused (creating the account is refused either way).
Route::get('/register', function (Request $request) {
    if (RegistrationMode::fromConfig(config('app.enable_registration')) === RegistrationMode::Off) {
        return redirect()->route('login');
    }

    return app(RegisteredUserController::class)->create($request);
})->middleware(['guest:'.config('fortify.guard')])->name('register');

// AGPL-3.0 section 13: source code of this modified version.
Route::get('/source', function () {
    return redirect()->away((string) config('timepilot.source_url'));
})->name('timepilot.source');

// Joining by invitation and the first password of a new organization admin. Both links are
// signed (relative, so they survive the HTTPS proxy) and expire.
Route::middleware(['guest', 'signed:relative', 'throttle:20,1'])->group(function (): void {
    Route::get('/join/{invitation}', [TimePilotJoinController::class, 'showInvitation'])
        ->name('timepilot.join');
    Route::post('/join/{invitation}', [TimePilotJoinController::class, 'acceptInvitation'])
        ->name('timepilot.join.store');
    Route::get('/welcome/{user}/{check}', [TimePilotJoinController::class, 'showWelcome'])
        ->name('timepilot.welcome');
    Route::post('/welcome/{user}/{check}', [TimePilotJoinController::class, 'completeWelcome'])
        ->name('timepilot.welcome.store');
});
