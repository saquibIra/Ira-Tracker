<script setup lang="ts">
// Time Pilot: the sidebar card links to the desktop app and, for admins, the
// auto-tracking settings. It replaces solidtime's release notes.
import { ComputerDesktopIcon, SparklesIcon } from '@heroicons/vue/20/solid';
import { canUpdateOrganization } from '@/utils/permissions';
</script>

<template>
    <div class="py-4 hidden lg:block">
        <div class="rounded-lg border border-border-secondary bg-card-background px-3 py-2.5">
            <div class="flex items-center gap-2 pb-2">
                <img src="/images/timepilot-mark.svg" alt="" class="h-5 w-5" />
                <span class="text-xs font-semibold text-text-primary">Time Pilot</span>
            </div>
            <a
                href="/download/"
                class="flex items-center gap-1.5 rounded px-1 py-1 text-xs text-text-secondary hover:bg-card-background-active hover:text-text-primary">
                <ComputerDesktopIcon class="w-3.5 text-icon-default" />
                Get the desktop app
            </a>
            <a
                v-if="canUpdateOrganization()"
                href="/autotrack/admin/"
                class="flex items-center gap-1.5 rounded px-1 py-1 text-xs text-text-secondary hover:bg-card-background-active hover:text-text-primary">
                <SparklesIcon class="w-3.5 text-icon-default" />
                Auto-tracking settings
            </a>
            <a
                href="/source"
                class="mt-1 block px-1 text-2xs text-text-quaternary hover:text-text-secondary"
                >Source code (AGPL)</a
            >
        </div>
    </div>
</template>
