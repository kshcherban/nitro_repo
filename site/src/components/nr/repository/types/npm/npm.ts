import { NpmIcon } from "vue3-simple-icons";
import type { FrontendRepositoryType } from "@/types/repository";
import NPMProjectHelper from "./NPMProjectHelper.vue";

export interface NpmProxyRoute {
  url: string;
  name?: string;
}

export interface NpmProxyConfigType {
  routes: NpmProxyRoute[];
}

export type NPMConfigType =
  | {
      type: "Hosted";
    }
  | {
      type: "Proxy";
      config: NpmProxyConfigType;
    };

export function defaultProxy(): NpmProxyConfigType {
  return {
    routes: [
      {
        url: "https://registry.npmjs.org",
        name: "npmjs",
      },
    ],
  };
}

export const NpmFrontendDefinition: FrontendRepositoryType = {
  name: "npm",
  properName: "NPM",
  projectComponent: {
    component: NPMProjectHelper,
    props: {},
  },
  icons: [
    {
      name: "NPM",
      component: NpmIcon,
      url: "https://www.npmjs.com/",
      props: {},
    },
  ],
};
