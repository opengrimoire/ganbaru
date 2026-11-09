<script lang="ts">
  import { getLocalization } from "$lib/i18n/translator.svelte";
  import { BUILD_PLATFORM_PROFILE } from "$lib/platform";
  import { CHAT_END_HOLD_MS, CHAT_SCENE_MODEL_NAME, chatSceneEnd, chatSceneStill } from "$lib/vault/welcome-chat-scene";
  import WelcomeChatScene, {
    CHAT_SCENE_MAX_HEIGHT,
    CHAT_SCENE_MAX_WIDTH,
    CHAT_SCENE_MIN_HEIGHT,
    CHAT_SCENE_MIN_WIDTH,
  } from "./WelcomeChatScene.svelte";
  import WelcomeScene from "./WelcomeScene.svelte";
  import WelcomeSceneStep from "./WelcomeSceneStep.svelte";

  /** The welcome screen on Chat: a prompt sent to Claude at a raised effort, which reads the project and replies, on a loop. */
  const { t } = getLocalization();
  const mobileShell = BUILD_PLATFORM_PROFILE.shell === "mobile";
  const modelName = `Claude ${CHAT_SCENE_MODEL_NAME}`;
</script>

<WelcomeSceneStep headline={t("vaultSetup.chat.headline")} body={t("vaultSetup.chat.body")}>
  <WelcomeScene
    class="flex-1"
    minWidth={CHAT_SCENE_MIN_WIDTH}
    maxWidth={CHAT_SCENE_MAX_WIDTH}
    minHeight={CHAT_SCENE_MIN_HEIGHT}
    maxHeight={CHAT_SCENE_MAX_HEIGHT}
    duration={chatSceneEnd(mobileShell)}
    hold={CHAT_END_HOLD_MS}
    still={chatSceneStill(mobileShell)}
    description={t(mobileShell ? "vaultSetup.chat.sceneDescriptionTouch" : "vaultSetup.chat.sceneDescription", modelName)}
  >
    {#snippet frame(elapsed, width, height)}<WelcomeChatScene {elapsed} {width} {height} />{/snippet}
  </WelcomeScene>
</WelcomeSceneStep>
