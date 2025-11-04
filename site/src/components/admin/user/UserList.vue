<template>
  <div id="usersBox">
    <div id="headerBar">
      <h2>Users</h2>
      <input
        type="text"
        id="nameSearch"
        v-model="searchValue"
        autofocus
        placeholder="Search by Name, Username, or Primary Email Address" />
    </div>
    <div
      id="users"
      class="betterScroll">
      <div
        class="row"
        id="header">
        <div
          :class="['col', { sorted: sortBy === 'id' }]"
          @click="sortBy = 'id'"
          title="Sort by ID">
          ID #
        </div>
        <div
          :class="['col', { sorted: sortBy === 'name' }]"
          @click="sortBy = 'name'"
          title="Sort by Name">
          Name
        </div>
        <div
          :class="['col', { sorted: sortBy === 'username' }]"
          @click="sortBy = 'username'"
          title="Sort by Username">
          Username
        </div>
        <div class="col statusCol">
          Status
        </div>
      </div>
      <div
        class="row item"
        v-for="account in filteredTable"
        :key="account.id"
        @click="router.push(`/admin/user/${account.id}`)">
        <div class="col">{{ account.id }}</div>
        <div
          class="col"
          :title="account.name">
          {{ account.name }}
        </div>
        <div
          class="col"
          :title="account.username">
          {{ account.username }}
        </div>
        <div class="col">
          <span
            class="statusPill"
            :data-active="account.active">
            {{ account.active ? "Active" : "Inactive" }}
          </span>
        </div>
      </div>
    </div>
  </div>
</template>

<script setup lang="ts">
import router from "@/router";
import type { UserResponseType } from "@/types/base";
import { computed, ref, type PropType } from "vue";
const searchValue = ref<string>("");

const props = defineProps({
  users: Array as PropType<UserResponseType[]>,
});
const sortBy = ref<string>("id");

function sortList(a: UserResponseType, b: UserResponseType) {
  switch (sortBy.value) {
    case "id":
      return a.id - b.id;
    case "name":
      return a.name.localeCompare(b.name);
    case "username":
      return a.username.localeCompare(b.username);

    default:
      return 0;
  }
}
const filteredTable = computed(() => {
  if (!props.users) {
    return [];
  }
  const term = searchValue.value.trim().toLowerCase();
  const users = props.users.filter((user) => {
    if (!term) {
      return true;
    }
    return (
      user.name.toLowerCase().includes(term) ||
      user.username.toLowerCase().includes(term) ||
      user.email.toLowerCase().includes(term)
    );
  });
  return users.slice().sort(sortList);
});
</script>
<style scoped lang="scss">
@import "@/assets/styles/theme";
#headerBar {
  display: flex;
  justify-content: space-between;
  padding: 1rem;
  background-color: $primary-30;
  input {
    width: 25%;
  }
}
@media screen and (max-width: 1200px) {
  #headerBar {
    input {
      width: 50%;
    }
  }
}
@media screen and (max-width: 800px) {
  #headerBar {
    display: flex;
    flex-direction: column;
    input {
      width: 100%;
    }
  }
}
#users {
  background-color: $primary-50;
}

#header {
  .col {
    font-weight: bold;
    &:hover {
      cursor: pointer;
      color: $accent;
      transition: all 0.3s ease;
    }
  }
}
.row {
  display: grid;
  grid-template-columns: 0.6fr 1.2fr 1fr 1fr;
  grid-template-rows: auto;
  .col {
    padding: 1rem;
    border-bottom: 1px solid $primary-30;
  }
}
.item {
  cursor: pointer;
  &:hover {
    background-color: $primary-30;
    transition: all 0.3s ease;
    .col {
      color: $accent;
      transition: all 0.3s ease;
    }
  }
}
.statusPill {
  display: inline-block;
  padding: 0.25rem 0.6rem;
  border-radius: 999px;
  font-size: 0.85rem;
  font-weight: 600;
  background-color: $primary-30;
  color: $text;
  &[data-active="false"] {
    background-color: $secondary-70;
    color: $text;
  }
}
</style>
