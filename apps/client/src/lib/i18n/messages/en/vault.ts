export const vaultSetup = {
  welcomeHeadline: "Calendar-driven Pomodoro",
  welcomeBody: "Plan your day as usual and the Pomodoro will run by itself.",
  film: {
    description:
      "A short animation of a day in the app: a long session is drawn in the calendar right before one already planned with the Creative Pomodoro, and the event panel opens on it. Picking the Thesis project from the University group gives the session the project's color and its Extended Pomodoro, so only the title is typed before saving. The two sessions share one Focus rail that carries one rhythm through both, so the second session's breaks move as the first one joins it and takes its project's rhythm. The first session runs through a break to its finish, the second carries on to complete the day, and both then sit among the week's other events as the view pulls back, where icons mark the repeating lunches, the sessions with music, and the meetings.",
    writeChapter: "Write chapter 3",
    reviewNotes: "Review notes",
    lunch: "Lunch",
    draftOutline: "Draft outline",
    readSources: "Read sources",
    teamMeeting: "Team meeting",
    problemSet: "Problem set",
    gym: "Gym",
    universityGroup: "University",
    healthGroup: "Health",
    personalGroup: "Personal",
    thesisProject: "Thesis",
    assignmentsProject: "Assignments",
  },
  language: {
    headline: "Choose your language",
    body: "You can change it later in Settings > Appearance.",
  },
  blocker: {
    mailTab: "Inbox",
    newTab: "New tab",
    yTab: (name: string) => `Home / ${name}`,
  },
  blocking: {
    headline: "No distractions while you work",
    body: "The sites and apps you pick stay closed while the Pomodoro runs, and reopen when it ends.",
    sceneDescription:
      "A short animation of a Calendar event that runs the Pomodoro, above a laptop's browser and a phone. Bluedit is open in the browser until the event starts; then Ganbaru's block page replaces it, and Y typed into the address bar gets the same page. Egogram opened on the phone returns to the home screen with a notification, and so does each try after. When the event ends, Y opens in the browser and Egogram opens as usual.",
  },
  limits: {
    headline: "Shared limits on every device",
    body: "Set a daily or weekly limit for the sites and apps you pick, shared by all your devices.",
    sceneDescription:
      "A short animation of a one hour daily limit for social media that holds the Y and Egogram websites and the Egogram app. Time on Y in a laptop's browser fills part of one bar, then the Y tab is closed and Egogram on a phone fills the rest, each in its own color. When the hour is used up, Egogram returns to the home screen with a notification, and each time it is opened again it returns there at once. Back on the laptop, Y typed into a new tab opens Ganbaru's block page instead, and so does Egogram's website.",
    limitName: "Social media",
  },
  music: {
    headline: "Your playlists start automatically",
    body: "Mix YouTube and local music in your playlists, and they'll play as soon as a work session or break begins.",
    sceneDescription:
      "A short animation of the playlist builder: a track from Local music is added to the Work (focus) playlist, then a video from YouTube to Break (calm). In the Calendar, an event gets the Thesis project, which has no music of its own, and its Music section sets Work (focus) for focus and Break (calm) for short breaks. When the event's Pomodoro starts on its own, a notification shows a local track from Work (focus) playing automatically, and when the break starts, another shows the YouTube video from Break (calm).",
    playingNotice: (track: string, playlist: string) => `Playing ${track} from ${playlist}`,
  },
  projects: {
    headline: "Set up once, use every day",
    body: "Set a project's default Pomodoro and playlists, and its events will use them automatically.",
    sceneDescription:
      "A short animation of the Novel project's settings, opened from its List view on Event defaults, where the event name is set to Writing session, the color to a new one, the duration to 2 hours, and the Pomodoro to Creative, a playlist is picked for focus, short breaks, and long breaks, and the settings are saved and closed. In the Calendar, an event is then dragged out, and picking Novel for it gives it the name Writing session, the new color, 2 hours, the Creative Pomodoro, and the three playlists.",
    project: "Novel",
    eventName: "Writing session",
    task: {
      outlinePlot: "Outline the plot",
      writeOpening: "Write the opening scene",
      buildCharacters: "Build character profiles",
      researchSetting: "Research the setting",
      draftChapter: "Write chapter 2",
      outlineScenes: "Outline the scenes",
      firstDraft: "Write the first draft",
    },
    status: {
      backlog: "Backlog",
      todo: "To do",
      inProgress: "In progress",
      inReview: "In review",
      blocked: "Blocked",
      done: "Done",
    },
  },
  tasks: {
    headline: "Organize your work in projects",
    body: "Split a big goal into small tasks and follow each one from start to finish.",
    sceneDescription:
      "A short animation of the Novel project's List view. The task Write chapter 2 is typed into the bottom of the list and added, then expanded to add the subtasks Outline the scenes and Write the first draft. The task's status and its first subtask's are then set to In progress and the task's priority to High from their cells in the table.",
  },
  notes: {
    headline: "Notes for each project",
    body: "Format your pages with Markdown or slash commands.",
    sceneDescription:
      "A short animation of the Characters page in the Novel project's Notes, which starts with a line of text, the heading Open threads, and a bullet. Typing three number signs and a space on the next line turns it into the heading Mateo's secret, and a paragraph is typed below it. A slash on the next line then opens the slash menu, a short query narrows it to Callout, and Callout is chosen to add the callout Decide what Mateo saw at the station. Further down, a row named Mateo is added to the page's Cast database and given the role Ally, then the database switches to its Board view, which shows a column for each role and scrolls sideways to the Ally column. Back at the top of the page, a right click on the first line opens its menu, where Quote under Paragraph turns the line into a quote. The pointer then rests on Characters in the top bar to list the project's notes and folders, and on the Worldbuilding folder to show the folder and notes inside it.",
    sceneDescriptionTouch:
      "A short animation of the Characters page in the Novel project's Notes, which starts with a line of text, the heading Open threads, and a bullet. Typing three number signs and a space on the next line turns it into the heading Mateo's secret, and a paragraph is typed below it. A slash on the next line then opens the slash menu, a short query narrows it to Callout, and Callout is chosen to add the callout Decide what Mateo saw at the station. Further down, a row named Mateo is added to the page's Cast database and given the role Ally, then the database switches to its Board view, which shows a column for each role and scrolls sideways to the Ally column. Back at the top of the page, a long press on a word of the first line selects it and opens its menu, where Quote under Paragraph turns the line into a quote. Tapping Characters in the top bar then lists the project's notes and folders, and tapping the Worldbuilding folder shows the folder and notes inside it.",
    page: "Characters",
    quote: "Everyone in this book is hiding something.",
    quoteWord: "hiding",
    threads: "Open threads",
    bullet: "Clara's letter goes missing",
    heading: "Mateo's secret",
    paragraph: "Clara and Tomás grew up on the same street, and only Mateo knows why they stopped talking.",
    slashQuery: "call",
    callout: "Decide what Mateo saw at the station.",
    database: "Cast",
    name: "Name",
    role: "Role",
    roles: { lead: "Lead", rival: "Rival", ally: "Ally" },
    characters: { clara: "Clara", tomas: "Tomás", mateo: "Mateo" },
    navigation: {
      chapters: "Chapters",
      research: "Research",
      worldbuilding: "Worldbuilding",
      plot: "Plot outline",
      style: "Style guide",
      timeline: "Timeline",
      maps: "Maps",
      glossary: "Glossary",
      oldTown: "Old town",
      station: "The station",
    },
  },
  quickNotes: {
    headline: "Write it down before it slips away",
    body: "Jot down quick notes in seconds and sort them with tags you can filter by.",
    sceneDescription:
      "A short animation of Quick notes, where the notes carry tags such as Ideas, Home, and Health. The notes Passport and flights and Hotel in Lisbon are each given the existing Trip tag from their tag pickers, which gives them the tag's color, and selecting the Trip chip filters the notes down to the ones tagged Trip.",
    sceneDescriptionTouch:
      "A short animation of Quick notes, where the notes carry tags such as Ideas, Home, and Health and several share the Trip tag, and tapping the Trip chip filters the notes down to them.",
    tags: { trip: "Trip", ideas: "Ideas", home: "Home", health: "Health" },
    notes: {
      gift: { title: "Gift for Ana", body: "She mentioned a ceramics class." },
      passport: { title: "Passport and flights", body: "Renew the passport before May." },
      pitch: { title: "Podcast idea", body: "Short interviews with people who changed careers." },
      packing: { title: "Packing list", body: "Adapter, sunscreen, and comfortable shoes." },
      dentist: { title: "Call the dentist", body: "Ask about a Thursday morning." },
      hotel: { title: "Hotel in Lisbon", body: "Near the river, breakfast included." },
      plants: { title: "App idea", body: "A watering reminder that checks the weather." },
      sintra: { title: "Day trip to Sintra", body: "Take the early train from Rossio." },
      groceries: { title: "Groceries", body: "Oat milk, lemons, rice, and coffee." },
      recipe: { title: "Lentil soup", body: "Cumin, lemon, and a little smoked paprika." },
    },
  },
  chat: {
    headline: "Bring in AI only when you want it",
    body: "Chat with AI agents like Claude, Codex, or Cursor that work right inside your project's folder.",
    sceneDescription:
      "A short animation of Chat in the Novel project, where Claude is mentioned in a question about what Mateo knows about Clara and answers in the message's thread after reading chapter 2 and searching for Mateo.",
    channel: "general",
    roles: { claude: "Story feedback", codex: "Ebook builds" },
    prompt: "read chapter 2 and tell me what Mateo knows about Clara.",
    chapterPath: "chapters/02.md",
    reply:
      "In chapter 2, Mateo sees Clara leave the station with the letter, but he never tells Tomás. That silence could set up their argument in chapter 3.",
  },
  about: {
    headline: "Free and open source",
    body: "Your data stays on your devices and syncs directly between them, with no account, ads, or telemetry.\nApp and code are yours forever.",
    sceneDescription:
      "A short animation of Ganbaru's public source code repository in a browser, with its files and its AGPL 3.0 license in view. A click on Star stars the repository.",
  },
  stepProgress: (current: number, total: number) => `Step ${current} of ${total}`,
  licenseButton: "License (AGPL 3.0)",
  sourceCode: "Source code",
  sourceOpenError: "Could not open the source code. Visit github.com/opengrimoire/ganbaru in your browser.",
  continue: "Continue",
  title: "Choose where to store your data",
  defaultLocation: "Default folder",
  useDefaultFolder: "Continue with the default folder",
  changeFolder: "Choose another folder",
  importFolder: "Import an existing folder",
  data: {
    illustrationDescription:
      "An illustration of the Ganbaru folder in a file manager, with folders for images and project files, the database that holds your calendar, tasks, and notes, and the file that names the folder.",
    entries: {
      assets: "Images and attachments",
      projects: "A working folder per project",
      database: "Calendar, tasks, notes, and more",
      marker: "The folder's name and version",
    },
  },
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
  continueWithoutLinking: "Continue without linking",
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
