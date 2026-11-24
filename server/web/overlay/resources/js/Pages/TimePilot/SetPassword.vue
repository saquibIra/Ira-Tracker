<script setup lang="ts">
// Time Pilot: the page behind an invitation link (employees) and a welcome link
// (organization admins). The person chooses a password and is signed in.
import { Head, useForm } from '@inertiajs/vue3';
import AuthenticationCard from '@/Components/AuthenticationCard.vue';
import AuthenticationCardLogo from '@/Components/AuthenticationCardLogo.vue';
import { Field, FieldLabel, FieldError } from '@/packages/ui/src/field';
import PrimaryButton from '@/packages/ui/src/Buttons/PrimaryButton.vue';
import TextInput from '@/packages/ui/src/Input/TextInput.vue';

const props = defineProps<{
    title: string;
    intro: string;
    email: string;
    name: string;
    action: string;
    submitLabel: string;
}>();

function browserTimezone(): string {
    try {
        return Intl.DateTimeFormat().resolvedOptions().timeZone || 'UTC';
    } catch {
        return 'UTC';
    }
}

const form = useForm({
    name: props.name,
    password: '',
    password_confirmation: '',
    timezone: browserTimezone(),
});

const submit = () => {
    form.post(props.action, {
        onFinish: () => form.reset('password', 'password_confirmation'),
    });
};
</script>

<template>
    <Head :title="title" />

    <AuthenticationCard>
        <template #logo>
            <AuthenticationCardLogo />
        </template>

        <h1 class="text-xl font-semibold tracking-tight text-text-primary">{{ title }}</h1>
        <p class="mt-1.5 text-sm text-text-secondary">{{ intro }}</p>

        <form class="mt-6" @submit.prevent="submit">
            <Field>
                <FieldLabel for="email">Email</FieldLabel>
                <TextInput
                    id="email"
                    :model-value="email"
                    type="email"
                    class="block w-full opacity-70"
                    readonly
                    autocomplete="username" />
            </Field>

            <Field class="mt-4">
                <FieldLabel for="name">Your name</FieldLabel>
                <TextInput
                    id="name"
                    v-model="form.name"
                    type="text"
                    class="block w-full"
                    required
                    :autofocus="!name"
                    autocomplete="name" />
                <FieldError v-if="form.errors.name">{{ form.errors.name }}</FieldError>
            </Field>

            <Field class="mt-4">
                <FieldLabel for="password">Password</FieldLabel>
                <TextInput
                    id="password"
                    v-model="form.password"
                    type="password"
                    class="block w-full"
                    required
                    :autofocus="!!name"
                    autocomplete="new-password" />
                <FieldError v-if="form.errors.password">{{ form.errors.password }}</FieldError>
            </Field>

            <Field class="mt-4">
                <FieldLabel for="password_confirmation">Confirm password</FieldLabel>
                <TextInput
                    id="password_confirmation"
                    v-model="form.password_confirmation"
                    type="password"
                    class="block w-full"
                    required
                    autocomplete="new-password" />
                <FieldError v-if="form.errors.password_confirmation">{{
                    form.errors.password_confirmation
                }}</FieldError>
            </Field>

            <FieldError v-if="form.errors.timezone" class="mt-4">{{ form.errors.timezone }}</FieldError>

            <PrimaryButton
                class="mt-6 w-full justify-center"
                :class="{ 'opacity-25': form.processing }"
                :disabled="form.processing">
                {{ submitLabel }}
            </PrimaryButton>
        </form>
    </AuthenticationCard>
</template>
