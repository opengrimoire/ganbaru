import type {
  dataFolderError as enDataFolderError,
  language as enLanguage,
  vaultOwnership as enVaultOwnership,
  vaultOwnershipPrompt as enVaultOwnershipPrompt,
  vaultHandoff as enVaultHandoff,
  vaultSetup as enVaultSetup,
} from "../en/vault";
import type { MessageShape } from "../types";

export const vaultSetup = {
  welcomeTitle: "Gratis y de código abierto",
  licenseButton: "Licencia (AGPL 3.0)",
  sourceCode: "Código fuente",
  sourceOpenError: "No se pudo abrir el código fuente. Visita github.com/opengrimoire/ganbaru en tu navegador.",
  continue: "Continuar",
  title: "Elige dónde guardar tus datos",
  intro:
    "Si vienes de una instalación anterior, importa tu carpeta de Ganbaru existente.",
  developmentBuildWarning: (folderName: string) =>
    `Compilación de desarrollo: usa la carpeta ${folderName} predeterminada, o elige una copia de tu carpeta de producción. No apuntes dev a tus datos reales de producción.`,
  defaultLocation: "Carpeta predeterminada",
  useDefaultFolder: "Usar la carpeta predeterminada",
  changeFolder: "Cambiar carpeta",
  importFolder: "Importar carpeta",
  languageSelectorLabel: "Elegir idioma",
  languageSearchPlaceholder: "Buscar idiomas...",
  noLanguagesFound: "No se encontraron idiomas.",
} as const satisfies MessageShape<typeof enVaultSetup>;

export const dataFolderError = {
  startup:
    "No se puede abrir esta carpeta. Elige otra o importa una existente.",
  default:
    "No se puede usar la carpeta predeterminada. Revisa sus permisos o elige otra.",
  change:
    "No se puede usar esta carpeta. Elige una carpeta vacía o una de Ganbaru.",
  import:
    "No se puede importar esta carpeta. Elige la de tu instalación anterior.",
  backup: "No se pudo crear la copia de seguridad.",
  restore: "No se pudo restaurar esta copia de seguridad.",
  general: "No se puede usar esta carpeta.",
  unknown: "Error de carpeta desconocido.",
  permission:
    "No se puede acceder a esta carpeta. Revisa los permisos o elige otra.",
  database:
    "No se pueden abrir los datos de esta carpeta. Restaura una copia o elige otra.",
  defaultNotValid:
    "La carpeta predeterminada contiene otros archivos. Muévelos o elige otra carpeta.",
  folderNotEmpty:
    "Esta carpeta no está vacía. Elige una carpeta vacía o una de Ganbaru.",
  missingMarker:
    "No se reconoce esta carpeta. Elige la carpeta principal de Ganbaru.",
  damagedMarker:
    "La información de esta carpeta está dañada. Restaura una copia o elige otra.",
  newerSchema:
    "Esta carpeta necesita una versión más nueva. Actualiza Ganbaru.",
  notGanbaruFolder:
    "No es una carpeta de Ganbaru. Elige la de tu instalación anterior.",
  notFound:
    "No se encontró la carpeta. Elige otra o importa una existente.",
  openElsewhere:
    "Esta carpeta está abierta en otra app de Ganbaru. Cierra esa app o elige otra carpeta.",
} as const satisfies MessageShape<typeof enDataFolderError>;

export const language = {
  preferenceLabel: "Idioma",
  preferenceDescription: "Elige el idioma de la aplicación.",
  systemOption: "Idioma del sistema",
  englishOption: "Inglés",
  spanishOption: "Español",
} as const satisfies MessageShape<typeof enLanguage>;

export const vaultOwnership = {
  recovery: "Esta bóveda necesita recuperar la transferencia antes de poder modificarse.",
} as const satisfies MessageShape<typeof enVaultOwnership>;

export const vaultOwnershipPrompt = {
  title: (device: string) => `${device} es el dispositivo principal`,
  description: "Solo el dispositivo principal puede hacer cambios.",
  replicaDescription: "Las notas rápidas siguen editables aquí y se sincronizan con tus dispositivos. Solo el dispositivo principal puede cambiar todo lo demás.",
  ownerRequiredDescription: "Este cambio también afecta datos que solo el dispositivo principal puede cambiar. Usa este dispositivo como principal para hacerlo.",
  continueReadOnly: "Continuar en solo lectura",
  switching: "Cambiando a este dispositivo...",
  replacementTitle: "¿Reemplazar los datos actuales de este dispositivo?",
  replacementAndroid:
    "Este teléfono tiene una bóveda de Ganbaru separada. Sus datos no se pueden combinar con la bóveda vinculada. Ganbaru guardará una copia completa en Descargas antes de reemplazarla.",
  replacementDesktop:
    "Esta computadora tiene una bóveda de Ganbaru separada. Sus datos no se pueden combinar con la bóveda vinculada. Ganbaru conservará una copia de recuperación junto a la carpeta actual de Ganbaru antes de reemplazarla.",
  replacementConfirm: "Reemplazar con la bóveda vinculada",
} as const satisfies MessageShape<typeof enVaultOwnershipPrompt>;

export const vaultHandoff = {
  heading: "Dispositivos vinculados",
  description: "Administra tus dispositivos conectados",
  desktopOnboardingTitle: "Conecta tu teléfono",
  desktopOnboardingDescription:
    "Instala Ganbaru en tu teléfono y escanea este código QR en la aplicación para vincular tus dispositivos.",
  androidOnboardingTitle: "Conecta con tu computadora",
  androidOnboardingDescription:
    "Usa Ganbaru en este teléfono para escanear el código QR mostrado en tu computadora.",
  stepOne: "Paso 1",
  stepTwo: "Paso 2",
  networkAccessDescription:
    "Permite que Ganbaru reciba conexiones de tu teléfono en esta red.",
  allowNetworkAccess: "Permitir conexión del teléfono",
  networkAccessConfirmTitle: "¿Permitir la conexión del teléfono?",
  networkAccessConfirmMessage:
    "El firewall de Linux protege tu computadora de conexiones no deseadas. Ganbaru necesita su permiso para vincular tu teléfono con esta computadora mediante tu red local. Linux te pedirá tu contraseña, que Ganbaru nunca ve ni guarda.",
  networkAccessNetworkConfirmMessage:
    "Permitiste las conexiones del teléfono en otra red que usa las mismas direcciones que esta. ¿Permitirlas también en esta red? Linux no te pedirá tu contraseña.",
  networkAccessNetworkConfirmation: "Los dispositivos vinculados no pueden conectarse hasta que permitas esta red",
  networkAccessRevokeTitle: "¿Quitar el acceso a la red local?",
  networkAccessRevokeMessage:
    "El firewall de Linux permite actualmente que los dispositivos vinculados se conecten con Ganbaru en esta red local. Linux te pedirá tu contraseña para quitar ese permiso. Ganbaru nunca ve ni guarda tu contraseña.",
  networkAccessRevokeAction: "Quitar acceso",
  networkAccessHeading: "Acceso a la red local",
  networkAccessAllowed: "Permite que los dispositivos vinculados se conecten a esta computadora",
  networkAccessNotRequired: "No se requiere permiso del firewall",
  networkAccessGranted: "Las conexiones del teléfono están permitidas en esta red.",
  networkAccessRevoked: "Se quitó el acceso para conectar el teléfono.",
  networkAccessManual:
    "Ganbaru no puede solicitar acceso a la red automáticamente en este sistema Linux.",
  networkAccessDevelopmentUnavailable:
    "Esta versión de desarrollo para Linux no puede usar el asistente de firewall empaquetado. Usa una versión empaquetada o configura el firewall manualmente para vincular un teléfono.",
  notLinked: "No hay dispositivos vinculados",
  devicesHeading: "Dispositivos",
  linkedTo: (device: string) => `Vinculado con ${device}`,
  desktopDevice: "Computadora",
  androidDevice: "Teléfono",
  anotherDevice: "Otro dispositivo",
  transferPending: "Hay una transferencia de la bóveda pendiente de terminar.",
  linkPhone: "Vincular teléfono",
  linkToDesktop: "Vincular con computadora",
  createQr: "Mostrar código QR de vinculación",
  linkAnotherDevice: "Vincular otro dispositivo",
  enterCode: "Introducir código de vinculación",
  linkThisComputer: "o vincula esta computadora con otra",
  codeDialogTitle: "Vincular esta computadora",
  codeInstructions: "En la otra computadora, abre Ajustes > Datos y copia su código de vinculación. Pégalo aquí.",
  codePlaceholder: "Código de vinculación",
  codeInvalid: "Ese código de vinculación no es válido. Copia uno nuevo de la otra computadora e inténtalo de nuevo.",
  codeExpired: "Ese código de vinculación venció. Obtén uno nuevo de la otra computadora e inténtalo de nuevo.",
  linkDevice: "Vincular dispositivo",
  copyCode: "Copiar código de vinculación",
  codeCopied: "Código de vinculación copiado",
  copyCodeFailed: "No se pudo copiar el código de vinculación.",
  enrollmentApprovalTitle: "¿Aprobar este dispositivo?",
  enrollmentApprovalInstructions:
    "Apruébalo solo si el dispositivo muestra este mismo código. Si no, recházalo: alguien más podría tener tu código de vinculación.",
  enrollmentCode: "Código de confirmación",
  enrollmentApprove: "Aprobar",
  enrollmentDecline: "Rechazar",
  enrollmentWaitingTitle: "Confirma en la otra computadora",
  enrollmentWaiting: "La otra computadora muestra un código. Aprueba este dispositivo cuando su código coincide con este.",
  enrollmentDeclined: "La otra computadora rechazó este dispositivo. Obtén ahí un código de vinculación nuevo e inténtalo de nuevo.",
  enrollmentUnanswered:
    "La otra computadora no aprobó este dispositivo a tiempo. Mantén abierto su código de vinculación e inténtalo de nuevo.",
  enrollmentBusy: "Otro dispositivo espera aprobación en la otra computadora. Espera a que se responda e inténtalo de nuevo.",
  enrollmentReplaced: "La otra computadora creó un código de vinculación nuevo. Usa el código nuevo para intentarlo de nuevo.",
  enrollmentGone: "Ese dispositivo dejó de esperar aprobación.",
  qrDialogTitle: "Conectar otro dispositivo",
  qrInstructions:
    "Para conectar un teléfono, escanea este código QR en Ajustes > Datos. Para conectar otra computadora, copia el código de vinculación e introdúcelo en Ajustes > Datos.",
  qrAlt: "Código QR de vinculación",
  refreshesIn: (time: string) => `El código se renueva en ${time}`,
  refreshingCode: "Renovando código...",
  scanQr: "Escanear código QR de la computadora",
  cameraStarting: "Iniciando cámara",
  notNow: "Ahora no",
  continue: "Continuar",
  deviceLabel: "Teléfono",
  useHere: "Cambiar a este dispositivo",
  refreshCopy: "Obtener los últimos cambios",
  retry: "Reintentar",
  cancel: "Cancelar transferencia",
  unlink: "Desvincular dispositivo",
  unlinkTitle: "¿Desvincular este dispositivo?",
  unlinkMessage:
    "El propietario actual no cambiará. Una copia de solo lectura seguirá así hasta que la recuperes explícitamente.",
  recover: "Recuperar esta copia local",
  recoverTitle: "¿Usar esta copia como una bóveda separada?",
  recoverMessage:
    "Usa esto solo si el dispositivo propietario ya no está disponible. Ambas copias se conservan, este dispositivo podrá escribir y los dispositivos se desvincularán. Los cambios posteriores no se combinarán automáticamente.",
  workingOwnership: "Preparando este dispositivo...",
  workingRefresh: "Obteniendo los últimos cambios...",
  workingRequest: "Esperando al otro dispositivo...",
  linked: "Dispositivo vinculado.",
  linkedSuccessfully: (device: string) => `${device} se vinculó correctamente.`,
  linkedLabel: "vinculado",
  refreshed: "Se recibieron los últimos cambios.",
  requestSent: "Se envió la solicitud. Mantén ambas apps abiertas en la misma red.",
  unlinked: "Dispositivo desvinculado.",
  revokedByCoordinator:
    "Este dispositivo fue desvinculado por la computadora coordinadora. Su copia local seguirá siendo de solo lectura a menos que la recuperes como una bóveda separada.",
  recovered: "Esta copia local ahora es una bóveda separada con escritura.",
  localBackup:
    "Antes de que la bóveda de la computadora reemplace datos de Android por primera vez, Ganbaru guarda una copia recuperable en Descargas.",
  sameNetwork: "Mantén ambas apps abiertas en la misma red local.",
  failed: (details: string) => `Falló la acción del dispositivo vinculado: ${details}`,
  blockerChat: "Termina o detén el trabajo activo de Chat antes de mover la bóveda.",
  blockerTerminal: "Cierra las terminales de Chat en ejecución antes de mover la bóveda.",
  blockerFocus: "Termina o detén la sesión activa de Pomodoro antes de mover la bóveda.",
  blockerTransfer: "Termina o reintenta primero la transferencia actual de la bóveda.",
  incompatibleVersions:
    "Actualiza Ganbaru en ambos dispositivos antes de vincular o transferir datos. Sus formatos de datos son diferentes.",
  differentCoordinator:
    "Este dispositivo está vinculado con otra computadora. Desvincúlalo antes de conectarlo con una computadora diferente.",
  linkedVaultInactive:
    "Hay otra bóveda abierta en este dispositivo, así que la vinculación está en pausa. Vuelve a abrir la bóveda vinculada para reanudarla.",
  coordinatorVaultInactive:
    "La computadora coordinadora tiene otra bóveda abierta. Abre allí la bóveda vinculada y reintenta.",
  ownershipChurnLimit:
    "La edición cambió de dispositivo demasiadas veces en la última hora. Espera un rato y vuelve a cambiar de dispositivo.",
  bundleRequestLimit:
    "Este dispositivo pidió la bóveda demasiadas veces en la última hora. Espera un rato y vuelve a actualizar o a cambiar de dispositivo.",
  deviceKeyMissing:
    "Falta la clave de vinculación de este dispositivo en el llavero del sistema. Desvincula este dispositivo y vuelve a vincularlo.",
  deviceKeyUnavailable:
    "No se pudo leer la clave de vinculación de este dispositivo desde el llavero del sistema. Desbloquea el llavero o vuelve a iniciar sesión y reintenta.",
  linkingDisabled:
    "La vinculación de dispositivos está desactivada en este dispositivo porque no se pudieron leer sus datos de vinculación guardados. Tu bóveda sigue funcionando aquí; reinicia la aplicación y, si continúa, vuelve a vincular el dispositivo.",
  coordinatorBusy:
    "La computadora coordinadora está atendiendo demasiadas conexiones en este momento. Espera un momento y reintenta.",
  unreachable:
    "Este dispositivo no pudo comunicarse con la computadora coordinadora. Mantén ambas aplicaciones abiertas en la misma red local y reintenta.",
  ownerUnreachable:
    "El dispositivo principal no está disponible. Mantenlo abierto en la misma red local y vuelve a intentarlo.",
  unknownError: "Error desconocido",
} as const satisfies MessageShape<typeof enVaultHandoff>;
