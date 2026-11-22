<script lang="ts">
  import { onMount, untrack } from "svelte";
  import Loading from "$lib/components/ui/Loading.svelte";
  import { getLocalization } from "$lib/i18n/translator.svelte";
  import { PairingCameraScanner, type QrFrameDecoder } from "$lib/vault/pairing-camera";

  /**
   * Rear-camera scanner; the decoder decides which QR payload family it accepts (pairing by default). While
   * `disabled` is set it pauses between frames and resumes once it clears, keeping the camera running.
   */
  let {
    disabled = false,
    decode,
    label,
    onInvitation,
    onError,
  }: {
    disabled?: boolean;
    decode?: QrFrameDecoder;
    /** Accessible name of the preview; the pairing label is the default. */
    label?: string;
    onInvitation: (invitation: string) => void | Promise<void>;
    onError: (cause: unknown) => void;
  } = $props();

  const { t } = getLocalization();
  let video: HTMLVideoElement;
  let scanner: PairingCameraScanner | null = null;
  let ready = $state(false);
  let stopped = false;
  /** Whether a scan was due while `disabled` was set, so clearing it schedules the next one. */
  let paused = false;
  let timer: ReturnType<typeof setTimeout> | null = null;

  /** Schedules the next frame scan, or pauses until `disabled` clears. */
  function scheduleScan(): void {
    if (stopped) return;
    if (disabled) {
      paused = true;
      return;
    }
    timer = setTimeout(() => {
      timer = null;
      void scan();
    }, 180);
  }

  /** Scans one frame and hands a decoded payload to `onInvitation`, or schedules the next frame. */
  async function scan(): Promise<void> {
    if (!scanner || stopped) return;
    if (disabled) {
      paused = true;
      return;
    }
    try {
      const invitation = await scanner.scanFrame();
      stopped = true;
      scanner.stop();
      await onInvitation(invitation);
    } catch {
      scheduleScan();
    }
  }

  /** Stops scanning and releases the camera. */
  function stop(): void {
    stopped = true;
    if (timer) clearTimeout(timer);
    scanner?.stop();
  }

  $effect(() => {
    if (disabled) return;
    untrack(() => {
      if (!paused) return;
      paused = false;
      scheduleScan();
    });
  });

  onMount(() => {
    scanner = new PairingCameraScanner(video, decode);
    void scanner.start().then(() => {
      ready = true;
      scheduleScan();
    }).catch(onError);
    return stop;
  });
</script>

<div class="grid w-full">
  <div class="relative mx-auto aspect-square w-full overflow-hidden rounded-md bg-black">
    <video bind:this={video} muted class="h-full w-full object-cover" aria-label={label ?? t("vaultHandoff.scanQr")}></video>
    {#if !ready}
      <Loading layout="fill" showLabel label={t("vaultHandoff.cameraStarting")} class="absolute inset-0 text-sm text-white" />
    {/if}
    <div class="pointer-events-none absolute inset-[12%] rounded-lg border-2 border-white/80"></div>
  </div>
</div>
