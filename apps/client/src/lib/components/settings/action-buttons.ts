/** Shared look of text buttons that sit in Settings rows beside fields and selects. */
const SETTINGS_ACTION_BUTTON_CLASS =
  "inline-flex h-(--control-height) shrink-0 items-center justify-center gap-1.5 rounded-md border border-border px-2.5 text-[0.8rem] font-medium transition-colors focus:outline-none focus-visible:ring-1 focus-visible:ring-ring disabled:pointer-events-none disabled:opacity-55";

/** Quiet Settings action, such as reload, export, or clear. */
export const SETTINGS_SECONDARY_BUTTON_CLASS = `${SETTINGS_ACTION_BUTTON_CLASS} bg-card text-foreground hover:bg-accent dark:bg-transparent`;

/** The one main action of a Settings block, such as saving an edited file. */
export const SETTINGS_PRIMARY_BUTTON_CLASS = `${SETTINGS_ACTION_BUTTON_CLASS} border-primary bg-primary text-primary-foreground hover:bg-primary/90`;

/** Square icon button that matches the reset button beside Settings selects. */
export const SETTINGS_ICON_BUTTON_CLASS =
  "flex size-(--control-height) shrink-0 items-center justify-center rounded-md border border-border bg-secondary text-secondary-foreground transition-colors hover:bg-accent hover:text-accent-foreground disabled:pointer-events-none disabled:opacity-40";
