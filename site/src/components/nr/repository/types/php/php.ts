import { PhpIcon } from "vue3-simple-icons";
import type { FrontendRepositoryType } from "@/types/repository";
import PhpProjectHelper from "./PhpProjectHelper.vue";
import PhpRepositoryHelper from "./PhpRepositoryHelper.vue";

export interface PhpConfigType {
  type: "Hosted";
}

export const PhpFrontendDefinition: FrontendRepositoryType = {
  name: "php",
  properName: "PHP Composer",
  projectComponent: {
    component: PhpProjectHelper,
  },
  fullProjectComponent: {
    component: PhpRepositoryHelper,
  },
  icons: [
    {
      name: "PHP",
      component: PhpIcon,
      url: "https://www.php.net/",
      props: {},
    },
  ],
};
