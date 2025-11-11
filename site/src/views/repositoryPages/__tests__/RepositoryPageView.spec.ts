import { flushPromises, mount } from "@vue/test-utils";
import { describe, expect, it, vi } from "vitest";
import { defineComponent, ref } from "vue";
import RepositoryPageView from "@/views/repositoryPages/RepositoryPageView.vue";

vi.mock("@/router", () => {
  const push = vi.fn();
  const currentRoute = ref({
    params: {
      repositoryId: "repo-1",
    },
  });
  return {
    default: {
      currentRoute,
      push,
    },
  };
});

const mockRepository = {
  id: "repo-1",
  name: "example",
  storage_name: "primary",
  storage_id: "storage-1",
  repository_type: "npm",
  repository_kind: null,
  active: true,
  visibility: "Public",
  updated_at: "2025-11-09T00:00:00Z",
  created_at: "2025-01-01T00:00:00Z",
  auth_enabled: false,
  storage_usage_bytes: null,
  storage_usage_updated_at: null,
};

vi.mock("@/stores/repositories", () => ({
  useRepositoryStore: () => ({
    getRepositoryById: vi.fn().mockResolvedValue(mockRepository),
    getRepositoryIdByNames: vi.fn(),
  }),
}));

vi.mock("@/http", () => ({
  default: {
    get: vi.fn().mockImplementation((url: string) => {
      if (url === `/api/repository/${mockRepository.id}`) {
        return Promise.resolve({ data: mockRepository });
      }
      if (url === `/api/repository/${mockRepository.id}/configs`) {
        return Promise.resolve({ data: [] });
      }
      if (url === `/api/repository/page/${mockRepository.id}`) {
        return Promise.reject({
          response: {
            status: 404,
            data: "does not support config key page",
          },
        });
      }
      return Promise.resolve({ data: {} });
    }),
  },
}));

const simpleStub = defineComponent({
  template: "<div><slot /></div>",
});

describe("RepositoryPageView.vue", () => {
  it("does not show an info alert when no custom page is defined", async () => {
    const wrapper = mount(RepositoryPageView, {
      global: {
        stubs: {
          "v-container": simpleStub,
          "v-card": simpleStub,
          "v-card-text": simpleStub,
          "v-row": simpleStub,
          "v-col": simpleStub,
          "v-btn": defineComponent({ template: "<button><slot /></button>" }),
          "v-alert": defineComponent({ template: "<div class='alert'><slot /></div>" }),
          CopyURL: simpleStub,
          RepositoryHelper: simpleStub,
          RepositoryIcon: simpleStub,
          RepositoryPageViewer: simpleStub,
        },
      },
    });

    await flushPromises();

    expect(wrapper.text()).not.toContain("This repository does not define a custom page yet.");
  });
});
