<template>
  <section class="s3-config">
    <TwoByFormBox>
      <TextInput
        id="s3-bucket-name-display"
        v-model="model.bucket_name"
        disabled>
        Bucket Name
      </TextInput>
      <TextInput
        id="s3-region-display"
        v-model="regionDisplay"
        disabled>
        Region / Endpoint Mode
      </TextInput>
    </TwoByFormBox>

    <TwoByFormBox v-if="model.endpoint">
      <TextInput
        id="s3-endpoint-display"
        v-model="endpointDisplay"
        disabled>
        Endpoint URL
      </TextInput>
      <TextInput
        id="s3-custom-region-display"
        v-model="customRegionDisplay"
        disabled>
        Custom Region Name
      </TextInput>
    </TwoByFormBox>

    <TwoByFormBox>
      <TextInput
        id="s3-access-key-display"
        v-model="model.credentials.access_key"
        disabled>
        Access Key
      </TextInput>
      <TextInput
        id="s3-secret-key-display"
        :model-value="maskedSecret"
        type="password"
        disabled>
        Secret Key
      </TextInput>
    </TwoByFormBox>

    <TextInput
      id="s3-path-style-display"
      :model-value="model.path_style ? 'Path-style' : 'Virtual-hosted'"
      disabled>
      Addressing Mode
    </TextInput>
  </section>
</template>

<script setup lang="ts">
import TextInput from "@/components/form/text/TextInput.vue";
import TwoByFormBox from "@/components/form/TwoByFormBox.vue";
import { computed } from "vue";
import type { S3StorageSettings } from "@/components/nr/storage/storageTypes";

const model = defineModel<S3StorageSettings>({
  required: true,
});

if (!model.value.credentials) {
  model.value.credentials = {};
}
if (typeof model.value.path_style !== "boolean") {
  model.value.path_style = true;
}

const regionDisplay = computed({
  get: () => {
    if (model.value.endpoint) {
      return "Custom endpoint";
    }
    return model.value.region ?? "Region not set";
  },
  set: () => {},
});

const endpointDisplay = computed({
  get: () => model.value.endpoint ?? "",
  set: () => {},
});

const customRegionDisplay = computed({
  get: () => model.value.custom_region ?? "",
  set: () => {},
});

const maskedSecret = computed(() => {
  const secret = model.value.credentials?.secret_key ?? "";
  if (!secret) {
    return "";
  }
  return "*".repeat(Math.min(secret.length, 12));
});
</script>

<style scoped lang="scss">
.s3-config {
  display: flex;
  flex-direction: column;
  gap: 1rem;
}
</style>
