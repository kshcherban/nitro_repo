<template>
  <form class="npm-config" @submit.prevent="save">
    <DropDown
      v-model="selectedType"
      :options="npmTypes"
      :disabled="!isCreate"
      required
    >Repository Type</DropDown>

    <div v-if="isProxy" class="proxy-routes">
      <div
        v-for="(route, index) in proxyRoutes"
        :key="index"
        class="route-row"
      >
        <TextInput
          v-model="route.url"
          placeholder="https://registry.npmjs.org"
          required
        >Upstream URL</TextInput>
        <TextInput
          v-model="route.name"
          placeholder="Optional label"
        >Display Name</TextInput>
      <v-btn
        color="error"
        variant="text"
        class="text-none"
        type="button"
        prepend-icon="mdi-delete"
        @click="removeRoute(index)"
      >Remove</v-btn>
      </div>
      <v-btn
        color="primary"
        variant="tonal"
        class="text-none align-self-start"
        type="button"
        prepend-icon="mdi-plus"
        @click="addRoute">
        Add Route
      </v-btn>
      <ProxyCacheNotice class="mt-2" />
    </div>

    <SubmitButton
      v-if="!isCreate"
      :block="false"
      prepend-icon="mdi-content-save">
      Save
    </SubmitButton>
  </form>
</template>
<script setup lang="ts">
import { computed, defineProps, onMounted, ref, watch } from "vue";
import DropDown from "@/components/form/dropdown/DropDown.vue";
import TextInput from "@/components/form/text/TextInput.vue";
import SubmitButton from "@/components/form/SubmitButton.vue";
import http from "@/http";
import ProxyCacheNotice from "@/components/nr/repository/ProxyCacheNotice.vue";
import { defaultProxy, type NPMConfigType } from "./npm";

const npmTypes = [
  { value: "Hosted", label: "Hosted" },
  { value: "Proxy", label: "Proxy" },
];

const props = defineProps({
  settingName: String,
  repository: {
    type: String,
    required: false,
  },
});

const value = defineModel<NPMConfigType>({
  default: { type: "Hosted" },
});

const selectedType = ref<string>(value.value?.type ?? "Hosted");
const isCreate = computed(() => !props.repository);
const isProxy = computed(() => value.value?.type === "Proxy");
const proxyRoutes = computed(() => {
  if (value.value?.type !== "Proxy") {
    return [] as ReturnType<typeof defaultProxy>["routes"];
  }
  return value.value.config.routes;
});

watch(selectedType, (newType) => {
  if (newType === "Proxy") {
    if (value.value?.type === "Proxy") {
      return;
    }
    value.value = {
      type: "Proxy",
      config: defaultProxy(),
    };
  } else {
    value.value = { type: "Hosted" };
  }
});

function ensureProxyConfig() {
  if (value.value?.type !== "Proxy") {
    value.value = {
      type: "Proxy",
      config: defaultProxy(),
    };
  }
}

function addRoute() {
  ensureProxyConfig();
  if (value.value?.type === "Proxy") {
    value.value.config.routes.push({ url: "", name: "" });
  }
}

function removeRoute(index: number) {
  if (value.value?.type === "Proxy") {
    value.value.config.routes.splice(index, 1);
  }
}

async function load() {
  if (!props.repository) {
    return;
  }
  try {
    const response = await http.get(`/api/repository/${props.repository}/config/npm`);
    value.value = response.data;
    selectedType.value = value.value?.type ?? "Hosted";
  } catch (error) {
    console.error(error);
  }
}

async function save() {
  if (!props.repository) {
    return;
  }
  try {
    await http.put(`/api/repository/${props.repository}/config/npm`, value.value);
  } catch (error) {
    console.error(error);
  }
}

onMounted(() => {
  if (!value.value) {
    value.value = { type: "Hosted" };
  }
  load();
});
</script>

<style scoped lang="scss">
@use "@/assets/styles/theme.scss" as *;

.npm-config {
  display: flex;
  flex-direction: column;
  gap: 1rem;
}
.proxy-routes {
  display: flex;
  flex-direction: column;
  gap: 1rem;
}
.route-row {
  display: grid;
  gap: 0.5rem;
  grid-template-columns: repeat(auto-fit, minmax(200px, 1fr));
  align-items: end;

  :deep(.v-btn) {
    justify-self: flex-start;
  }
}
</style>
