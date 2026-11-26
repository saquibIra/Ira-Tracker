<?php

declare(strict_types=1);

return [
    /*
     * OAuth clients created by the Time Pilot installer (desktop app, auto-tracking admin
     * page). They skip the "authorize this app" screen; every other client still asks.
     */
    'first_party_clients' => array_values(array_filter(array_map(
        'trim',
        explode(',', (string) env('TIMEPILOT_FIRST_PARTY_CLIENTS', ''))
    ))),

    /*
     * AGPL-3.0 section 13: where people using this modified solidtime get its source code.
     */
    'source_url' => (string) env('TIMEPILOT_SOURCE_URL', ''), // set by the installer (SOURCE_URL)

    /*
     * How long the "set your password" link for a new organization admin stays valid.
     */
    'admin_invite_days' => (int) env('TIMEPILOT_ADMIN_INVITE_DAYS', 7),

    /*
     * How long the set-password page opened from an employee invitation stays valid (the
     * invitation email link itself is valid for 90 days and can be opened again).
     */
    'join_link_minutes' => (int) env('TIMEPILOT_JOIN_LINK_MINUTES', 120),
];
