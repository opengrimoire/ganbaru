import { createHash } from "node:crypto";
import { readFile } from "node:fs/promises";
import path from "node:path";
import { fileURLToPath } from "node:url";

const scriptDir = path.dirname(fileURLToPath(import.meta.url));
const clientDir = path.resolve(scriptDir, "..", "..");
const androidDir = path.join(clientDir, "src-tauri", "gen", "android");

/** Read one generated Android source file. */
async function readAndroidFile(relativePath) {
  const filePath = path.join(androidDir, relativePath);
  try {
    return await readFile(filePath, "utf8");
  } catch (error) {
    const message = error instanceof Error ? error.message : String(error);
    throw new Error(`read generated Android ${relativePath}: ${message}`);
  }
}

/** Read one generated Android binary resource. */
async function readAndroidBytes(relativePath) {
  const filePath = path.join(androidDir, relativePath);
  try {
    return await readFile(filePath);
  } catch (error) {
    const message = error instanceof Error ? error.message : String(error);
    throw new Error(`read generated Android ${relativePath}: ${message}`);
  }
}

/** Read one client source file outside the generated Android project. */
async function readClientFile(relativePath) {
  const filePath = path.join(clientDir, relativePath);
  try {
    return await readFile(filePath, "utf8");
  } catch (error) {
    const message = error instanceof Error ? error.message : String(error);
    throw new Error(`read client ${relativePath}: ${message}`);
  }
}

/** Require a literal generated-project contract. */
function requireText(source, expected, label, failures) {
  if (!source.includes(expected)) failures.push(`${label} must contain ${JSON.stringify(expected)}`);
}

/** Reject a literal that would widen the generated-project contract. */
function rejectText(source, forbidden, label, failures) {
  if (source.includes(forbidden)) failures.push(`${label} must not contain ${JSON.stringify(forbidden)}`);
}

/** Count non-overlapping literal occurrences. */
function countText(source, value) {
  return source.split(value).length - 1;
}

const [
  rootBuild,
  appBuild,
  productionStrings,
  developmentStrings,
  wrapper,
  manifest,
  mainActivity,
  dayTheme,
  nightTheme,
  filePaths,
  backupRules,
  extractionRules,
  launcherIcon,
  calendarNotificationIcon,
  pomodoroNotificationIcon,
  androidConfigSource,
  androidCapability,
  mobileNotificationManifest,
  mobileNotificationPluginRoot,
  rustBuildTask,
  rustPlugin,
  mobileDocumentsBuild,
  mobileMediaBuild,
  mobileNotificationsBuild,
] =
  await Promise.all([
    readAndroidFile("build.gradle.kts"),
    readAndroidFile("app/build.gradle.kts"),
    readAndroidFile("app/src/main/res/values/strings.xml"),
    readAndroidFile("app/src/debug/res/values/strings.xml"),
    readAndroidFile("gradle/wrapper/gradle-wrapper.properties"),
    readAndroidFile("app/src/main/AndroidManifest.xml"),
    readAndroidFile("app/src/main/java/org/opengrimoire/ganbaruai/MainActivity.kt"),
    readAndroidFile("app/src/main/res/values/themes.xml"),
    readAndroidFile("app/src/main/res/values-night/themes.xml"),
    readAndroidFile("app/src/main/res/xml/file_paths.xml"),
    readAndroidFile("app/src/main/res/xml/backup_rules.xml"),
    readAndroidFile("app/src/main/res/xml/data_extraction_rules.xml"),
    readAndroidBytes("app/src/main/res/mipmap-xxxhdpi/ic_launcher.png"),
    readAndroidFile("app/src/main/res/drawable/ic_notification_calendar.xml"),
    readClientFile(
      "../../crates/ganbaru-mobile-notifications/android/src/main/res/drawable/ic_notification_focus.xml",
    ),
    readClientFile("src-tauri/tauri.android.conf.json"),
    readClientFile("src-tauri/capabilities/android.json"),
    readClientFile(
      "../../crates/ganbaru-mobile-notifications/android/src/main/AndroidManifest.xml",
    ),
    readClientFile("../../crates/ganbaru-mobile-notifications/src/lib.rs"),
    readAndroidFile(
      "buildSrc/src/main/java/org/opengrimoire/ganbaruai/kotlin/BuildTask.kt",
    ),
    readAndroidFile(
      "buildSrc/src/main/java/org/opengrimoire/ganbaruai/kotlin/RustPlugin.kt",
    ),
    readClientFile("../../crates/ganbaru-mobile-documents/android/build.gradle.kts"),
    readClientFile("../../crates/ganbaru-mobile-media/android/build.gradle.kts"),
    readClientFile("../../crates/ganbaru-mobile-notifications/android/build.gradle.kts"),
  ]);

const failures = [];
const androidConfig = JSON.parse(androidConfigSource);
const androidCapabilityConfig = JSON.parse(androidCapability);
const androidCapabilityPermissions = androidCapabilityConfig.permissions.filter(
  (permission) => typeof permission === "string",
);

if (androidConfig.plugins?.notification !== undefined) {
  failures.push(
    "Android Tauri config must not configure the notification plugin because its Rust setup expects unit configuration",
  );
}
if (androidCapabilityPermissions.includes("notification:allow-create-channel")) {
  failures.push("Android capability must keep Calendar channel creation behind the native adapter");
}
for (const expected of [
  "notification:allow-is-permission-granted",
  "notification:allow-request-permission",
  "ganbaru-mobile-notifications:allow-calendarChannelStatus",
  "ganbaru-mobile-notifications:allow-scheduleCalendarNotifications",
  "ganbaru-mobile-notifications:allow-pendingCalendarNotifications",
  "ganbaru-mobile-notifications:allow-cancelCalendarNotifications",
  "ganbaru-mobile-notifications:allow-takeCalendarNotificationAction",
  "ganbaru-mobile-notifications:allow-reconcilePomodoroSchedule",
]) {
  if (!androidCapabilityPermissions.includes(expected)) {
    failures.push(`Android capability must include ${JSON.stringify(expected)}`);
  }
}
for (const nativeOnly of [
  "ganbaru-mobile-notifications:allow-updatePomodoroNotification",
  "ganbaru-mobile-notifications:allow-pomodoroNotificationState",
  "ganbaru-mobile-notifications:allow-cancelPomodoroNotification",
  "ganbaru-mobile-notifications:allow-completeFocusNotification",
  "ganbaru-mobile-notifications:allow-attachFocusLifecycle",
]) {
  if (androidCapabilityPermissions.includes(nativeOnly)) {
    failures.push(`Android capability must keep native Focus authority private: ${nativeOnly}`);
  }
}
for (const expected of [
  'const PLUGIN_NAME: &str = "ganbaru-mobile-notifications";',
  "tauri::plugin::Builder::new(PLUGIN_NAME)",
]) {
  requireText(
    mobileNotificationPluginRoot,
    expected,
    "mobile notification runtime plugin identifier",
    failures,
  );
}
for (const expected of [
  "android.permission.SCHEDULE_EXACT_ALARM",
  "android.permission.RECEIVE_BOOT_COMPLETED",
  "android.permission.FOREGROUND_SERVICE",
  "android.permission.FOREGROUND_SERVICE_SPECIAL_USE",
  "android.intent.action.BOOT_COMPLETED",
  "android.intent.action.MY_PACKAGE_REPLACED",
  ".PomodoroNotificationReceiver",
  ".PomodoroReminderReceiver",
  ".PomodoroNotificationService",
  'android:foregroundServiceType="specialUse"',
  'android:stopWithTask="false"',
  "android.app.PROPERTY_SPECIAL_USE_FGS_SUBTYPE",
  'android:exported="false"',
]) {
  requireText(mobileNotificationManifest, expected, "mobile notification manifest", failures);
}
rejectText(
  mobileNotificationManifest,
  'android:exported="true"',
  "mobile notification manifest",
  failures,
);

requireText(rootBuild, 'classpath("com.android.tools.build:gradle:8.11.0")', "root build", failures);
requireText(
  rootBuild,
  'classpath("org.jetbrains.kotlin:kotlin-gradle-plugin:2.2.10")',
  "root build",
  failures,
);
requireText(wrapper, "gradle-8.14.3-bin.zip", "Gradle wrapper", failures);

for (const expected of [
  "abstract val execOperations: ExecOperations",
  "var projectDir: String? = null",
  "execOperations.exec {",
  "workingDir(File(projectDir, rootDirRel))",
]) {
  requireText(rustBuildTask, expected, "Android Rust build task", failures);
}
for (const forbidden of ["project.exec {", "project.logger", "project.projectDir"]) {
  rejectText(rustBuildTask, forbidden, "Android Rust build task", failures);
}
requireText(
  rustPlugin,
  "projectDir = project.projectDir.path",
  "Android Rust Gradle plugin",
  failures,
);

for (const [source, label] of [
  [appBuild, "Android app Kotlin compiler"],
  [mobileDocumentsBuild, "mobile documents Kotlin compiler"],
  [mobileMediaBuild, "mobile media Kotlin compiler"],
  [mobileNotificationsBuild, "mobile notifications Kotlin compiler"],
]) {
  requireText(source, "import org.jetbrains.kotlin.gradle.dsl.JvmTarget", label, failures);
  requireText(source, "jvmTarget = JvmTarget.JVM_17", label, failures);
  rejectText(source, "kotlinOptions", label, failures);
}

for (const [expected, label] of [
  ['buildToolsVersion = "35.0.0"', "SDK Build Tools"],
  ["compileSdk = 36", "compile SDK"],
  ['ndkVersion = "30.0.15729638"', "NDK"],
  ["minSdk = 29", "minimum SDK"],
  ["targetSdk = 36", "target SDK"],
  ['applicationIdSuffix = ".dev"', "debug application ID suffix"],
  ['rootProject.file("keystore.properties")', "release signing properties"],
  ['signingConfig = signingConfigs.findByName("release")', "release signing configuration"],
  ['manifestPlaceholders["usesCleartextTraffic"] = "false"', "production cleartext policy"],
  ["sourceCompatibility = JavaVersion.VERSION_17", "Java source compatibility"],
  ["targetCompatibility = JavaVersion.VERSION_17", "Java target compatibility"],
  ["jvmTarget = JvmTarget.JVM_17", "Kotlin JVM target"],
]) {
  requireText(appBuild, expected, label, failures);
}

for (const expected of [
  '<string name="app_name">Ganbaru AI</string>',
  '<string name="main_activity_title">Ganbaru AI</string>',
]) {
  requireText(productionStrings, expected, "production app identity", failures);
}
for (const expected of [
  '<string name="app_name">Ganbaru AI Dev</string>',
  '<string name="main_activity_title">Ganbaru AI Dev</string>',
]) {
  requireText(developmentStrings, expected, "development app identity", failures);
}

requireText(manifest, '<uses-permission android:name="android.permission.INTERNET" />', "manifest", failures);
requireText(manifest, '<uses-permission android:name="android.permission.CAMERA" />', "manifest", failures);
requireText(manifest, 'android:allowBackup="false"', "manifest", failures);
requireText(manifest, 'android:fullBackupContent="@xml/backup_rules"', "manifest", failures);
requireText(manifest, 'android:dataExtractionRules="@xml/data_extraction_rules"', "manifest", failures);
requireText(manifest, 'android:icon="@mipmap/ic_launcher"', "manifest", failures);
requireText(manifest, 'android:roundIcon="@mipmap/ic_launcher_round"', "manifest", failures);
rejectText(manifest, "LEANBACK_LAUNCHER", "manifest", failures);
rejectText(manifest, "android.software.leanback", "manifest", failures);
if (countText(manifest, "<uses-permission ") !== 2) {
  failures.push("manifest must declare exactly the INTERNET and CAMERA permissions");
}

const launcherIconSha256 = createHash("sha256").update(launcherIcon).digest("hex");
if (launcherIconSha256 !== "158362b7787a594e4bb007281d89bd9f0575e5c583ddbfa9c9398621f4828055") {
  failures.push("xxxhdpi launcher icon must be regenerated from the Ganbaru AI icon manifest");
}

requireText(
  calendarNotificationIcon,
  'android:pathData="M19,4h-1V2h-2v2H8V2H6v2H5',
  "Calendar notification icon",
  failures,
);
requireText(
  pomodoroNotificationIcon,
  'android:pathData="M12,2A10,10 0,1 0,22 12A10,10 0,0 0,12 2',
  "Pomodoro notification icon",
  failures,
);

for (const expected of [
  "enableEdgeToEdge()",
  "WindowInsetsCompat.Type.systemBars() or WindowInsetsCompat.Type.displayCutout()",
  "GanbaruAndroidInsets",
  "GanbaruAndroidAppearance",
  "ganbaru:android-insets",
  "@JavascriptInterface",
  "WebSettingsCompat.setAlgorithmicDarkeningAllowed(webView.settings, false)",
  "isAppearanceLightNavigationBars = lightTheme",
]) {
  requireText(mainActivity, expected, "Android system-bar inset bridge", failures);
}

for (const [theme, label] of [[dayTheme, "day theme"], [nightTheme, "night theme"]]) {
  requireText(theme, '<item name="android:forceDarkAllowed">false</item>', label, failures);
}

requireText(
  filePaths,
  '<external-files-path name="captured_images" path="Pictures/" />',
  "FileProvider paths",
  failures,
);
rejectText(filePaths, "<external-path", "FileProvider paths", failures);
rejectText(filePaths, 'path="."', "FileProvider paths", failures);

const backupDomains = [
  "root",
  "file",
  "database",
  "sharedpref",
  "external",
  "device_root",
  "device_file",
  "device_database",
  "device_sharedpref",
];
for (const domain of backupDomains) {
  const exclusion = `<exclude domain="${domain}" path="." />`;
  if (countText(backupRules, exclusion) !== 1) {
    failures.push(`legacy backup rules must exclude ${domain} exactly once`);
  }
  if (countText(extractionRules, exclusion) !== 2) {
    failures.push(`data extraction rules must exclude ${domain} from cloud and device transfer`);
  }
}

if (failures.length > 0) {
  console.error("Android generated-project contract failed:");
  for (const failure of failures) console.error(`- ${failure}`);
  process.exitCode = 1;
} else {
  console.log("Android generated-project contract passed.");
}
