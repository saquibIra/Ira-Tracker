// Time Pilot: the "share this invite link" dialog on the Members page. Email is off, so
// the admin copies the link of a pending invitation and sends it themselves.
import axios from 'axios';
import { reactive } from 'vue';
import { getCurrentOrganizationId } from '@/utils/useUser';
import { useInvitationsStore } from '@/utils/useInvitations';

export const inviteLink = reactive({
    show: false,
    email: '',
    url: '',
    error: '',
});

/** Opens the dialog for an invitation, by id or (right after inviting) by email. */
export async function openInviteLink(target: { id?: string; email: string }): Promise<void> {
    const organization = getCurrentOrganizationId();
    if (!organization) {
        return;
    }
    Object.assign(inviteLink, { show: true, email: target.email, url: '', error: '' });
    try {
        let id = target.id;
        if (!id) {
            // Right after inviting: reload the Invitations list (it may already be on screen)
            // and find the new invitation in it.
            const store = useInvitationsStore();
            await store.fetchInvitations();
            const email = target.email.trim().toLowerCase();
            id = store.invitations.find((i) => i.email.toLowerCase() === email)?.id;
        }
        if (!id) {
            throw new Error('invitation not found');
        }
        const response = await axios.get<{ data: { email: string; url: string } }>(
            `/api/v1/organizations/${encodeURIComponent(organization)}/invitations/${encodeURIComponent(id)}/link`,
            { headers: { Accept: 'application/json' } }
        );
        inviteLink.url = response.data.data.url;
    } catch {
        inviteLink.error =
            'The invite link could not be loaded. Open it again from the Invitations tab.';
    }
}

/** Copies text to the clipboard; false if the browser refused. */
export async function copyText(text: string): Promise<boolean> {
    try {
        await navigator.clipboard.writeText(text);
        return true;
    } catch {
        return false;
    }
}
