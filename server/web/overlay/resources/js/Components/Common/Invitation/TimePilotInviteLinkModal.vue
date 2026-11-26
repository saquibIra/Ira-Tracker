<script setup lang="ts">
// Time Pilot: shows the link of an invitation with a Copy button (see timepilotInviteLink.ts).
import { ref, watch } from 'vue';
import DialogModal from '@/packages/ui/src/DialogModal.vue';
import PrimaryButton from '@/packages/ui/src/Buttons/PrimaryButton.vue';
import SecondaryButton from '@/packages/ui/src/Buttons/SecondaryButton.vue';
import { copyText, inviteLink } from '@/utils/timepilotInviteLink';

const copied = ref(false);
const linkInput = ref<HTMLInputElement | null>(null);

watch(
    () => inviteLink.show,
    () => {
        copied.value = false;
    }
);

async function copy() {
    if (!inviteLink.url) {
        return;
    }
    if (await copyText(inviteLink.url)) {
        copied.value = true;
        setTimeout(() => (copied.value = false), 2000);
    } else {
        // Clipboard blocked: select the link so it can be copied by hand.
        linkInput.value?.focus();
        linkInput.value?.select();
    }
}

function close() {
    inviteLink.show = false;
}
</script>

<template>
    <DialogModal closeable :show="inviteLink.show" max-width="xl" @close="close">
        <template #title>Invite link</template>

        <template #content>
            <p>
                Send this link to <strong class="text-text-primary">{{ inviteLink.email }}</strong>
                yourself, by email, chat or any way you like. They open it to choose a password and
                join your organization. It stays valid for 90 days.
            </p>
            <input
                ref="linkInput"
                data-testid="invite_link"
                :value="inviteLink.url"
                :placeholder="inviteLink.error ? '' : 'Loading…'"
                readonly
                aria-label="Invite link"
                class="mt-4 block h-9 w-full rounded-md border border-input-border bg-input-background px-3 font-mono text-xs text-text-primary shadow-sm focus-visible:border-transparent focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-ring"
                @focus="($event.target as HTMLInputElement).select()" />
            <p v-if="inviteLink.error" class="mt-2 text-sm text-destructive">
                {{ inviteLink.error }}
            </p>
        </template>

        <template #footer>
            <SecondaryButton @click="close">Done</SecondaryButton>
            <PrimaryButton
                class="ms-3"
                data-testid="invite_link_copy"
                :disabled="!inviteLink.url"
                :class="{ 'opacity-25': !inviteLink.url }"
                @click="copy">
                {{ copied ? 'Copied' : 'Copy link' }}
            </PrimaryButton>
        </template>
    </DialogModal>
</template>
