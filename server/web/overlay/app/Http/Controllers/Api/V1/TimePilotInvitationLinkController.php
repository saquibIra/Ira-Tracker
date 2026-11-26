<?php

declare(strict_types=1);

namespace App\Http\Controllers\Api\V1;

use App\Http\Controllers\Web\TimePilotJoinController;
use App\Models\Organization;
use App\Models\OrganizationInvitation;
use Illuminate\Auth\Access\AuthorizationException;
use Illuminate\Http\JsonResponse;

/**
 * Time Pilot: the link of a pending invitation, so the admin can share it themselves
 * (Members > Invitations > Copy invite link). Whoever may resend the invitation email may
 * see its link.
 */
class TimePilotInvitationLinkController extends Controller
{
    /**
     * @throws AuthorizationException
     */
    public function show(Organization $organization, OrganizationInvitation $invitation): JsonResponse
    {
        $this->checkPermission($organization, 'invitations:resend');
        if ($invitation->organization_id !== $organization->getKey()) {
            throw new AuthorizationException('Invitation does not belong to organization');
        }

        return response()->json([
            'data' => [
                'email' => $invitation->email,
                'url' => TimePilotJoinController::invitationUrl($invitation),
            ],
        ]);
    }
}
