@component('mail::message')
# {{ __('Join :organization on Time Pilot', ['organization' => $invitation->organization->name]) }}

{{ __(':organization uses Time Pilot to track time on its projects. Accept the invitation and choose a password to get started.', ['organization' => $invitation->organization->name]) }}

@component('mail::button', ['url' => $acceptUrl])
{{ __('Accept invitation') }}
@endcomponent

{{ __('If you did not expect this invitation, you can ignore this email.') }}
@endcomponent
