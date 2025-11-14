import { DebianIcon } from "vue3-simple-icons";
import type { FrontendRepositoryType } from "@/types/repository";

export interface DebRepositoryConfig {
  distributions: string[];
  components: string[];
  architectures: string[];
}

export function defaultDebConfig(): DebRepositoryConfig {
  return {
    distributions: ["stable"],
    components: ["main"],
    architectures: ["amd64", "all"],
  };
}

export const DebFrontendDefinition: FrontendRepositoryType = {
  name: "deb",
  properName: "Debian",
  icons: [
    {
      name: "Debian",
      component: DebianIcon,
      url: "https://www.debian.org/",
      props: {},
    },
  ],
};
