import java.util.Properties
import org.jetbrains.kotlin.gradle.dsl.JvmTarget

plugins {
    id("com.android.application")
    id("org.jetbrains.kotlin.android")
    id("rust")
}

val tauriProperties = Properties().apply {
    val propFile = file("tauri.properties")
    if (propFile.exists()) {
        propFile.inputStream().use { load(it) }
    }
}

val releaseSigningPropertiesFile = rootProject.file("keystore.properties")
val releaseSigningProperties = Properties().apply {
    if (releaseSigningPropertiesFile.exists()) {
        releaseSigningPropertiesFile.inputStream().use { load(it) }
    }
}

fun requiredReleaseSigningProperty(name: String): String =
    releaseSigningProperties.getProperty(name)?.takeIf { it.isNotBlank() }
        ?: throw GradleException("Android release signing property '$name' is missing")

val releaseTaskRequested = gradle.startParameter.taskNames.any { taskName ->
    taskName.contains("release", ignoreCase = true)
}

// CI builds release artifacts without secrets and signs them in a separate job.
val unsignedReleaseRequested =
    providers.environmentVariable("GANBARU_ANDROID_UNSIGNED_RELEASE").orNull == "true"

if (releaseTaskRequested && !releaseSigningPropertiesFile.isFile && !unsignedReleaseRequested) {
    throw GradleException(
        "Android release builds require gen/android/keystore.properties or GANBARU_ANDROID_UNSIGNED_RELEASE=true; see docs/operations/release/signing.md",
    )
}

android {
    buildToolsVersion = "35.0.0"
    compileSdk = 36
    ndkVersion = "30.0.15729638"
    namespace = "org.opengrimoire.ganbaru"
    defaultConfig {
        manifestPlaceholders["usesCleartextTraffic"] = "false"
        applicationId = "org.opengrimoire.ganbaru"
        minSdk = 29
        targetSdk = 36
        versionCode = tauriProperties.getProperty("tauri.android.versionCode", "1").toInt()
        versionName = tauriProperties.getProperty("tauri.android.versionName", "1.0")
    }
    signingConfigs {
        if (releaseSigningPropertiesFile.isFile) {
            create("release") {
                keyAlias = requiredReleaseSigningProperty("keyAlias")
                keyPassword = requiredReleaseSigningProperty("keyPassword")
                storeFile = file(requiredReleaseSigningProperty("storeFile"))
                storePassword = requiredReleaseSigningProperty("storePassword")
            }
        }
    }
    buildTypes {
        getByName("debug") {
            applicationIdSuffix = ".dev"
            manifestPlaceholders["usesCleartextTraffic"] = "true"
            isDebuggable = true
            isJniDebuggable = true
            isMinifyEnabled = false
            packaging {
                jniLibs.keepDebugSymbols.add("*/arm64-v8a/*.so")
                jniLibs.keepDebugSymbols.add("*/armeabi-v7a/*.so")
                jniLibs.keepDebugSymbols.add("*/x86/*.so")
                jniLibs.keepDebugSymbols.add("*/x86_64/*.so")
            }
        }
        getByName("release") {
            signingConfig = signingConfigs.findByName("release")
            isMinifyEnabled = true
            proguardFiles(
                *fileTree(".") { include("**/*.pro") }
                    .plus(getDefaultProguardFile("proguard-android-optimize.txt"))
                    .toList().toTypedArray()
            )
        }
    }
    compileOptions {
        sourceCompatibility = JavaVersion.VERSION_17
        targetCompatibility = JavaVersion.VERSION_17
    }
    buildFeatures {
        buildConfig = true
    }
}

kotlin {
    compilerOptions {
        jvmTarget = JvmTarget.JVM_17
    }
}

rust {
    rootDirRel = "../../../"
}

// wry generates RustWebView.kt with Tauri bridge scripts allowed on every origin. Release builds serve only the bundled
// app, so they restrict the scripts to the app origin; debug builds keep every origin for the development server.
val rustWebViewSource =
    layout.projectDirectory.file("src/main/java/org/opengrimoire/ganbaru/generated/RustWebView.kt").asFile
val anyOriginInitScriptRule = "setOf(\"*\")"
val appOriginInitScriptRule = "setOf(\"http://tauri.localhost\", \"https://tauri.localhost\")"

fun registerInitScriptOriginTask(profile: String, rule: String) =
    tasks.register("rustWebViewInitScriptOrigins$profile") {
        val source = rustWebViewSource
        mustRunAfter(tasks.matching { it.name.startsWith("rustBuild") && it.name.endsWith(profile) })
        doLast {
            val text = source.readText()
            val updated = text.replace(anyOriginInitScriptRule, rule).replace(appOriginInitScriptRule, rule)
            if (!updated.contains("addDocumentStartJavaScript(this, script, $rule)")) {
                throw GradleException("Generated RustWebView.kt no longer matches the init script origin rule; review $source")
            }
            if (updated != text) source.writeText(updated)
        }
    }

val releaseInitScriptOrigins = registerInitScriptOriginTask("Release", appOriginInitScriptRule)
val debugInitScriptOrigins = registerInitScriptOriginTask("Debug", anyOriginInitScriptRule)

tasks.matching { it.name.startsWith("compile") && it.name.endsWith("ReleaseKotlin") }.configureEach {
    dependsOn(releaseInitScriptOrigins)
}
tasks.matching { it.name.startsWith("compile") && it.name.endsWith("DebugKotlin") }.configureEach {
    dependsOn(debugInitScriptOrigins)
}

dependencies {
    implementation("androidx.webkit:webkit:1.14.0")
    implementation("androidx.appcompat:appcompat:1.7.1")
    implementation("androidx.activity:activity-ktx:1.10.1")
    implementation("com.google.android.material:material:1.12.0")
    implementation("androidx.lifecycle:lifecycle-process:2.10.0")
    testImplementation("junit:junit:4.13.2")
    androidTestImplementation("androidx.test.ext:junit:1.1.4")
    androidTestImplementation("androidx.test.espresso:espresso-core:3.5.0")
}

apply(from = "tauri.build.gradle.kts")
