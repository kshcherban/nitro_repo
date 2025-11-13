<template>
  <div
    v-if="alerts.length"
    class="global-alerts">
    <transition-group name="global-alert" tag="div">
      <v-alert
        v-for="alert in alerts"
        :key="alert.id"
        :type="alert.kind"
        variant="tonal"
        border="start"
        density="comfortable"
        class="global-alert"
        closable
        @click:close="dismiss(alert.id)">
        <div class="global-alert__title">{{ alert.title }}</div>
        <div v-if="alert.message" class="global-alert__message">{{ alert.message }}</div>
      </v-alert>
    </transition-group>
  </div>
</template>

<script setup lang="ts">
import { storeToRefs } from "pinia";
import { useAlertsStore } from "@/stores/alerts";

const alertsStore = useAlertsStore();
const { alerts } = storeToRefs(alertsStore);

function dismiss(id: number) {
  alertsStore.dismiss(id);
}
</script>

<style scoped lang="scss">
.global-alerts {
  position: fixed;
  top: 1.5rem;
  right: 1.5rem;
  display: flex;
  flex-direction: column;
  gap: 0.75rem;
  width: min(360px, calc(100vw - 2rem));
  z-index: 2200;
}

.global-alert__title {
  font-weight: 600;
  margin-bottom: 0.25rem;
}

.global-alert__message {
  font-size: 0.95rem;
}

.global-alert-enter-from,
.global-alert-leave-to {
  opacity: 0;
  transform: translateX(12px);
}

.global-alert-enter-active,
.global-alert-leave-active {
  transition: opacity 0.2s ease, transform 0.2s ease;
}

.global-alert-leave-active {
  position: absolute;
}

@media (max-width: 600px) {
  .global-alerts {
    left: 1rem;
    right: 1rem;
    width: auto;
  }
}
</style>
