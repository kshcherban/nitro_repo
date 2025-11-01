<template>
  <div>
    <form @submit.prevent="">
      <TextInput v-model="value.type" disabled>Repository Type</TextInput>
    </form>
  </div>
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
  if (props.repository) {
    try {
      const response = await http.get(`/api/repository/${props.repository}/config/php`);
      value.value = response.data;
    } catch (error) {
      console.error(error);
    }
  }
});
</script>
