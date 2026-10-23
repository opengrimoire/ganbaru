export const sync = {
  heading: "Sync",
  description: (features: string) => `${features} sync between your linked devices when both are on the same network.`,
  state: {
    off: "Off",
    idle: "Up to date",
    syncing: "Syncing...",
    offline: "Waiting for your other device",
    paused: "Paused",
    waiting_for_identity: "Waiting for your other device to confirm the link",
    error: "Sync stopped because of an error",
  },
  role: {
    hub: "Your other devices sync through this device",
    client: "This device syncs through your main device",
  },
  lastExchange: (time: string) => `Last synced ${time}`,
  neverExchanged: "Not synced yet",
  pending: (count: number) => `${count} ${count === 1 ? "change" : "changes"} waiting to sync`,
  waiting: (count: number) => `${count} received ${count === 1 ? "change" : "changes"} waiting to apply`,
  heldHeading: "Changes this device cannot use yet",
  heldNewerFormat: (count: number) =>
    `${count} ${count === 1 ? "change needs" : "changes need"} a newer version of Ganbaru`,
  heldNewerManifest: (count: number) =>
    `${count} ${count === 1 ? "change comes" : "changes come"} from a newer data layout. Update Ganbaru on this device.`,
  heldInvalid: (count: number) =>
    `${count} ${count === 1 ? "change was" : "changes were"} rejected because ${count === 1 ? "it was" : "they were"} invalid`,
  conflicts: (count: number) =>
    `${count} ${count === 1 ? "item was" : "items were"} edited on two devices. Open ${count === 1 ? "it" : "them"} to choose what to keep.`,
  lockUnsupported: "This folder's drive can't tell whether another Ganbaru app has it open. Don't open it from two apps at once, or your data can be damaged.",
  syncNow: "Sync now",
  syncNowFailed: "Sync could not start",
  pause: "Pause sync",
  pauseFailed: "The sync setting could not be changed",
  errorDetail: (message: string) => `Last error: ${message}`,
  writerLimit: "This device created more sync identities than a device may have, so the coordinator refuses its new changes. They stay on this device.",
  clockAhead: (device: string) =>
    `The clock on ${device} is ahead of this device. Its edits win conflicting changes until the clocks agree, so check its date and time.`,
  clockAheadOwn: "Some edits from this device were made while its clock was ahead. They win conflicting changes until that time passes.",
  bootstrapRequired: "This device was away too long to catch up. Your changes here still reach your main device, but to receive changes again, link this device again or transfer the vault to it.",
  uncounted: {
    dormant: (device: string) =>
      `Sync no longer waits for ${device}, which has not synced for a long time. When it returns, link it again or transfer the vault to it.`,
    relinkRequired: (device: string) => `Link ${device} again so it can sync.`,
  },
  connection: {
    lan: (address: string) => `Connected over local network at ${address}`,
    overlay: (address: string) => `Connected over overlay network at ${address}`,
  },
  degraded: {
    backgroundRestricted: "Android restricts Ganbaru in the background, so sync waits until you open the app.",
    standbyRestricted: "Android rarely lets Ganbaru run in the background, so sync can be late.",
    localNetworkDenied: "This device is not allowed to reach your other devices on the local network. Allow local network access for Ganbaru in the system settings.",
    firewallHub: "The firewall keeps your other devices from reaching this device.",
    firewallClient: "Your main device was not found on the network. A firewall on this device may be blocking it.",
    openBatterySettings: "Open battery settings",
    allowNetworkAccess: "Allow network access",
  },
  recovery: {
    heading: "Recently deleted with unsaved edits",
    description: "These were deleted on one device while another device was still editing them.",
    restore: "Restore",
    discard: "Discard",
    restored: "Restored",
    discarded: "Edits discarded",
    failed: "This could not be restored",
    discardFailed: "The edits could not be discarded",
    loadFailed: "Deleted items could not be loaded",
    untitled: "Untitled",
    descendants: (count: number) =>
      `Restoring also brings back ${count} ${count === 1 ? "item" : "items"} deleted with it`,
    editedBy: (device: string, time: string) => `Edited on ${device}, ${time}`,
    thisDevice: "this device",
    otherDevice: "another device",
  },
} as const;
