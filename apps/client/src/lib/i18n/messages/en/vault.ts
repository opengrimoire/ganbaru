export const vaultSetup = {
  welcomeTitle: "Free and open source",
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
  languageSelectorLabel: "Choose language",
  languageSearchPlaceholder: "Search languages...",
  noLanguagesFound: "No languages found.",
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
} as const;

export const language = {
  preferenceLabel: "Language",
  preferenceDescription: "Choose the app language.",
  systemOption: "System language",
  englishOption: "English",
  spanishOption: "Spanish",
} as const;

export const vaultOwnership = {
  recovery: "This vault needs handoff recovery before it can be changed.",
} as const;

export const vaultOwnershipPrompt = {
  title: (device: string) => `${device} is the main device`,
  description: "Only the main device can make changes.",
  replicaDescription: "Quick notes, preferences, themes, contacts, the Music library, Projects, and Calendar stay editable here and sync with your devices. Only the main device can change everything else.",
  ownerRequiredDescription: "This change also affects data that only the main device can change. Use this device as the main device to make it.",
  continueReadOnly: "Continue in read-only",
  switching: "Switching to this device...",
  replacementTitle: "Replace this device's current data?",
  replacementAndroid:
    "This phone has a separate Ganbaru vault. Its data cannot be merged with the linked vault. Ganbaru will save a complete backup to Downloads before replacing it.",
  replacementDesktop:
    "This computer has a separate Ganbaru vault. Its data cannot be merged with the linked vault. Ganbaru will preserve a recovery copy beside the current Ganbaru folder before replacing it.",
  replacementConfirm: "Replace with linked vault",
} as const;

export const vaultHandoff = {
  heading: "Linked devices",
  description: "Manage your connected devices",
  desktopOnboardingTitle: "Connect your phone",
  desktopOnboardingDescription:
    "Install Ganbaru on your phone and scan this QR code in the app to link your devices.",
  androidOnboardingTitle: "Connect to your computer",
  androidOnboardingDescription:
    "Use Ganbaru on this phone to scan the QR code shown on your computer.",
  stepOne: "Step 1",
  stepTwo: "Step 2",
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
  networkAccessDevelopmentUnavailable:
    "This Linux development build cannot use the packaged firewall helper. Use a packaged build or configure the firewall manually to link a phone.",
  notLinked: "No devices linked",
  devicesHeading: "Devices",
  linkedTo: (device: string) => `Linked to ${device}`,
  desktopDevice: "Desktop",
  androidDevice: "Phone",
  anotherDevice: "Another device",
  transferPending: "A vault transfer is waiting to finish.",
  linkPhone: "Link phone",
  linkToDesktop: "Link to desktop",
  createQr: "Show pairing QR code",
  linkAnotherDevice: "Link another device",
  enterCode: "Enter pairing code",
  linkThisComputer: "or link this computer to another",
  codeDialogTitle: "Link this computer",
  codeInstructions: "On the other computer, open Settings > Data and copy its pairing code. Paste it here.",
  codePlaceholder: "Pairing code",
  codeInvalid: "That pairing code is not valid. Copy a new code from the other computer and try again.",
  codeExpired: "That pairing code has expired. Get a new code from the other computer and try again.",
  linkDevice: "Link device",
  copyCode: "Copy pairing code",
  codeCopied: "Pairing code copied",
  copyCodeFailed: "The pairing code could not be copied.",
  enrollmentApprovalTitle: "Approve this device?",
  enrollmentApprovalInstructions:
    "Approve only if the device shows this same code. If it does not, decline: someone else may have your pairing code.",
  enrollmentCode: "Confirmation code",
  enrollmentApprove: "Approve",
  enrollmentDecline: "Decline",
  enrollmentWaitingTitle: "Confirm on the other computer",
  enrollmentWaiting: "The other computer shows a code. It approves this device when its code matches this one.",
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
  notNow: "Not now",
  continue: "Continue",
  deviceLabel: "Phone",
  useHere: "Switch to this device",
  refreshCopy: "Get latest changes",
  retry: "Retry",
  cancel: "Cancel transfer",
  unlink: "Unlink device",
  unlinkTitle: "Unlink this device?",
  unlinkMessage:
    "The current owner will not change. A read-only copy stays read-only until you explicitly recover it.",
  recover: "Recover this local copy",
  recoverTitle: "Use this copy as a separate vault?",
  recoverMessage:
    "Use this only if the owning device is permanently unavailable. Both copies are preserved, this device becomes writable, and the devices are unlinked. Later changes will not merge automatically.",
  workingOwnership: "Preparing this device...",
  workingRefresh: "Getting the latest changes...",
  workingRequest: "Waiting for the other device...",
  linked: "Device linked.",
  linkedSuccessfully: (device: string) => `${device} was successfully linked.`,
  linkedLabel: "linked",
  refreshed: "Latest changes received.",
  requestSent: "The request was sent. Keep both apps open on the same network.",
  unlinked: "Device unlinked.",
  revokedByCoordinator:
    "This device was unlinked by the coordinating computer. Its local copy remains read-only unless you recover it as a separate vault.",
  recovered: "This local copy is now a separate writable vault.",
  localBackup:
    "Before the first desktop vault replaces Android data, Ganbaru saves a recoverable backup to Downloads.",
  sameNetwork: "Keep both apps open on the same local network.",
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
  ownershipChurnLimit:
    "Editing moved between devices too many times in the last hour. Wait a while, then switch devices again.",
  bundleRequestLimit:
    "This device asked for the vault too many times in the last hour. Wait a while, then refresh or switch devices again.",
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
  ownerUnreachable:
    "The main device is unavailable. Keep it open on the same local network, then retry.",
  unknownError: "Unknown error",
} as const;
