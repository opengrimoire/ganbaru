/**
 * The state of the welcome scenes' drawn event panel at one moment. A scene computes it from its own beats, so the
 * panel is a pure function of the scene's time, the same in the Calendar film and in the Music scene.
 */

import type { PomodoroPresetKey } from "$lib/pomodoro/rhythm";

/** The phases an event's music names a playlist for, in the event panel's order. */
export type WelcomeEventPanelMusicPhase = "focus" | "short-break" | "long-break";

/** One phase row of the panel's Music section: the playlist it takes and how far it has taken it. */
export interface WelcomeEventPanelMusicRow {
  phase: WelcomeEventPanelMusicPhase;
  /** The playlist's icon name and its localized name. */
  playlistIcon: string;
  playlistName: string;
  /** How far the row has gone from no playlist to this one, from 0 to 1. */
  fill: number;
  /** The touch on the row's playlist trigger, from 0 to 1 as it shows. */
  touch: number;
}

/** One playlist the playlist picker lists, with its icon, localized name, and track count. */
export interface WelcomeEventPanelMusicOption {
  key: string;
  icon: string;
  name: string;
  count: number;
}

/** A phase's playlist picker, open under or over that row's trigger. */
export interface WelcomeEventPanelMusicPicker {
  phase: WelcomeEventPanelMusicPhase;
  options: readonly WelcomeEventPanelMusicOption[];
  /** The option the row takes, checked in the list, or null for none. */
  selected: string | null;
  /** The touch on the selected option, from 0 to 1 as it shows. */
  touch: number;
}

/** The panel's Music section, open below its header. */
export interface WelcomeEventPanelMusic {
  /** Whether the event plays music, so the section's header is on and sums up its phases. */
  enabled: boolean;
  /** How far the section's body has opened, from 0 to 1. */
  expanded: number;
  rows: readonly WelcomeEventPanelMusicRow[];
  /** The open playlist picker, or null while every picker is closed. */
  picker: WelcomeEventPanelMusicPicker | null;
}

/** The touches on the panel's controls, each from 0 to 1 as it shows. */
export interface WelcomeEventPanelTouches {
  project: number;
  group: number;
  projectPick: number;
  /** The finger on the phone sheet's grabber as it drags the sheet to its full height. */
  sheetDrag: number;
  music: number;
  save: number;
}

/** Everything the drawn event panel shows at one moment. */
export interface WelcomeEventPanelFrame {
  /** How far the phone's sheet has risen to its peek, from 0 to 1. */
  sheetRise: number;
  /** How far the phone's sheet has gone on from its peek to its full height, from 0 to 1. */
  sheetExpand: number;
  /** The project picker's open pane, or null while it is closed. */
  pane: "groups" | "projects" | null;
  /** Whether the event has its project's color and Focus rhythm. */
  picked: boolean;
  /** The typed title, and whether the title has focus and its caret shows. */
  typed: string;
  typing: boolean;
  caretOn: boolean;
  touches: WelcomeEventPanelTouches;
  preset: PomodoroPresetKey;
  /** The Music section's state; null keeps it collapsed and off, as on an event without a project's music. */
  music: WelcomeEventPanelMusic | null;
  /** How far the panel's body has scrolled to its end when it cannot show everything, from 0 to 1. */
  scroll: number;
}

/** One row of the drawn project picker: a group or a project, with its icon name and localized label. */
export interface WelcomeEventPanelPickerRow {
  id: string;
  icon: string;
  label: string;
}

/** The drawn project picker's groups, the picked group's projects, and the ids of the group and project picked. */
export interface WelcomeEventPanelPicker {
  groups: readonly WelcomeEventPanelPickerRow[];
  projects: readonly WelcomeEventPanelPickerRow[];
  group: string;
  project: string;
}
