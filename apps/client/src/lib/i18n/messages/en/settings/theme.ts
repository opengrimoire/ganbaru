export const theme = {
  themesHeading: "Themes",
  quickToggle: "Quick toggle",
  pickerTitle: "Theme",
  pickerLabel: "Theme picker",
  closePicker: "Close theme picker",
  lightQuickToggle: "Light mode quick toggle theme",
  darkQuickToggle: "Dark mode quick toggle theme",
  allThemes: "All themes",
  pasteJson: "Paste theme JSON",
  closeImport: "Close import",
  pasteClipboard: "Paste from clipboard",
  openFile: "Open file",
  import: "Import",
  importTheme: "Import theme",
  imported: "Theme imported",
  exporting: "Exporting theme…",
  exported: "Theme exported",
  exportedToDownloads: (name: string) => `Theme exported to Downloads as ${name}`,
  exportFailed: "Could not export theme",
  clipboardFailed: "Could not read from clipboard. Paste manually below.",
  fileReadFailed: "Could not read the selected file.",
  pasteFirst: "Paste a theme JSON object first.",
  deleteTitle: (name: string) => `Delete ${name}?`,
  thisTheme: "this theme",
  cannotBeUndone: "This cannot be undone",
  duplicateAndEdit: "Duplicate and edit",
  duplicateAndEditTheme: "Duplicate and edit theme",
  viewTheme: "View theme",
  editTheme: "Edit theme",
  exportJson: "Export theme JSON",
  exportingJson: "Exporting theme JSON",
  dismissTransferNotification: "Dismiss theme transfer notification",
  deleteTheme: "Delete theme",
  activeTheme: (name: string) => `${name} is active`,
  applyTheme: (name: string) => `Apply ${name}`,
  jsonSchema: "Schema",
  jsonReadonly: "Read-only representation of the theme",
  jsonEditable: "Edit directly, apply to commit changes",
  copyJson: "Copy JSON",
  saveToFile: "Save to file",
  discardEdits: "Discard edits",
  applyChanges: "Apply changes",
  rebakeMessage: (savedVersion: number, currentVersion: number) =>
    `This theme was saved against an older derivation engine (v${savedVersion}, current v${currentVersion}). Rebake to refresh non-pinned colors.`,
  maybeLater: "Maybe later",
  rebake: "Rebake",
  editor: {
    dialogLabel: "Theme editor",
    expandEditor: "Expand editor",
    collapseEditor: "Collapse editor",
    resetAllToSeed:
      "Reset every source, override, palette slot, and icon tag to its clone-time value",
    restoreSeedTitle: "Restore every value to the clone-time snapshot",
    noSeedChangesTitle: "Nothing has changed since this theme was cloned",
    resetAllToSeedButton: "Reset all to seed",
    applyAndReturn: "Apply and return",
    saveAndApply: "Save and apply",
    backToThemes: "Back to themes",
    editingTheme: "Custom theme",
    discardTitle: "Discard theme changes?",
    discardMessage: "Your unsaved theme changes will be lost.",
    discardChanges: "Discard changes",
    keepEditing: "Keep editing",
    operationFailed: "The theme operation failed. Your editor remains open.",
    jsonCopied: "JSON copied to clipboard",
    jsonCopyFailed: "Could not copy JSON",
    jsonSaving: "Saving…",
    jsonSaved: "Saved to file",
    jsonSavedToDownloads: (name: string) => `Saved to Downloads as ${name}`,
    jsonSaveFailed: "Could not save file",
    dismissFileNotification: "Dismiss theme file notification",
    jsonUpdated: "Theme updated from JSON",
    day: "day",
    night: "night",
    iconTagLabel: (label: string) => `Icon tag: ${label}`,
    iconTagEditableLabel: (label: string) =>
      `Icon tag: ${label} (decorative, click to flip)`,
    builtInIconTag: "Built-in theme icon tag",
    iconTagTitle: (label: string) =>
      `Decorative tag for ${label} use. Click to flip.`,
    themeName: "Theme name",
    sectionsLabel: "Theme editor sections",
    originalValue: "Already at original value",
    restoreOriginal: "Restore original value",
    resetOriginal: (label: string) =>
      `Reset ${label} to its original value`,
    builtInReadOnly: "Built-in themes are read-only",
    linkedColorsResetThroughSource: "Linked colors reset through their source",
    isolateEditLabel: (label: string) => `Isolated edit ${label}`,
    isolateEditTitle: "Edit this color independently of its source",
    isolateEdit: "Isolated edit",
    linkBackLabel: (label: string) => `Link back ${label} to its source`,
    linkBackTitle: "Re-link this color to its source",
    linkBack: "Link back",
    backgroundShort: "Bg",
    textShort: "Text",
    backgroundControl: (label: string) => `${label} background`,
    textControl: (label: string) => `${label} text`,
    sourceControl: (label: string) => `${label} source`,
    expandGroupLabel: (label: string) => `Expand ${label} options`,
    collapseGroupLabel: (label: string) => `Collapse ${label} options`,
    expand: "Expand",
    collapse: "Collapse",
    colorDefaults: "Color defaults",
    colorDefaultsDescription: "Surface, palette, and details",
    customCalendarDefault: "Custom calendar default",
    controlsLabel: "Theme editor controls",
    contrastIssue: (count: number) =>
      `${count} contrast ${count === 1 ? "issue" : "issues"}`,
    jumpToNextContrast: "Jump to next failing contrast row",
    jumpToNextContrastTitle:
      "Scroll to the next row below its contrast target. Muted surfaces target 3:1; everything else targets 4.5:1.",
    jumpToNext: "Jump to next",
    fixAllContrast: "Auto-fix every failing contrast row",
    fixAllContrastTitle:
      "Pick a legible text color for every pair below its target",
    fixAll: "Fix all",
    contrastReadOnlyLabel: (label: string, ratio: string) =>
      `${label} text contrast is ${ratio}:1`,
    contrastAutoFixLabel: (label: string) =>
      `Auto-fix ${label} text contrast`,
    contrastTargetAaBody: "AA body text",
    contrastTargetAaLargeUi: "AA large/UI",
    contrastTitle: (ratio: string, target: string, suffix: string) =>
      `Contrast ${ratio}:1. This pair targets ${target}:1 (${suffix}).`,
    contrastTitleEditable: (ratio: string, target: string, suffix: string) =>
      `Contrast ${ratio}:1. This pair targets ${target}:1 (${suffix}). Click to auto-pick a legible text color.`,
    eventPalette: "Event palette",
    eventPaletteDescription:
      "32 color slots, each one has a faded variant for optional past-event dimming, blended toward calendar background",
    pastVariant: (color: string) => `Past variant ${color}`,
    nav: {
      general: "General",
      calendar: "Calendar",
      signals: "Text and actions",
      json: "JSON",
    },
    calendarDefault: {
      light: "Light-based",
      dark: "Dark-based",
      appCanvas: "App canvas-based",
      custom: "Custom-based",
    },
    token: {
      appCanvas: {
        title: "App canvas",
        description: "Most views paint their own surface over it",
      },
      calendarHeader: {
        title: "Calendar header",
        description: "Calendar toolbar and day/time headers",
      },
      textColor: {
        title: "Text color",
        description: "Applied to text across the app",
      },
      card: {
        title: "Card",
        description: "Background of grouped panels, dialogs, and tinted cards",
      },
      primaryAction: {
        title: "Primary action",
        description: "Main accent color for highlighted buttons and links",
      },
      buttonText: {
        title: "Button text",
        description: "Text on primary buttons",
      },
      destructive: {
        title: "Destructive",
        description: "Color used for delete actions and warnings",
      },
      destructiveText: {
        title: "Destructive text",
        description:
          "Text color on destructive buttons and the title bar close hover",
      },
      focusRing: {
        title: "Focus ring",
        description: "Outline shown around focused inputs and buttons",
      },
      eventPanelSurface: {
        title: "Event panel surface",
        description: "Background of the event creation/edit panel",
      },
      eventPanelSectionHeader: {
        title: "Event panel section header",
        description: "Background strip behind section rows",
      },
      eventPanelBodyText: {
        title: "Event panel body text",
        description: "Overrides --foreground inside the panel",
      },
      eventPanelMutedText: {
        title: "Event panel muted text",
        description: "Secondary text color inside the panel",
      },
      confirmAction: {
        title: "Confirm action",
        description:
          "Background of the Save button and the active scope selector pill in the event panel",
      },
      confirmActionText: {
        title: "Confirm action text",
        description: "Text color on the Save button and active scope pill",
      },
      armedDelete: {
        title: "Armed delete",
        description:
          "Background of the delete button once it has been armed (click-again-to-confirm state)",
      },
      armedDeleteText: {
        title: "Armed delete text",
        description: "Text color on the delete button in its armed state",
      },
      acceptedAttendee: {
        title: "Accepted attendee",
        description: "Status tile color for accepted attendees on a calendar event",
      },
      acceptedAttendeeText: {
        title: "Accepted attendee text",
        description: "Text color on the accepted attendance tile",
      },
      tentativeAttendee: {
        title: "Tentative attendee",
        description: "Status tile color for tentative attendees on a calendar event",
      },
      tentativeAttendeeText: {
        title: "Tentative attendee text",
        description: "Text color on the tentative attendance tile",
      },
      declinedAttendee: {
        title: "Declined attendee",
        description: "Status tile color for declined attendees on a calendar event",
      },
      declinedAttendeeText: {
        title: "Declined attendee text",
        description: "Text color on the declined attendance tile",
      },
      calendarBackground: {
        title: "Calendar background",
        description: "Background of the calendar grid",
      },
      gridLines: {
        title: "Grid lines",
        description: "Color of the hour and day separator lines",
      },
      timeLabels: {
        title: "Time labels",
        description: "Hour numbers down the side of the calendar",
      },
      nowLine: {
        title: "Now line",
        description: "Horizontal line marking the current time",
      },
      emptyRail: {
        title: "Empty rail",
        description: "Color of empty parts of the pomodoro session rail",
      },
      breakMarker: {
        title: "Break marker",
        description: "Color of break segments on the session rail",
      },
      focusMarker: {
        title: "Focus marker",
        description: "Color of focus segments on the session rail",
      },
    },
    group: {
      appCanvas: {
        title: "App canvas",
        description:
          "Dominant background color, most surfaces tint automatically from it",
      },
      calendarSurface: {
        title: "Calendar surface",
        description: "Calendar background, gridlines, and time labels",
      },
      calendarDetails: {
        title: "Calendar details",
        description: "Rail states and accents on the calendar grid",
      },
      eventPanel: {
        title: "Event panel",
        description:
          "Surfaces on the event creation and edit panel opened from the calendar",
      },
      ink: {
        title: "Ink",
        description: "Base text color",
      },
      primaryAction: {
        title: "Primary action",
        description: "Accent for highlighted buttons and links",
      },
      destructive: {
        title: "Destructive",
        description: "Danger signal",
      },
      confirm: {
        title: "Confirm",
        description: "Positive signal",
      },
      warning: {
        title: "Warning",
        description: "Caution signal",
      },
    },
    pair: {
      popover: {
        title: "Popover",
        description: "Dropdowns, menus, and floating panels",
      },
      secondarySurface: {
        title: "Secondary surface",
        description: "Less emphasized buttons",
      },
      mutedSurface: {
        title: "Muted surface",
        description: "Subtle wells and the default hint-text color",
      },
      hoverHighlight: {
        title: "Hover highlight",
        description: "Soft tint shown when hovering rows and buttons",
      },
      titleBar: {
        title: "Title bar",
        description: "Top frame of the app window",
      },
      titleBarHover: {
        title: "Title bar hover",
        description: "Tint shown when hovering title bar buttons",
      },
      destructive: {
        title: "Destructive",
        description: "Delete actions, armed delete, and declined status",
      },
      confirm: {
        title: "Confirm",
        description: "Save actions, active pills, and accepted status",
      },
      warning: {
        title: "Warning",
        description: "Tentative status and caution surfaces",
      },
    },
  },
} as const;
