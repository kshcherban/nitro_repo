import { fileURLToPath, URL } from "node:url";

import { defineConfig, type PluginOption, type UserConfig } from "vite";
import vue from "@vitejs/plugin-vue";
import vueJsx from "@vitejs/plugin-vue-jsx";
import fs from "fs";
import browserslistToEsbuild from "browserslist-to-esbuild";
import { ViteEjsPlugin } from "vite-plugin-ejs";

const enableDevTools =
  process.env.NODE_ENV !== "production" &&
  process.env.VITE_DEVTOOLS !== "false";

export default defineConfig(async (_env): Promise<UserConfig> => {
  const plugins: PluginOption[] = [
    vue(),
    vueJsx(),
    ViteEjsPlugin(),
  ];

  if (enableDevTools) {
    const { default: vueDevTools } = await import("vite-plugin-vue-devtools");
    plugins.push(vueDevTools());
  }

  plugins.push({
      name: "copy-routes",
      apply: "build",

      closeBundle() {
        console.log("Copying routes.json to dist/assets");
        fs.copyFile(
          fileURLToPath(new URL("./src/router/routes.json", import.meta.url)),
          fileURLToPath(new URL("./dist/routes.json", import.meta.url)),
          (err) => {
            if (err) {
              console.error(err);
            } else {
              console.log("routes.json copied successfully");
            }
          },
        );
      },
    });

  return {
    build: {
      target: browserslistToEsbuild(undefined, {
        path: ".browserlistrc",
      }),
    },
    plugins,
    css: {
      preprocessorOptions: {
        scss: {
          api: "modern-compiler",
        },
      },
      devSourcemap: true,
    },
    resolve: {
      alias: {
        "@": fileURLToPath(new URL("./src", import.meta.url)),
      },
    },
  };
});
