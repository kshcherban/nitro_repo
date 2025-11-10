<template>
  <v-card
    class="config-card"
    data-testid="php-config-card">
    <v-card-text class="px-0">
      <v-row dense>
        <v-col cols="12" md="6">
          <TextInput
            v-model="value.type"
            disabled
            id="php-repository-type">
            Repository Type
          </TextInput>
        </v-col>
      </v-row>
      <v-alert
        variant="tonal"
        type="info"
        class="mt-4"
        density="comfortable">
        PHP repositories currently operate in Hosted mode; no additional configuration is required.
      </v-alert>
    </v-card-text>
  </v-card>
</template>

<script setup lang="ts">
import { onMounted } from "vue";
import TextInput from "@/components/form/text/TextInput.vue";
import http from "@/http";
import type { PhpConfigType } from "./php";

const props = defineProps({
  settingName: String,
  repository: {
    type: String,
    required: false,
  },
});

const value = defineModel<PhpConfigType>({
  default: { type: "Hosted" },
});

onMounted(async () => {
  if (!props.repository) {
    return;
  }
  try {
    const response = await http.get(`/api/repository/${props.repository}/config/php`);
    if (response?.data) {
      value.value = response.data;
    }
  } catch (error) {
    console.error("Failed to load PHP config", error);
  }
});
</script>

<style scoped lang="scss">
.config-card {
  border: none;
  box-shadow: none;
  background-color: transparent;
}
</style>
