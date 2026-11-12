import { mount, unmount } from "svelte";
import ChatComposer from "$lib/components/chat/composer/ChatComposer.svelte";

export * from "$lib/components/chat/composer/composer-test-store";

export interface ChatComposerHarness {
  mounted: { target: HTMLDivElement; component: ReturnType<typeof mount> }[];
  setup: (hero?: boolean) => { target: HTMLDivElement; editor: HTMLDivElement };
  cleanup: () => Promise<void>;
}

export function createChatComposerHarness(): ChatComposerHarness {
  const mounted: ChatComposerHarness["mounted"] = [];
  return {
    mounted,
    setup(hero = false) {
      const target = document.createElement("div");
      document.body.append(target);
      const component = mount(ChatComposer, { target, props: { hero } });
      mounted.push({ target, component });
      const editor = target.querySelector("div[data-chat-composer]");
      if (!(editor instanceof HTMLDivElement)) throw new Error("Chat composer did not render");
      return { target, editor };
    },
    async cleanup() {
      while (mounted.length > 0) {
        const entry = mounted.pop();
        if (!entry) continue;
        await unmount(entry.component);
        entry.target.remove();
      }
    },
  };
}
