<script setup lang="ts">
import { onMounted } from 'vue';
import { useTheme } from '@/utils/theme.js';

onMounted(async () => {
    useTheme();
});

// Time Pilot: shown beside every sign-in, password and invitation page on large screens.
const highlights = [
    {
        title: 'Tracks itself',
        text: 'The desktop app recognises the project from the work on screen. No timers to start or stop.',
    },
    {
        title: 'Private by design',
        text: "Screenshots stay on each person's own computer. Only projects and times are shared.",
    },
    {
        title: 'Reviewed in a minute',
        text: 'At the end of the day everyone checks their time, fixes anything, and submits.',
    },
];
</script>

<template>
    <div class="min-h-screen bg-default-background lg:grid lg:grid-cols-[minmax(0,5fr)_minmax(0,6fr)]">
        <aside
            class="tp-brand relative hidden overflow-hidden lg:flex flex-col justify-between p-12 xl:p-16 text-white">
            <svg
                class="tp-dial pointer-events-none absolute -right-40 -bottom-40 h-[34rem] w-[34rem]"
                viewBox="0 0 48 48"
                fill="none"
                aria-hidden="true">
                <circle cx="24" cy="24" r="22" stroke="currentColor" stroke-width="0.25" />
                <circle cx="24" cy="24" r="14.5" stroke="currentColor" stroke-width="0.4" />
                <path d="M24 24V12" stroke="currentColor" stroke-width="0.6" stroke-linecap="round" />
                <path d="M24 24L33 18.8" stroke="currentColor" stroke-width="0.6" stroke-linecap="round" />
            </svg>

            <div class="relative flex items-center gap-3">
                <img src="/images/timepilot-mark.svg" alt="" class="h-10 w-10" />
                <span class="text-xl font-semibold tracking-tight">Time Pilot</span>
            </div>

            <div class="relative max-w-md">
                <h2 class="text-3xl font-semibold leading-tight tracking-tight text-balance">
                    Time tracking that keeps up with your team.
                </h2>
                <ul class="mt-10 space-y-6">
                    <li v-for="item in highlights" :key="item.title" class="flex gap-3">
                        <span class="mt-2 h-1.5 w-1.5 shrink-0 rounded-full bg-accent-300"></span>
                        <div>
                            <p class="font-semibold">{{ item.title }}</p>
                            <p class="mt-1 text-sm text-white/70">{{ item.text }}</p>
                        </div>
                    </li>
                </ul>
            </div>

            <p class="relative text-xs text-white/50">
                Time Pilot ·
                <a href="/source" class="underline underline-offset-2 hover:text-white/80"
                    >Source code</a
                >
            </p>
        </aside>

        <main class="flex min-h-screen flex-col items-center justify-center px-4 py-10 sm:px-10">
            <div class="w-full max-w-md">
                <div class="mb-6 flex justify-center lg:justify-start">
                    <slot name="logo" />
                </div>

                <div
                    class="rounded-xl border border-card-border bg-card-background px-6 py-7 shadow-card sm:px-8">
                    <slot />
                </div>

                <slot name="actions"></slot>

                <p class="mt-8 text-center text-xs text-text-quaternary lg:hidden">
                    Time Pilot ·
                    <a href="/source" class="underline underline-offset-2 hover:text-text-secondary"
                        >Source code</a
                    >
                </p>
            </div>
        </main>
    </div>
</template>

<style scoped>
.tp-brand {
    background:
        radial-gradient(circle at 85% 12%, rgba(45, 212, 191, 0.22), transparent 42%),
        radial-gradient(circle at 8% 92%, rgba(13, 148, 136, 0.3), transparent 48%),
        #0b1a2b;
}
.tp-dial {
    color: rgba(255, 255, 255, 0.07);
}
</style>
