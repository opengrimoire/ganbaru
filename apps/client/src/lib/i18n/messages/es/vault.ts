import type {
  dataFolderError as enDataFolderError,
  language as enLanguage,
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
  linkedAndroid:
    "Este teléfono está vinculado a otros dispositivos. Desvincula primero este teléfono y luego restaura.",
  anchorRollback:
    "Esta computadora es el ancla de tus dispositivos vinculados y esta copia desharía cambios que ellos ya siguen. Traspasa el ancla o quita esos dispositivos y luego restaura.",
} as const satisfies MessageShape<typeof enDataFolderError>;

export const language = {
  preferenceLabel: "Idioma",
  preferenceDescription: "Elige el idioma de la aplicación.",
  systemOption: "Idioma del sistema",
  englishOption: "Inglés",
  spanishOption: "Español",
} as const satisfies MessageShape<typeof enLanguage>;

export const vaultHandoff = {
  heading: "Dispositivos vinculados",
  description: "Administra tus dispositivos conectados",
  desktopOnboardingTitle: "Conecta tu teléfono",
  desktopOnboardingDescription:
    "Instala Ganbaru en tu teléfono y escanea este código QR en la aplicación para vincular tus dispositivos",
  androidOnboardingTitle: "Conecta este teléfono",
  androidOnboardingDescription:
    "¿Ya usas Ganbaru en una computadora o en tu propio servidor? Escanea el código QR que te muestra",
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
  notLinked: "No hay dispositivos vinculados",
  anchorServer: "Servidor ancla",
  relayServer: "Servidor relé",
  syncSettings: "Ajustes de sincronización",
  devicesHeading: "Dispositivos",
  desktopDevice: "Computadora",
  androidDevice: "Teléfono",
  linkToDesktop: "Vincular con computadora",
  createQr: "Mostrar código QR de vinculación",
  linkAnotherDevice: "Vincular otro dispositivo",
  enterCode: "Conectar con una computadora o servidor",
  linkThisComputer: "o conecta esta computadora con otra computadora o un servidor",
  codeDialogTitle: "Conectar esta computadora",
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
    "Apruébalo solo si el otro dispositivo muestra este mismo código",
  enrollmentCode: "Código de confirmación",
  enrollmentApprove: "Aprobar",
  enrollmentDecline: "Rechazar",
  enrollmentWaitingTitle: "Confirma en tu otro dispositivo",
  enrollmentWaiting: "Apruébalo ahí si muestra este mismo código",
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
  connectTabs: {
    computer: "Otra computadora",
    server: "Un servidor",
  },
  serverConnect: {
    description: "En tu servidor de Ganbaru, ejecuta ganbaru-server anchor setup y pega el código que imprime. Esta computadora se une a la bóveda que ejecuta el servidor, en una carpeta nueva.",
    startDescription: "En tu servidor de Ganbaru, ejecuta ganbaru-server anchor setup y pega el código que imprime. Si el servidor ejecuta una bóveda, esta computadora se une a ella en una carpeta nueva. Si el servidor está vacío, recibe esta bóveda y se convierte en su ancla.",
    open: "Conectar con un servidor",
    phoneDescription: "En tu servidor de Ganbaru, ejecuta ganbaru-server anchor setup y luego escanea el código QR que muestra o pega su código. Este teléfono se une a la bóveda que ejecuta el servidor, que reemplaza la bóveda de aquí. Lo que ya hayas agregado aquí se respalda antes.",
    scan: "Escanear el código QR del servidor",
    paste: "Pegar el código",
    useComputer: "Conectar con una computadora",
    codeLabel: "Código de configuración del servidor",
    useKit: "Usar el kit de recuperación",
    kitDescription: "Solo hace falta cuando el servidor se niega a agregar este dispositivo por sí mismo.",
    kitLabel: "Código del kit de recuperación",
    action: "Conectar",
    connecting: "Conectando…",
    errors: {
      code: "El servidor rechazó este código de configuración. Imprime uno nuevo en el servidor e inténtalo de nuevo.",
      kit: "Este código del kit de recuperación está mal escrito. Revisa cada grupo.",
      rejected: "El servidor rechazó este kit de recuperación. Puede pertenecer a otra bóveda.",
      joinRejected: "El servidor se negó a agregar este dispositivo. Inténtalo de nuevo con tu kit de recuperación.",
      waiting: "El servidor agrega este dispositivo cuando el ancla de la bóveda esté en línea. Abre Ganbaru en tus otros dispositivos e inténtalo de nuevo con el mismo código.",
      notEnrolled: "La bóveda de este servidor aún no agregó este dispositivo. Inténtalo de nuevo en un momento.",
      noVault: "Este servidor aún no ejecuta ninguna bóveda. Agrégalo primero desde una computadora que tenga la bóveda.",
      alreadyOpen: "Esta bóveda ya está abierta en este dispositivo.",
      wrongKit: "Este kit de recuperación no pertenece a la bóveda de este servidor. Revisa el kit y el código de configuración.",
      unreachable: "Este dispositivo no pudo llegar al servidor. Comprueba que esté en marcha y, si tiene un cortafuegos, permite su puerto (43823 por defecto) desde esta red.",
      failed: "Este dispositivo no pudo conectarse al servidor. Comprueba que el servidor esté en marcha e inténtalo de nuevo con un código de configuración nuevo.",
    },
  },
  copyHere: "Copiar la bóveda a este dispositivo",
  copyHereDescription: "Este dispositivo está vinculado. Copia aquí la bóveda vinculada para empezar a sincronizarla.",
  copied: "La bóveda vinculada ya está en este dispositivo.",
  copyInProgress: "La bóveda vinculada ya se está copiando a este dispositivo.",
  firstCopyTitle: "¿Copiar la bóveda vinculada a este dispositivo?",
  firstCopyAndroid:
    "La bóveda vinculada reemplaza la bóveda de este teléfono, cuyos datos no se pueden combinar con ella. Ganbaru guarda una copia de la bóveda reemplazada en este teléfono, que puedes exportar o eliminar en Ajustes > Datos.",
  firstCopyDesktop:
    "Ganbaru copia la bóveda vinculada en su propia carpeta de esta computadora. Tu bóveda actual no cambia y sigue en la lista de bóvedas.",
  firstCopyConfirm: "Copiar bóveda",
  linkFolder: "Carpeta para la bóveda vinculada",
  linkFolderDefault: "Ubicación predeterminada",
  chooseLinkFolder: "Elegir carpeta",
  useDefaultLinkFolder: "Usar predeterminada",
  retry: "Reintentar",
  cancel: "Cancelar transferencia",
  unlink: "Desvincular dispositivo",
  unlinkTitle: "¿Desvincular este dispositivo?",
  unlinkMessage:
    "El dispositivo deja de sincronizarse con esta bóveda. Cada dispositivo conserva los datos que ya tiene.",
  recover: "Conservar esta copia como bóveda propia",
  recoverTitle: "¿Conservar esta copia como bóveda propia?",
  recoverMessage:
    "Usa esto solo si la computadora coordinadora ya no volverá a estar disponible. Este dispositivo olvida a su coordinador y a sus dispositivos vinculados, y conserva esta copia como su propia bóveda. Los cambios hechos en otros dispositivos ya no se sincronizarán con ella.",
  workingCopy: "Copiando la bóveda vinculada a este dispositivo...",
  linkedTitle: (device: string) => `${device} está vinculado`,
  linkedDescription: "Tus dispositivos ya se sincronizan entre sí",
  linkedLabel: "vinculado",
  unlinked: "Dispositivo desvinculado.",
  revokedByCoordinator:
    "Este dispositivo fue desvinculado por la computadora coordinadora. Su copia de la bóveda se queda en este dispositivo, pero ya no se sincroniza.",
  recovered: "Esta copia ahora es una bóveda propia en este dispositivo.",
  localBackup:
    "Cuando la bóveda vinculada reemplaza la bóveda de este teléfono, Ganbaru guarda una copia de la reemplazada en Ajustes > Datos.",
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
  replacedVaultBackupFailed:
    "La bóveda vinculada no se copió porque este dispositivo no tiene espacio para guardar una copia de su bóveda actual. Libera espacio y reintenta.",
  linkFolderNotEmpty: "La carpeta elegida no está vacía. Elige una carpeta vacía para la bóveda vinculada.",
  linkFolderInsideVault:
    "La carpeta elegida está dentro de otra bóveda. Elige una carpeta fuera de cualquier bóveda para la bóveda vinculada.",
  bundleRequestLimit:
    "Este dispositivo pidió la bóveda demasiadas veces en la última hora. Espera un rato y vuelve a intentarlo.",
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
  unknownError: "Error desconocido",
} as const satisfies MessageShape<typeof enVaultHandoff>;
