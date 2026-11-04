import { Channel } from "@tauri-apps/api/core";
import {
  closeNativeYouTubeHost,
  openNativeYouTubeHost,
  postNativeYouTubeHostMessage,
  setNativeYouTubeHostBounds,
  type NativeYouTubeHostBounds,
  type NativeYouTubeHostInstance,
} from "$lib/api/music";

/**
 * Android hosts the YouTube player page in a separate native WebView with no app bridge, so the app WebView can
 * deny every frame source.
 */
export const usesNativeYouTubeHost = typeof __GANBARU_BUILD_PLATFORM__ !== "undefined"
  && __GANBARU_BUILD_PLATFORM__ === "android";

/** Placement that keeps a native host attached but invisible. */
export const HIDDEN_NATIVE_YOUTUBE_BOUNDS: NativeYouTubeHostBounds = {
  x: 0,
  y: 0,
  width: 1,
  height: 1,
  scale: 1,
  visible: false,
};

export interface NativeYouTubeHost {
  open(url: string, bounds: NativeYouTubeHostBounds): Promise<void>;
  setBounds(bounds: NativeYouTubeHostBounds): Promise<void>;
  post(message: Record<string, unknown>): void;
  close(): Promise<void>;
}

/**
 * Reads one native host notice. Kotlin forwards the page's string message unchanged, so only a JSON object from the
 * expected instance is returned.
 */
export function parseNativeYouTubeHostNotice(
  value: unknown,
  instance: NativeYouTubeHostInstance,
): Record<string, unknown> | null {
  if (!value || typeof value !== "object") return null;
  const notice = value as { instance?: unknown; data?: unknown };
  if (notice.instance !== instance || typeof notice.data !== "string") return null;
  try {
    const data: unknown = JSON.parse(notice.data);
    return data && typeof data === "object" && !Array.isArray(data) ? data as Record<string, unknown> : null;
  } catch {
    return null;
  }
}

interface NativeYouTubeInstanceState {
  queue: Promise<void>;
  session: number;
}

const instanceStates = new Map<NativeYouTubeHostInstance, NativeYouTubeInstanceState>();

function instanceState(instance: NativeYouTubeHostInstance): NativeYouTubeInstanceState {
  let state = instanceStates.get(instance);
  if (!state) {
    state = { queue: Promise.resolve(), session: 0 };
    instanceStates.set(instance, state);
  }
  return state;
}

/**
 * Creates a controller for one native host instance. Operations on an instance run in call order across controllers
 * because the native side handles each command independently. Each open starts a new session with a fresh channel,
 * so a replaced page cannot deliver messages and a controller whose page was replaced cannot move or close it.
 */
export function createNativeYouTubeHost(
  instance: NativeYouTubeHostInstance,
  onData: (data: Record<string, unknown>) => void,
): NativeYouTubeHost {
  const shared = instanceState(instance);
  let ownSession: number | null = null;

  function enqueue(operation: () => Promise<void>): Promise<void> {
    const next = shared.queue.then(operation);
    shared.queue = next.catch(() => undefined);
    return next;
  }

  function owns(): boolean {
    return ownSession !== null && ownSession === shared.session;
  }

  return {
    open(url, bounds) {
      const session = ++shared.session;
      ownSession = session;
      const channel = new Channel<unknown>((value) => {
        if (session !== shared.session) return;
        const data = parseNativeYouTubeHostNotice(value, instance);
        if (data) onData(data);
      });
      return enqueue(() => openNativeYouTubeHost(instance, url, bounds, channel));
    },
    setBounds(bounds) {
      if (!owns()) return Promise.resolve();
      return enqueue(() => setNativeYouTubeHostBounds(instance, bounds));
    },
    post(message) {
      if (!owns()) return;
      void enqueue(() => postNativeYouTubeHostMessage(instance, message)).catch((error: unknown) => {
        console.warn("Unable to message the YouTube host.", error);
      });
    },
    close() {
      if (!owns()) return Promise.resolve();
      shared.session += 1;
      ownSession = null;
      return enqueue(() => closeNativeYouTubeHost(instance));
    },
  };
}
