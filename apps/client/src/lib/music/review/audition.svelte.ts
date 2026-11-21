import type { LocalRootBinding, MusicInspectorDetail } from "$lib/music/library/contracts";
import { getMusicPlayer } from "$lib/stores/music-player.svelte";
import { musicErrorMessage, type MusicErrorKey } from "$lib/music/user-errors";
import { translate } from "$lib/i18n/translator.svelte";
import {
  IDLE_MUSIC_AUDITION,
  transitionMusicAudition,
  type MusicAuditionEvent,
  type MusicAuditionState,
} from "./audition-state";

export const MUSIC_CONTEXT_BOUNDARY_EVENT = "ganbaru-music-context-boundary";

/** A preview that failed, kept with the item it was for so another item never shows it. */
interface MusicAuditionFailure {
  itemId: string;
  message: string;
}

export class MusicReviewAuditionController {
  private failure = $state.raw<MusicAuditionFailure | null>(null);
  private state = $state.raw<MusicAuditionState>(IDLE_MUSIC_AUDITION);
  private reviewPositions: Record<string, number> = {};
  private readonly player = getMusicPlayer();
  private restoreTask: Promise<void> | null = null;
  private operationGeneration = 0;

  get musicPlayer(): ReturnType<typeof getMusicPlayer> {
    return this.player;
  }

  /** Whether Review has suspended, or is suspending, the playback it interrupts. */
  get active(): boolean {
    return this.state.phase !== "idle";
  }

  /** The item Review last loaded, or `null` before one loads. */
  get reviewItemId(): string | null {
    return this.state.phase === "auditioning" ? this.state.itemId : null;
  }

  get ownsPlayback(): boolean {
    return this.active && this.player.contextOwner === "review";
  }

  /** The message of the last preview of `itemId` that failed, or `null` when it did not fail. */
  errorFor(itemId: string | null): string | null {
    return itemId !== null && this.failure?.itemId === itemId ? this.failure.message : null;
  }

  dismissError(): void {
    this.failure = null;
  }

  async preview(
    detail: MusicInspectorDetail,
    _bindings: readonly LocalRootBinding[],
    autoplay: boolean,
  ): Promise<boolean> {
    if (this.restoreTask) await this.restoreTask;
    if (this.ownsPlayback && this.reviewItemId === detail.item.id) return true;
    const generation = ++this.operationGeneration;
    this.failure = null;
    if (this.state.phase !== "auditioning") {
      this.dispatch({ type: "suspendStarted" });
      try {
        const checkpoint = await this.player.suspendForReview();
        if (generation !== this.operationGeneration) {
          await this.player.restoreAfterReview(checkpoint);
          return false;
        }
        this.dispatch({ type: "suspended", checkpoint });
      } catch (error) {
        if (generation === this.operationGeneration) {
          this.clearOwnership();
          this.fail(detail.item.id, error, "music.errors.playerFailed");
        }
        return false;
      }
    }
    if (generation !== this.operationGeneration) return false;
    try {
      await this.player.loadReviewItem(detail.item.id, autoplay);
      if (generation !== this.operationGeneration) return false;
      this.player.contextOwner = "review";
      this.dispatch({ type: "itemLoaded", itemId: detail.item.id });
      const retainedPosition = this.reviewPositions[detail.item.id] ?? 0;
      if (retainedPosition > 0) await this.player.seekToMs(retainedPosition);
      return true;
    } catch (error) {
      if (generation === this.operationGeneration) this.fail(detail.item.id, error, "music.errors.playbackFailed");
      return false;
    }
  }

  async restore(): Promise<void> {
    if (this.restoreTask) return this.restoreTask;
    const checkpoint = this.state.phase === "auditioning" ? this.state.checkpoint : null;
    this.operationGeneration += 1;
    this.clearOwnership();
    if (!checkpoint) return;
    const task = this.player.restoreAfterReview(checkpoint)
      .catch((error: unknown) => {
        // Review has closed by now, so the player reports it where playback is shown.
        this.player.playerError = musicErrorMessage(error, translate("music.errors.playerFailed"));
      })
      .finally(() => {
        if (this.restoreTask === task) this.restoreTask = null;
      });
    this.restoreTask = task;
    await task;
  }

  discard(): void {
    this.operationGeneration += 1;
    this.clearOwnership();
  }

  supersedeForBoundary(owner: "calendar-event" | "pomodoro"): void {
    if (this.ownsPlayback) {
      if (this.reviewItemId) this.reviewPositions[this.reviewItemId] = this.player.snapshot.positionMs;
      this.player.contextOwner = owner;
    }
    this.discard();
  }

  private dispatch(event: MusicAuditionEvent): void {
    this.state = transitionMusicAudition(this.state, event);
  }

  private fail(itemId: string, error: unknown, fallbackKey: MusicErrorKey): void {
    this.failure = { itemId, message: musicErrorMessage(error, translate(fallbackKey)) };
  }

  private clearOwnership(): void {
    this.dispatch({ type: "ended" });
    this.failure = null;
  }
}

export function createMusicReviewAuditionController(): MusicReviewAuditionController {
  return new MusicReviewAuditionController();
}
