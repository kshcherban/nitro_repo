import { DockerIcon } from "vue3-simple-icons";
import type { FrontendRepositoryType } from "@/types/repository";

export const DockerFrontendDefinition: FrontendRepositoryType = {
  name: "docker",
  properName: "Docker",
  icons: [
    {
      name: "Docker",
      component: DockerIcon,
      url: "https://www.docker.com/",
      props: {
        color: "#2496ED",
        size: "28",
      },
    },
  ],
};
