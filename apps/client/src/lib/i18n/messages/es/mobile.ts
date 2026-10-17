import type { mobile as enMobile } from "../en/mobile";
import type { MessageShape } from "../types";

export const mobile = {
  primaryNavigation: "Navegación principal",
  createEvent: "Crear evento",
  theme: "Tema",
  startupFailed: "Ganbaru no pudo abrir sus datos móviles privados",
  pomodoroInactiveDescription:
    "Pomodoro comienza automáticamente cuando inicia un bloque de calendario con Pomodoro habilitado.",
  pomodoroOpenCalendar: "Abrir calendario",
  contextSwitcher: (current: string) => `${current}, cambiar`,
  vaultSetup: {
    title: "Configura tus datos",
    intro: "Empieza con datos nuevos, restaura una copia o importa una carpeta.",
    dataLocation: "Ubicación de datos",
    startFromZero: "Empezar de cero",
    restoreBackupFile: "Restaurar archivo de copia",
    importExistingFolder: "Importar carpeta existente",
  },
  focusOnboarding: {
    title: "Mantén Enfoque en funcionamiento",
    description:
      "Revisa estos ajustes de Android para iniciar eventos de Enfoque programados con Ganbaru cerrado.",
    notifications: "Notificaciones",
    notificationsDescription: "Muestra el progreso de Enfoque y las alertas de fase",
    exactAlarm: "Alarmas y recordatorios",
    exactAlarmDescription: "Inicia los eventos de Enfoque programados a la hora correcta",
    usageAccess: "Acceso de uso",
    usageAccessDescription: "Mide el tiempo en primer plano de las apps que elijas",
    appBlocking: "Bloqueo de apps",
    appBlockingDescription: "Vuelve al inicio cuando aplica una regla o límite seleccionado",
    backgroundRestricted: "Android actualmente restringe Ganbaru en segundo plano",
    review: "Revisar",
    continue: "Continuar",
    continueIn: (seconds: number) => `Continuar en ${seconds} s`,
    checkingAccess: "Comprobando el acceso de Android",
    statusError: "No se pudo consultar parte del acceso de Android. Aún puedes revisar cada ajuste.",
  },
  settings: {
    categoriesLabel: "Categorías de ajustes",
    backToCategories: "Volver a las categorías de ajustes",
    backToSection: "Volver a la sección",
    storageHeading: "Almacenamiento",
    privateStorage: "Privado",
    privateStorageDescription:
      "Android conserva estos datos en el almacenamiento privado de Ganbaru. Desinstalar la app los elimina.",
    androidUpdatesDescription: "Las actualizaciones de Android son APK firmados publicados con cada lanzamiento de GitHub.",
    androidLatestPublished: (date: string) => `Publicado ${date}`,
    downloadApk: "Descargar APK",
    viewReleases: "Ver",
  },
} as const satisfies MessageShape<typeof enMobile>;
