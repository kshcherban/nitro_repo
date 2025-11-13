<template>
  <section class="maven-proxy">
    <div class="maven-proxy__header">
      <h3 class="text-subtitle-1 font-weight-medium mb-2">Upstream Routes</h3>
      <p class="text-body-2 text-medium-emphasis">
        Routes are tried in order to fetch artifacts from remote Maven repositories.
      </p>
    </div>

    <div class="maven-proxy__routes" v-auto-animate>
      <div
        v-for="(route, index) in value.routes"
        :key="`${route.url}-${index}`"
        class="maven-proxy__route">
        <v-row dense>
          <v-col cols="12" md="7">
            <TextInput
              v-model="route.url"
              required
              placeholder="https://repo1.maven.org/maven2/">
              Upstream URL
            </TextInput>
          </v-col>
          <v-col cols="12" md="4">
            <TextInput
              v-model="route.name"
              placeholder="Maven Central">
              Display Name
            </TextInput>
          </v-col>
          <v-col
            cols="12"
            md="1"
            class="d-flex align-end justify-end">
            <v-btn
              color="error"
              variant="text"
              class="text-none"
              :disabled="value.routes.length <= 1"
              prepend-icon="mdi-delete"
              @click="removeRoute(index)">
              Remove
            </v-btn>
          </v-col>
        </v-row>
      </div>
    </div>

    <div class="maven-proxy__add mt-4">
      <v-row dense>
        <v-col cols="12" md="7">
          <TextInput
            v-model="draft.url"
            placeholder="https://repo1.maven.org/maven2/"
            required>
            Upstream URL
          </TextInput>
        </v-col>
        <v-col cols="12" md="4">
          <TextInput
            v-model="draft.name"
            placeholder="Maven Central">
            Display Name
          </TextInput>
        </v-col>
        <v-col
          cols="12"
          md="1"
          class="d-flex align-end justify-end">
          <v-btn
            color="primary"
            variant="tonal"
            class="text-none"
            :disabled="!draft.url.trim()"
            prepend-icon="mdi-plus"
            @click="addRoute">
            Add
          </v-btn>
        </v-col>
      </v-row>
    </div>
  </section>
</template>

<script setup lang="ts">
import { reactive } from "vue";
import { useAlertsStore } from "@/stores/alerts";
import TextInput from "@/components/form/text/TextInput.vue";
import { defaultProxy, type MavenProxyRoute, type MavenProxyConfigType } from "./maven";

const value = defineModel<MavenProxyConfigType>({
  required: true,
});

if (!value.value || !Array.isArray(value.value.routes)) {
  value.value = defaultProxy();
} else if (value.value.routes.length === 0) {
  value.value = defaultProxy();
}

const draft = reactive<MavenProxyRoute>({
  url: "",
  name: "",
});
const alerts = useAlertsStore();

function removeRoute(index: number) {
  if (index < 0 || index >= value.value.routes.length) {
    return;
  }
  value.value = {
    ...value.value,
    routes: value.value.routes.filter((_, i) => i !== index),
  };
}

function addRoute() {
  const trimmedUrl = draft.url.trim();
  if (!trimmedUrl) {
    return;
  }
  try {
    // Validate URL format
    new URL(trimmedUrl);
  } catch (error) {
    console.error("Invalid Maven proxy URL", error);
    alerts.error("Invalid URL", "Provide a valid upstream Maven repository URL.");
    return;
  }
  value.value = {
    ...value.value,
    routes: [
      ...value.value.routes,
      {
        url: trimmedUrl,
        name: (draft.name ?? "").trim() || undefined,
      },
    ],
  };
  draft.url = "";
  draft.name = "";
}
</script>

<style scoped lang="scss">
.maven-proxy {
  display: flex;
  flex-direction: column;
  gap: 1rem;

  &__routes {
    display: flex;
    flex-direction: column;
    gap: 1rem;
  }

  &__route,
  &__add {
    padding: 0;
    border-radius: 0;
    background-color: transparent;
    border: none;
    box-shadow: none;
  }
}
</style>
