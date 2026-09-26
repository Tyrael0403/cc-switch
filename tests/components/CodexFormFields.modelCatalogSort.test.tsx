import { render, screen, waitFor } from "@testing-library/react";
import { act } from "react";
import { FormProvider, useForm } from "react-hook-form";
import { beforeEach, describe, expect, it, vi } from "vitest";
import { CodexFormFields } from "@/components/providers/forms/CodexFormFields";
import type { Announcements } from "@dnd-kit/core";
import type { CodexCatalogModel } from "@/types";

const dndState = vi.hoisted(() => ({
  ids: [] as string[],
  onDragEnd: null as ((event: unknown) => void) | null,
  announcements: null as import("@dnd-kit/core").Announcements | null,
}));

vi.mock("@dnd-kit/core", async (importOriginal) => {
  const actual = await importOriginal<typeof import("@dnd-kit/core")>();
  return {
    ...actual,
    DndContext: ({
      children,
      onDragEnd,
      accessibility,
    }: {
      children: React.ReactNode;
      onDragEnd: (event: unknown) => void;
      accessibility?: { announcements?: Announcements };
    }) => {
      dndState.onDragEnd = onDragEnd;
      dndState.announcements = accessibility?.announcements ?? null;
      return <div>{children}</div>;
    },
  };
});

vi.mock("@dnd-kit/sortable", async (importOriginal) => {
  const actual = await importOriginal<typeof import("@dnd-kit/sortable")>();
  return {
    ...actual,
    useSortable: ({ id }: { id: string }) => {
      dndState.ids.push(id);
      return {
        attributes: { "aria-roledescription": "sortable" },
        listeners: { onPointerDown: vi.fn() },
        setNodeRef: (node: HTMLElement | null) => node,
        transform: null,
        transition: null,
        isDragging: false,
      };
    },
  };
});

const catalogModels: CodexCatalogModel[] = [
  {
    model: "deepseek-v4-flash",
    displayName: "DeepSeek V4 Flash",
    inputModalities: ["text", "image"],
  },
  {
    model: "kimi-k2",
    displayName: "Kimi K2",
    supportsParallelToolCalls: false,
  },
];

function FormWrapper({ children }: { children: React.ReactNode }) {
  const form = useForm();
  return <FormProvider {...form}>{children}</FormProvider>;
}

function renderCodexFormFields(
  onCatalogModelsChange: (models: CodexCatalogModel[]) => void,
  models: CodexCatalogModel[] = catalogModels,
) {
  return render(
    <FormWrapper>
      <CodexFormFields
        codexApiKey="test-key"
        onApiKeyChange={vi.fn()}
        category="custom"
        shouldShowApiKeyLink={false}
        websiteUrl=""
        shouldShowSpeedTest={false}
        codexBaseUrl="https://api.example.com/v1"
        onBaseUrlChange={vi.fn()}
        isFullUrl={false}
        onFullUrlChange={vi.fn()}
        isEndpointModalOpen={false}
        onEndpointModalToggle={vi.fn()}
        autoSelect={false}
        onAutoSelectChange={vi.fn()}
        apiFormat="openai_responses"
        onApiFormatChange={vi.fn()}
        anthropicAuthField="ANTHROPIC_AUTH_TOKEN"
        onAnthropicAuthFieldChange={vi.fn()}
        impersonateClaudeCode={false}
        onImpersonateClaudeCodeChange={vi.fn()}
        maxOutputTokens=""
        onMaxOutputTokensChange={vi.fn()}
        promptCacheRouting="auto"
        onPromptCacheRoutingChange={vi.fn()}
        catalogModels={models}
        onCatalogModelsChange={onCatalogModelsChange}
        speedTestEndpoints={[]}
        customUserAgent=""
        onCustomUserAgentChange={vi.fn()}
        localProxyHeadersOverride=""
        onLocalProxyHeadersOverrideChange={vi.fn()}
        localProxyBodyOverride=""
        onLocalProxyBodyOverrideChange={vi.fn()}
      />
    </FormWrapper>,
  );
}

describe("CodexFormFields model catalog sorting", () => {
  beforeEach(() => {
    dndState.ids = [];
    dndState.onDragEnd = null;
    dndState.announcements = null;
  });

  it("reorders model mappings from the drag handle and preserves row data", async () => {
    const onCatalogModelsChange = vi.fn();

    renderCodexFormFields(onCatalogModelsChange);

    expect(
      screen.getByRole("button", {
        name: "拖拽排序: DeepSeek V4 Flash (#1)",
      }),
    ).toBeInTheDocument();
    expect(
      screen.getByRole("button", { name: "拖拽排序: Kimi K2 (#2)" }),
    ).toBeInTheDocument();
    expect(dndState.onDragEnd).not.toBeNull();
    expect(dndState.ids).toHaveLength(2);
    expect(dndState.announcements).not.toBeNull();
    const dragStartEvent = {
      active: { id: dndState.ids[1] },
    } as Parameters<Announcements["onDragStart"]>[0];
    const dragOverEvent = {
      active: { id: dndState.ids[1] },
      over: { id: dndState.ids[0] },
    } as Parameters<NonNullable<Announcements["onDragOver"]>>[0];

    expect(dndState.announcements!.onDragStart!(dragStartEvent)).toBe(
      "拖拽排序: Kimi K2 (#2)",
    );
    expect(dndState.announcements!.onDragOver!(dragOverEvent)).toBe(
      "拖拽排序: Kimi K2 (#2) → 拖拽排序: DeepSeek V4 Flash (#1)",
    );
    expect(dndState.announcements!.onDragEnd!(dragOverEvent)).toBe(
      "拖拽排序: Kimi K2 (#2) → 拖拽排序: DeepSeek V4 Flash (#1)",
    );
    expect(dndState.announcements!.onDragCancel!(dragOverEvent)).toBe(
      "拖拽排序: Kimi K2 (#2)",
    );

    await act(async () => {
      dndState.onDragEnd!({
        active: { id: dndState.ids[1] },
        over: { id: dndState.ids[0] },
      });
    });

    await waitFor(() =>
      expect(onCatalogModelsChange).toHaveBeenCalledWith([
        {
          model: "kimi-k2",
          displayName: "Kimi K2",
          contextWindow: "",
          supportsParallelToolCalls: false,
        },
        {
          model: "deepseek-v4-flash",
          displayName: "DeepSeek V4 Flash",
          contextWindow: "",
          inputModalities: ["text", "image"],
        },
      ]),
    );
  });

  it("gives duplicate mapping rows distinct accessible names", () => {
    renderCodexFormFields(vi.fn(), [
      { model: "same-model", displayName: "Same model" },
      { model: "same-model", displayName: "Same model" },
    ]);

    expect(
      screen.getByRole("button", { name: "拖拽排序: Same model (#1)" }),
    ).toBeInTheDocument();
    expect(
      screen.getByRole("button", { name: "拖拽排序: Same model (#2)" }),
    ).toBeInTheDocument();
    expect(
      dndState.announcements!.onDragStart!({
        active: { id: dndState.ids[1] },
      } as Parameters<Announcements["onDragStart"]>[0]),
    ).toBe("拖拽排序: Same model (#2)");
  });
});
