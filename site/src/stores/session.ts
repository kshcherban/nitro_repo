import { defineStore } from "pinia";
import { type Ref, ref } from "vue";
import type { Me, UserResponseType, Session } from "@/types/base";
import http from "@/http";
export const sessionStore = defineStore(
  "sessionStore",
  () => {
    const session: Ref<Session | undefined> = ref(undefined);
    const user: Ref<UserResponseType | undefined> = ref(undefined);
    function login(me: Me) {
      user.value = me.user;
      session.value = me.session;
    }
    function isAdmin(): boolean {
      if (user.value === undefined) {
        return false;
      }
      return user.value.admin;
    }

    async function logout() {
      await http
        .post("/api/user/logout")
        .then(() => {})
        .catch(() => {});
      session.value = undefined;
      user.value = undefined;
      console.log(`User ${user.value} logged out successfully`);
    }
    async function updateUser(): Promise<UserResponseType | undefined> {
      return await http
        .get<Me>("/api/user/me")
        .then((response) => {
          const fetchedSession = response.data.session;
          session.value = {
            ...fetchedSession,
            expires: new Date(fetchedSession.expires),
            created: new Date(fetchedSession.created),
          };
          user.value = response.data.user;
          return response.data.user;
        })
        .catch(() => {
          user.value = undefined;
          session.value = undefined;
          return undefined;
        });
    }

    return { user, session, login, logout, updateUser, isAdmin };
  },
  {
    persist: true,
  },
);
