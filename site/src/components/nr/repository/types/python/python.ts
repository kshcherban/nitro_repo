import { PythonIcon } from "vue3-simple-icons";
import type { FrontendRepositoryType } from "@/types/repository";
import PythonProjectHelper from "./PythonProjectHelper.vue";
import PythonRepositoryHelper from "./PythonRepositoryHelper.vue";

export interface PythonProxyRoute {
  url: string;
  name?: string;
}

export interface PythonProxyConfigType {
  routes: PythonProxyRoute[];
}

export type PythonConfigType =
  | {
      type: "Hosted";
    }
  | {
      type: "Proxy";
      config: PythonProxyConfigType;
    };

export function defaultProxy(): PythonProxyConfigType {
  return {
    routes: [
      {
        url: "https://pypi.org/simple",
        name: "PyPI",
      },
    ],
  };
}

export const PythonFrontendDefinition: FrontendRepositoryType = {
  name: "python",
  properName: "Python",
  projectComponent: {
    component: PythonProjectHelper,
  },
  fullProjectComponent: {
    component: PythonRepositoryHelper,
  },
  icons: [
    {
      name: "Python",
      component: PythonIcon,
      url: "https://www.python.org/",
      props: {},
    },
  ],
};
