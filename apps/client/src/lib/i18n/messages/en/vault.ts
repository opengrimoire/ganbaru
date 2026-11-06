export const vaultSetup = {
  welcomeHeadline: "Plan your day. Then actually do it.",
  welcomeBody: "Plan in the calendar, focus right from it, and watch your week fill with the work you actually did.",
  film: {
    description:
      "A short animation of a day in the app: a Focus session is planned in the calendar, runs through its break and its finish, then sits among the week's other sessions as the view pulls back, where back-to-back sessions share one rhythm of short and long breaks.",
    writeChapter: "Write chapter 3",
    reviewNotes: "Review notes",
    lunch: "Lunch",
    draftOutline: "Draft outline",
    readSources: "Read sources",
  },
  language: {
    headline: "Choose your language",
    body: "You can change it later in Settings > Appearance.",
  },
  about: {
    headline: "Free and open source",
    body: "No account, no ads, and no tracking. Your data stays on your devices and syncs between them, never through servers of ours.",
    sceneDescription:
      "A short animation of Ganbaru's code, open for anyone to read: a copy of it goes to each of five people's devices, and each one then fills with that person's own things.",
  },
  stepProgress: (current: number, total: number) => `Step ${current} of ${total}`,
  licenseButton: "License (AGPL 3.0)",
  sourceCode: "Source code",
  sourceOpenError: "Could not open the source code. Visit github.com/opengrimoire/ganbaru in your browser.",
  continue: "Continue",
  title: "Choose where to store your data",
  intro:
    "If you are coming from a previous installation, import your existing Ganbaru folder.",
  developmentBuildWarning: (folderName: string) =>
    `Development build: use the default ${folderName} folder, or choose a copy of your production folder. Do not point dev at your real production data.`,
  defaultLocation: "Default folder",
  useDefaultFolder: "Use the default folder",
  changeFolder: "Change folder",
  importFolder: "Import folder",
} as const;

export const dataFolderError = {
  startup:
    "Can't open this folder. Choose another or import an existing one.",
  default:
    "Can't use the default folder. Check its permissions or choose another.",
  change:
    "Can't use this folder. Choose an empty or existing Ganbaru folder.",
  import:
    "Can't import this folder. Choose your previous Ganbaru folder.",
  backup: "Couldn't create a backup.",
  restore: "Couldn't restore this backup.",
  general: "Can't use this folder.",
  unknown: "Unknown folder error.",
  permission:
    "Can't access this folder. Check permissions or choose another.",
  database:
    "Can't open this folder's data. Restore a backup or choose another.",
  defaultNotValid:
    "The default folder contains other files. Move them or choose another folder.",
  folderNotEmpty:
    "This folder isn't empty. Choose an empty or existing Ganbaru folder.",
  missingMarker:
    "Can't identify this folder. Choose the main Ganbaru folder.",
  damagedMarker:
    "This folder's information is damaged. Restore a backup or choose another.",
  newerSchema:
    "This folder needs a newer app version. Update Ganbaru.",
  notGanbaruFolder:
    "Not a Ganbaru folder. Choose your previous app folder.",
  notFound:
    "Folder not found. Choose another or import an existing one.",
  openElsewhere:
    "This folder is open in another Ganbaru app. Close that app or choose another folder.",
  linkedAndroid:
    "This phone is linked to other devices. Unlink this phone first, then restore.",
  anchorRollback:
    "This computer anchors your linked devices and this copy would undo changes they already follow. Hand the anchor over or remove those devices, then restore.",
} as const;

export const language = {
  preferenceLabel: "Language",
  preferenceDescription: "Choose the app language.",
  systemOption: "System language",
  englishOption: "English",
  spanishOption: "Spanish",
} as const;

export const vaultHandoff = {
  heading: "Linked devices",
  description: "Manage your connected devices",
  androidOnboardingTitle: "Connect this phone",
  networkAccessDescription:
    "Allow Ganbaru to receive connections from your phone on this network.",
  allowNetworkAccess: "Allow phone connection",
  networkAccessConfirmTitle: "Allow phone connection?",
  networkAccessConfirmMessage:
    "Your Linux firewall protects your computer from unwanted connections. Ganbaru needs its permission to link your phone with this computer over your local network. Linux will ask for your password, which Ganbaru never sees or stores.",
  networkAccessNetworkConfirmMessage:
    "You allowed phone connections on another network that uses the same addresses as this one. Allow them on this network too? Linux will not ask for your password.",
  networkAccessNetworkConfirmation: "Linked devices cannot connect until you allow this network",
  networkAccessRevokeTitle: "Remove local network access?",
  networkAccessRevokeMessage:
    "Your Linux firewall currently allows linked devices to connect to Ganbaru on this local network. Linux will ask for your password to remove that permission. Ganbaru never sees or stores your password.",
  networkAccessRevokeAction: "Remove access",
  networkAccessHeading: "Local network access",
  networkAccessAllowed: "Allow linked devices to connect to this computer",
  networkAccessNotRequired: "No firewall permission is required",
  networkAccessGranted: "Phone connections are allowed on this network.",
  networkAccessRevoked: "Phone connection access was removed.",
  networkAccessManual:
    "Ganbaru cannot request network access automatically on this Linux system.",
  notLinked: "No devices linked",
  anchorServer: "Anchor server",
  relayServer: "Relay server",
  syncSettings: "Sync settings",
  devicesHeading: "Devices",
  desktopDevice: "Desktop",
  androidDevice: "Phone",
  createQr: "Show pairing QR code",
  linkAnotherDevice: "Link another device",
  enterCode: "Connect to a computer or server",
  connectTitle: "Connect to",
  connectTitleYour: "Connect to your",
  computerSteps: {
    open: "On your other computer, open",
    copy: "Copy its pairing code",
    install: "Install Ganbaru on your computer",
    goTo: "Go to",
  },
  phoneSteps: {
    install: "Install Ganbaru on your phone",
  },
  codePlaceholder: "Pairing code",
  codeInvalid: "That pairing code is not valid. Copy a new code from the other computer and try again.",
  codeExpired: "That pairing code has expired. Get a new code from the other computer and try again.",
  connectAction: "Connect",
  connectingAction: "Connecting…",
  copyCode: "Copy pairing code",
  codeCopied: "Pairing code copied",
  copyCodeFailed: "The pairing code could not be copied.",
  enrollmentApprovalTitle: "Approve this device?",
  enrollmentApprovalInstructions:
    "Approve only if the other device shows this same code",
  enrollmentCode: "Confirmation code",
  enrollmentApprove: "Approve",
  enrollmentDecline: "Decline",
  enrollmentWaitingTitle: "Confirm on your other device",
  enrollmentWaiting: "Approve there if it shows this same code",
  enrollmentDeclined: "The other computer declined this device. Get a new pairing code there and try again.",
  enrollmentUnanswered:
    "The other computer did not approve this device in time. Keep its pairing code open, then try again.",
  enrollmentBusy: "Another device is waiting for approval on the other computer. Wait until it is answered, then try again.",
  enrollmentReplaced: "The other computer created a new pairing code. Use the new code to try again.",
  enrollmentGone: "That device stopped waiting for approval.",
  qrDialogTitle: "Connect another device",
  qrInstructions:
    "To connect a phone, scan this QR code in Settings > Data. To connect another computer, copy the pairing code and enter it in Settings > Data.",
  qrAlt: "Pairing QR code",
  refreshesIn: (time: string) => `Code refreshes in ${time}`,
  refreshingCode: "Refreshing code...",
  scanQr: "Scan desktop QR code",
  cameraStarting: "Starting camera",
  cameraFailed: "The camera could not start.",
  codeMode: {
    scan: "Scan",
    paste: "paste",
    pairingCode: "the pairing code",
    serverCode: "the code it shows",
  },
  notNow: "Not now",
  continue: "Continue",
  connectTabs: {
    phone: "your phone",
    phoneComputer: "computer",
    phoneServer: "server",
    phoneServerLead: "a",
    computer: "another computer",
    server: "a server",
  },
  serverConnect: {
    installPrefix: "Use",
    installLink: "this guide",
    installSuffix: "to set up ganbaru-server on a computer that's always on, such as a home server or NAS",
    run: "On that server, run",
    copyCommand: "Copy command",
    commandCopied: "Command copied",
    outcome: "This computer joins the server's vault in a new folder.",
    startOutcome: "If the server is new, it gets a copy of this vault. If it already has one, this computer joins that vault in a new folder.",
    scan: "Scan server QR code",
    codeLabel: "Server setup code",
    useKit: "Use the recovery kit instead",
    kitPrompt: "Refused by the server?",
    kitLabel: "Recovery kit code",
    kitDialogTitle: "Use the recovery kit",
    kitDialogMessage: "Enter the code from the recovery kit you saved. It lets this phone join even if the server refused it.",
    kitAdded: "Recovery kit added.",
    removeKit: "Remove",
    errors: {
      code: "The server refused this setup code. Print a new one on the server, then try again.",
      kit: "This recovery kit code is mistyped. Check each group.",
      rejected: "The server refused this recovery kit. It may belong to another vault.",
      joinRejected: "The server refused to add this device. Try again with your recovery kit.",
      waiting: "The server adds this device once the vault's anchor is online. Open Ganbaru on your other devices, then try again with the same code.",
      notEnrolled: "The vault on this server has not added this device yet. Try again in a moment.",
      noVault: "This server runs no vault yet. Add it from a desktop that has the vault first.",
      alreadyOpen: "This vault is already open on this device.",
      wrongKit: "This recovery kit does not belong to the vault on this server. Check the kit and the setup code.",
      unreachable: "This device could not reach the server. Check that it is running, and if it has a firewall, allow its port (43823 by default) from this network.",
      failed: "This device could not connect to the server. Check that the server is running, then try again with a new setup code.",
    },
  },
  copyHere: "Copy vault to this device",
  copyHereDescription: "This device is linked. Copy the linked vault here to start syncing with it.",
  copied: "The linked vault is now on this device.",
  copyInProgress: "The linked vault is already being copied to this device.",
  firstCopyTitle: "Copy the linked vault to this device?",
  firstCopyAndroid:
    "The linked vault replaces this phone's vault, whose data cannot be merged with it. Ganbaru keeps a backup of the replaced vault on this phone, which you can export or delete in Settings > Data.",
  firstCopyDesktop:
    "Ganbaru copies the linked vault into its own folder on this computer. Your current vault stays as it is and remains in the vault list.",
  firstCopyConfirm: "Copy vault",
  linkFolder: "Folder for the linked vault",
  linkFolderDefault: "Default location",
  chooseLinkFolder: "Choose folder",
  useDefaultLinkFolder: "Use default",
  retry: "Retry",
  cancel: "Cancel transfer",
  unlink: "Unlink device",
  unlinkTitle: "Unlink this device?",
  unlinkMessage:
    "The device stops syncing with this vault. Each device keeps the data it already has.",
  recover: "Keep this copy as its own vault",
  recoverTitle: "Keep this copy as its own vault?",
  recoverMessage:
    "Use this only if the coordinating computer is gone for good. This device forgets its coordinator and linked devices and keeps this copy as its own vault. Changes made elsewhere will no longer sync with it.",
  workingCopy: "Copying the linked vault to this device...",
  linkedTitle: (device: string) => `${device} is linked`,
  linkedDescription: "Your devices now sync with each other",
  linkedLabel: "linked",
  unlinked: "Device unlinked.",
  revokedByCoordinator:
    "This device was unlinked by the coordinating computer. Its copy of the vault stays on this device but no longer syncs.",
  recovered: "This copy is now a vault of its own on this device.",
  localBackup:
    "When the linked vault replaces this phone's vault, Ganbaru keeps a backup of the replaced one in Settings > Data.",
  failed: (details: string) => `The linked-device action failed: ${details}`,
  blockerChat: "Finish or stop active Chat work before moving the vault.",
  blockerTerminal: "Close running Chat terminals before moving the vault.",
  blockerFocus: "Finish or stop the active Pomodoro session before moving the vault.",
  blockerTransfer: "Finish or retry the current vault transfer first.",
  incompatibleVersions:
    "Update Ganbaru on both devices before linking or transferring data. Their data formats are different.",
  differentCoordinator:
    "This device is linked to another computer. Unlink it before connecting to a different computer.",
  linkedVaultInactive:
    "Another vault is open on this device, so linking is paused. Open the linked vault again to resume.",
  coordinatorVaultInactive:
    "The coordinating computer has another vault open. Open the linked vault there, then retry.",
  replacedVaultBackupFailed:
    "The linked vault was not copied because this device has no room to keep a backup of its current vault. Free some storage, then retry.",
  linkFolderNotEmpty: "The chosen folder is not empty. Choose an empty folder for the linked vault.",
  linkFolderInsideVault:
    "The chosen folder is inside another vault. Choose a folder outside any vault for the linked vault.",
  bundleRequestLimit:
    "This device asked for the vault too many times in the last hour. Wait a while, then try again.",
  deviceKeyMissing:
    "This device's linking key is missing from the system keyring. Unlink this device, then link it again.",
  deviceKeyUnavailable:
    "This device's linking key could not be read from the system keyring. Unlock the keyring or sign in again, then retry.",
  linkingDisabled:
    "Device linking is off on this device because its saved linking data could not be read. Your vault still works here; restart the app, and if this persists, link the device again.",
  coordinatorBusy:
    "The coordinating computer is handling too many connections right now. Wait a moment, then retry.",
  unreachable:
    "This device could not reach the coordinating computer. Keep both apps open on the same local network, then retry.",
  unknownError: "Unknown error",
} as const;
