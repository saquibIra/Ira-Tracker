<script setup lang="ts">
import { Link } from '@inertiajs/vue3';
</script>

<template>
    <Link :href="'/'" class="inline-flex items-center gap-2.5" aria-label="Time Pilot">
        <img src="/images/timepilot-mark.svg" alt="" class="h-9 w-9" />
        <span class="text-lg font-semibold tracking-tight text-text-primary">Time Pilot</span>
    </Link>
</template>
