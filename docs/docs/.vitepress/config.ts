import { defineConfig } from "vitepress";

export default defineConfig({
  lang: "en-US",
  title: "Nitro_Repo",
  description: "A Fast Artifact Manager",
  lastUpdated: true,
  themeConfig: {
    nav: [
      { text: "Home", link: "/", activeMatch: "^/$" },
      {
        text: "System Admin",
        link: "/sysAdmin/",
        activeMatch: "^/sysAdmin/",
      },
      {
        text: "Knowledge Base",
        link: "/knowledge/",
        activeMatch: "^/knowledge/",
      },
      {
        text: "Repository Types",
        link: "/repositoryTypes/",
        activeMatch: "^/repositoryTypes/",
      },
      {
        text: "SSO",
        link: "/sso/",
        activeMatch: "^/sso/",
      },
      {
        text: "Release Notes",
        link: "https://github.com/kshcherban/nitro_repo/releases",
      },
    ],
    socialLinks: [
      { icon: "github", link: "https://github.com/kshcherban/nitro_repo" },
    ],
    sidebar: {
      "/": generalInfo(),
      "/sysAdmin/": sysAdminBar(),
      "/knowledge/": knowledgeBaseBar(),
      "/repositoryTypes/": repositoryTypesBar(),
    },
  },
});

function generalInfo() {
  return [
    {
      text: "Nitro Repo",
      items: [
        { text: "What is Nitro Repo?", link: "/" },
        { text: "Features", link: "/features" },
        { text: "Contributing", link: "/contributing" },
        { text: "Single Sign-On", link: "/sso/" },
      ],
    },
  ];
}

function knowledgeBaseBar() {
  return [
    {
      text: "Other",
      items: [
        { text: "Architecture", link: "/knowledge/Architecture" },
        { text: "Internal Workings", link: "/knowledge/InternalWorkings" },
      ],
    },
  ];
}

function sysAdminBar() {
  return [
    {
      text: "Operating",
      items: [
        { text: "Prepping your System", link: "/sysAdmin/" },
        { text: "Maintenance", link: "/sysAdmin/maintenance" },
        { text: "Configuring S3 Storage", link: "/sysAdmin/s3" },
      ],
    },
  ];
}

function repositoryTypesBar() {
  return [
    {
      text: "Docker",
      link: "/repositoryTypes/docker",
      items: [
        {
          text: "Quick Reference",
          link: "/repositoryTypes/docker/reference",
        },
        {
          text: "HTTP Routes",
          link: "/repositoryTypes/docker/routes",
        },
        {
          text: "Standard",
          link: "/repositoryTypes/docker/standard",
        },
        {
          text: "Configs",
          link: "/repositoryTypes/docker/configs",
        },
      ],
    },
    {
      text: "Go",
      link: "/repositoryTypes/go",
      items: [
        {
          text: "Hosted Repositories",
          link: "/repositoryTypes/go/hosted",
        },
        {
          text: "Proxy Setup",
          link: "/repositoryTypes/go/proxy",
        },
        {
          text: "HTTP Routes",
          link: "/repositoryTypes/go/routes",
        },
      ],
    },
    {
      text: "Helm",
      link: "/repositoryTypes/helm",
      items: [
        {
          text: "HTTP Routes",
          link: "/repositoryTypes/helm/routes",
        },
      ],
    },
    {
      text: "Maven",
      link: "/repositoryTypes/maven",
      items: [
        {
          text: "Quick Reference",
          link: "/repositoryTypes/maven/reference",
        },
        {
          text: "HTTP Routes",
          link: "/repositoryTypes/maven/routes",
        },
        {
          text: "Maven Standard",
          link: "/repositoryTypes/maven/standard",
        },
        {
          text: "Nitro Deploy",
          link: "/repositoryTypes/maven/nitroDeploy",
        },
        {
          text: "Configs",
          link: "/repositoryTypes/maven/configs",
        },
      ],
    },
    {
      text: "NPM",
      link: "/repositoryTypes/npm",
      items: [
        {
          text: "Quick Reference",
          link: "/repositoryTypes/npm/reference",
        },
        {
          text: "HTTP Routes",
          link: "/repositoryTypes/npm/routes",
        },
        {
          text: "NPM Standard",
          link: "/repositoryTypes/npm/standard",
        },
        {
          text: "Configs",
          link: "/repositoryTypes/npm/configs",
        },
        {
          text: "Common Issues",
          link: "/repositoryTypes/npm/errors",
        },
      ],
    },
    {
      text: "PHP",
      link: "/repositoryTypes/php",
      items: [
        {
          text: "Quick Reference",
          link: "/repositoryTypes/php/reference",
        },
        {
          text: "HTTP Routes",
          link: "/repositoryTypes/php/routes",
        },
      ],
    },
    {
      text: "Python",
      link: "/repositoryTypes/python",
      items: [
        {
          text: "Quick Reference",
          link: "/repositoryTypes/python/reference",
        },
        {
          text: "HTTP Routes",
          link: "/repositoryTypes/python/routes",
        },
      ],
    },
    {
      text: "Cargo",
      link: "/repositoryTypes/cargo",
      items: [
        {
          text: "Overview",
          link: "/repositoryTypes/cargo/",
        },
      ],
    },
    {
      text: "Debian",
      link: "/repositoryTypes/deb/",
      items: [
        {
          text: "Overview",
          link: "/repositoryTypes/deb/",
        },
      ],
    },
  ];
}
