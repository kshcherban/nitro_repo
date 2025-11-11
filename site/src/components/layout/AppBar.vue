<template>
  <v-app-bar
    :elevation="1"
    color="white"
    app
    class="app-bar">
    <v-container fluid class="d-flex align-center pa-0 app-bar__inner">
      <router-link
        to="/"
        class="d-flex align-center text-decoration-none logo-link">
        <v-avatar
          :image="'/icon-128.png'"
          size="40"
          class="mr-3" />
        <span class="text-h6 font-weight-medium text-primary">Nitro Repository</span>
      </router-link>

      <v-spacer />

      <!-- User section -->
      <div v-if="user" class="d-flex align-center">
        <v-menu offset-y>
          <template v-slot:activator="{ props }">
            <v-btn
              v-bind="props"
              variant="text"
              class="text-none">
              <v-icon start>mdi-account-circle</v-icon>
              {{ user.username }}
              <v-icon end>mdi-menu-down</v-icon>
            </v-btn>
          </template>
          <v-list>
            <v-list-item
              :to="{ name: 'profile' }"
              prepend-icon="mdi-account">
              <v-list-item-title>Profile</v-list-item-title>
            </v-list-item>
            <v-list-item
              v-if="user.admin"
              :to="{ name: 'admin' }"
              prepend-icon="mdi-shield-account">
              <v-list-item-title>Admin Panel</v-list-item-title>
            </v-list-item>
            <v-divider />
            <v-list-item
              :to="{ name: 'logout' }"
              prepend-icon="mdi-logout">
              <v-list-item-title>Logout</v-list-item-title>
            </v-list-item>
          </v-list>
        </v-menu>
      </div>

      <!-- Login button for guests -->
      <v-btn
        v-else
        :to="{ name: 'login' }"
        color="primary"
        variant="flat"
        prepend-icon="mdi-login"
        class="text-none app-bar__login">
        Login
      </v-btn>
    </v-container>
  </v-app-bar>
</template>

<script setup lang="ts">
import type { PropType } from 'vue';
import type { UserResponseType } from '@/types/base';

defineProps({
  user: {
    type: Object as PropType<UserResponseType>,
    required: false,
  },
});
</script>

<style lang="scss" scoped>
.app-bar {
  padding-left: 1rem;
  padding-right: 1rem;
}

.app-bar__inner {
  gap: 1rem;
}

.logo-link {
  &:hover {
    opacity: 0.8;
  }
}

.app-bar__login {
  margin-right: 0.5rem;
}
</style>
