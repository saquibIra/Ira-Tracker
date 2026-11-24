@component('mail::message')
# {{ __('Welcome to Time Pilot, :name', ['name' => $adminName]) }}

{{ __('You are the admin of :organization on Time Pilot.', ['organization' => $organizationName]) }}
@if ($setPassword)
{{ __('Choose a password to sign in. Then add your projects and invite your team.') }}
@else
{{ __('Sign in with your Time Pilot account to add your projects and invite your team.') }}
@endif

@component('mail::button', ['url' => $url])
{{ $setPassword ? __('Set your password') : __('Sign in') }}
@endcomponent

@if ($setPassword)
{{ __('The link works once and expires in :days days. If it has expired, ask your Time Pilot contact to send a new one.', ['days' => $days]) }}
@endif
@endcomponent
