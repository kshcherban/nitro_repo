<template>
  <section class="s3-config">
    <TwoByFormBox>
      <TextInput
        id="s3-bucket-name"
        v-model="model.bucket_name"
        required
        autocomplete="off"
        spellcheck="false">
        Bucket Name
      </TextInput>
      <div v-if="!useCustomEndpoint" class="stacked-field">
        <DropDown
          id="s3-region"
          v-model="regionSelection"
          :options="regionOptions">
          AWS Region
        </DropDown>
        <p v-if="regionsLoading" class="helper">Loading regions…</p>
        <p v-else-if="regionError" class="helper error">{{ regionError }}</p>
        <p v-else class="helper">
          Pick the AWS region that matches your bucket. Use the custom endpoint option for
          non-AWS providers.
        </p>
      </div>
    </TwoByFormBox>

    <SwitchInput
      id="s3-use-custom-endpoint"
      v-model="useCustomEndpoint">
      Use custom endpoint
      <template #comment>
        Enable this for MinIO, Ceph, DigitalOcean Spaces, or any S3-compatible gateway with a custom
        URL.
      </template>
    </SwitchInput>

    <TwoByFormBox v-if="useCustomEndpoint">
      <TextInput
        id="s3-endpoint"
        v-model="model.endpoint"
        :required="useCustomEndpoint"
        placeholder="https://minio.internal.example.com"
        autocomplete="off"
        spellcheck="false">
        Endpoint URL
      </TextInput>
      <TextInput
        id="s3-custom-region"
        v-model="model.custom_region"
        placeholder="Optional label (onprem-us1)"
        autocomplete="off"
        spellcheck="false">
        Custom Region Name
      </TextInput>
    </TwoByFormBox>

    <TwoByFormBox>
      <TextInput
        id="s3-access-key"
        v-model="model.credentials.access_key"
        required
        autocomplete="off"
        spellcheck="false">
        Access Key
      </TextInput>
      <TextInput
        id="s3-secret-key"
        v-model="model.credentials.secret_key"
        required
        type="password"
        autocomplete="new-password">
        Secret Key
      </TextInput>
    </TwoByFormBox>

    <SwitchInput
      id="s3-path-style"
      v-model="model.path_style">
      Force path-style requests
      <template #comment>
        Keep enabled for MinIO and most custom gateways. Disable if AWS requires virtual-hosted
        style (bucket.s3.amazonaws.com).
      </template>
    </SwitchInput>
  </section>
</template>

<script setup lang="ts">
import DropDown from "@/components/form/dropdown/DropDown.vue";
import SwitchInput from "@/components/form/SwitchInput.vue";
import TextInput from "@/components/form/text/TextInput.vue";
import TwoByFormBox from "@/components/form/TwoByFormBox.vue";
import http from "@/http";
import { computed, onMounted, ref, watch, watchEffect, type Ref } from "vue";
import type { S3StorageSettings } from "@/components/nr/storage/storageTypes";

const model = defineModel<S3StorageSettings>({
  default: () => ({
    bucket_name: "",
    region: undefined,
    custom_region: undefined,
    endpoint: undefined,
    credentials: {
      access_key: "",
      secret_key: "",
    },
    path_style: true,
  }),
}) as Ref<S3StorageSettings>;

const ensureModel = (): S3StorageSettings => {
  if (!model.value) {
    model.value = {
      bucket_name: "",
      region: undefined,
      custom_region: undefined,
      endpoint: undefined,
      credentials: {
        access_key: "",
        secret_key: "",
      },
      path_style: true,
    };
  }
  return model.value;
};

const regionsLoading = ref(false);
const regionError = ref<string | null>(null);
const regionOptions = ref<{ label: string; value: string }[]>([]);
const useCustomEndpoint = ref(Boolean(ensureModel().endpoint));

const regionSelection = computed({
  get: () => ensureModel().region ?? "",
  set: (value: string) => {
    ensureModel().region = value || undefined;
  },
});

watchEffect(() => {
  const state = ensureModel();
  state.credentials ??= { access_key: "", secret_key: "" };
  if (typeof state.path_style !== "boolean") {
    state.path_style = true;
  }
});

watch(
  () => useCustomEndpoint.value,
  (enabled) => {
    const state = ensureModel();
    if (enabled) {
      state.region = undefined;
    } else {
      state.endpoint = undefined;
      state.custom_region = undefined;
      if (!state.region) {
        const firstRegion = regionOptions.value[0];
        if (firstRegion) {
          state.region = firstRegion.value;
        }
      }
    }
  },
  { immediate: true },
);

async function loadRegions() {
  regionsLoading.value = true;
  try {
    const response = await http.get<string[]>("/api/storage/s3/regions");
    regionOptions.value = response.data.map((region) => ({
      label: humanizeRegion(region),
      value: region,
    }));
    regionError.value = null;
    if (!useCustomEndpoint.value) {
      const state = ensureModel();
      if (!state.region) {
        const firstRegion = regionOptions.value[0];
        if (firstRegion) {
          state.region = firstRegion.value;
        }
      }
    }
  } catch (error) {
    console.error("Failed to load S3 regions", error);
    regionError.value = "Unable to load regions. Verify your session and try again.";
  } finally {
    regionsLoading.value = false;
  }
}

function humanizeRegion(value: string): string {
  return (
    value
      // insert space before capitals
      .replace(/([A-Z])/g, " $1")
      .replace(/\s+/g, " ")
      .trim() || value
  );
}

onMounted(() => {
  loadRegions();
});
</script>

<style scoped lang="scss">
.s3-config {
  display: flex;
  flex-direction: column;
  gap: 1rem;
}

.stacked-field {
  display: flex;
  flex-direction: column;
  gap: 0.25rem;
}

.helper {
  font-size: 0.875rem;
  color: var(--nr-text-secondary);
  margin: 0;
}

.helper.error {
  color: var(--nr-error, #c62828);
}
</style>
