<template>
  <form class="deb-config" @submit.prevent="save">
    <v-combobox
      v-model="value.distributions"
      multiple
      chips
      clearable
      persistent-hint
      hide-details="auto"
      label="Distributions"
      hint="Suites available for clients (e.g. stable, testing)"
    />
    <v-combobox
      v-model="value.components"
      multiple
      chips
      clearable
      persistent-hint
      hide-details="auto"
      label="Components"
      hint="Logical sections like main, contrib, non-free"
    />
    <v-combobox
      v-model="value.architectures"
      multiple
      chips
      clearable
      persistent-hint
      hide-details="auto"
      label="Architectures"
      hint="Architectures accepted on upload (e.g. amd64, arm64, all)"
    />
    <SubmitButton v-if="!isCreate" :block="false" prepend-icon="mdi-content-save">
      Save
    </SubmitButton>
  </form>
</template>

<script setup lang="ts">
import { computed, onMounted } from "vue";
import SubmitButton from "@/components/form/SubmitButton.vue";
import http from "@/http";
import { defaultDebConfig, type DebRepositoryConfig } from "./deb";

const props = defineProps({
  repository: {
    type: String,
    required: false,
  },
});

const value = defineModel<DebRepositoryConfig>({
  default: defaultDebConfig(),
});

const isCreate = computed(() => !props.repository);

function normalize() {
  if (!value.value) {
    value.value = defaultDebConfig();
    return;
  }
  if (!Array.isArray(value.value.distributions) || value.value.distributions.length === 0) {
    value.value.distributions = ["stable"];
  }
  if (!Array.isArray(value.value.components) || value.value.components.length === 0) {
    value.value.components = ["main"];
  }
  if (!Array.isArray(value.value.architectures) || value.value.architectures.length === 0) {
    value.value.architectures = ["amd64", "all"];
  }
}

async function load() {
  if (!props.repository) {
    return;
  }
  try {
    const response = await http.get(`/api/repository/${props.repository}/config/deb`);
    value.value = response.data ?? defaultDebConfig();
    normalize();
  } catch (error) {
    console.error(error);
  }
}

async function save() {
  if (!props.repository) {
    return;
  }
  try {
    await http.put(`/api/repository/${props.repository}/config/deb`, value.value);
  } catch (error) {
    console.error(error);
  }
}

onMounted(() => {
  normalize();
  load();
});
</script>

<style scoped lang="scss">
.deb-config {
  display: flex;
  flex-direction: column;
  gap: 1rem;
}
</style>
