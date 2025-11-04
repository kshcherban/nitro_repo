<template>
  <main>
    <h1>Install Page</h1>
    <form @submit.prevent="install">
      <TextInput
        id="username"
        v-model="input.username"
        autocomplete="username"
        required
        placeholder="admin"
        >Username</TextInput
      >
      <TextInput
        id="name"
        v-model="input.name"
        autocomplete="name"
        required
        placeholder="Admin User"
        >Name</TextInput
      >

      <EmailInput
        id="email"
        v-model="input.email"
        placeholder="admin@nitro-repo.dev"
        required
        >Email</EmailInput
      >
      <TwoByFormBox>
        <PasswordInput
          id="password"
          v-model="input.password"
          required
          :newPassword="true"
          >Password</PasswordInput
        >
        <PasswordInput
          id="confirmPassword"
          v-model="input.confirmPassword"
          required
          :newPassword="true"
          >Confirm Password</PasswordInput
        >
      </TwoByFormBox>
      <SubmitButton
        :disabled="installing || formValid !== ''"
        :title="installButtonTitle()"
        >Install</SubmitButton
      >
    </form>
  </main>
</template>
<script setup lang="ts">
import SubmitButton from "@/components/form/SubmitButton.vue";
import EmailInput from "@/components/form/text/EmailInput.vue";
import PasswordInput from "@/components/form/text/PasswordInput.vue";
import TextInput from "@/components/form/text/TextInput.vue";
import TwoByFormBox from "@/components/form/TwoByFormBox.vue";
import http from "@/http";
import router from "@/router";
import { siteStore } from "@/stores/site";
import { notify } from "@kyvg/vue3-notification";
import { computed, ref } from "vue";
const input = ref({
  username: "",
  email: "",
  name: "",
  password: "",
  confirmPassword: "",
});
const installing = ref(false);
const site = siteStore();
function installButtonTitle() {
  if (installing.value) {
    return "Installing...";
  }
  return formValid.value === "" ? "Install" : formValid.value;
}
const formValid = computed(() => {
  if (input.value.username === "") {
    return "Username is required.";
  }
  if (input.value.email === "") {
    return "Email is required.";
  }
  if (input.value.name === "") {
    return "Name is required.";
  }
  if (input.value.password === "") {
    return "Password is required.";
  }
  if (input.value.password !== input.value.confirmPassword) {
    return "Passwords do not match.";
  }
  return "";
});
async function install() {
  if (installing.value || formValid.value !== "") {
    return;
  }
  const newUser = {
    username: input.value.username,
    email: input.value.email,
    name: input.value.name,
    password: input.value.password,
  };
  const install = {
    user: newUser,
  };
  installing.value = true;
  try {
    const response = await http.post("/api/install", install);
    if (response.status === 204) {
      notify({
        type: "success",
        title: "Nitro Repo installed",
        text: "Redirecting to login…",
      });
      await site.getInfo();
      await router.replace({ name: "login" });
    }
  } catch (error: any) {
    console.error("Install failed", error);
    if (error?.response?.status === 404) {
      notify({
        type: "warn",
        title: "Nitro Repo already installed",
        text: "Redirecting to login…",
      });
      await router.replace({ name: "login" });
      return;
    }
    notify({
      type: "error",
      title: "Error",
      text: "An error occurred while installing the application.",
    });
  } finally {
    installing.value = false;
  }
}
</script>
<style lang="scss" scoped>
main {
  display: flex;
  flex-direction: column;
  align-items: center;
  justify-content: center;
  height: 100vh;
}
form {
}
</style>
