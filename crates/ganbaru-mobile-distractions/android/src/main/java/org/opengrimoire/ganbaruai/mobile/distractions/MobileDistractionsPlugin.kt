package org.opengrimoire.ganbaruai.mobile.distractions

import android.app.Activity
import android.content.ComponentName
import android.content.Intent
import android.net.Uri
import android.os.Process
import android.provider.Settings
import android.webkit.WebView
import app.tauri.annotation.Command
import app.tauri.annotation.InvokeArg
import app.tauri.annotation.TauriPlugin
import app.tauri.plugin.Invoke
import app.tauri.plugin.JSObject
import app.tauri.plugin.Plugin

@InvokeArg
internal class ApplyRulesArgs {
  lateinit var snapshotJson: String
}

@InvokeArg
internal class AcknowledgeEventsArgs {
  var ids: List<String> = listOf()
}

@InvokeArg
internal class AccountingVaultArgs {
  lateinit var vaultId: String
  var usageOnly: Boolean = false
}

internal fun launchableAppResponse(name: String, packageName: String): Map<String, String> = mapOf(
  "name" to name,
  "packageName" to packageName,
)

internal fun JournalEvent.toResponse(): Map<String, Any?> = mapOf(
  "id" to id,
  "kind" to kind,
  "packageName" to packageName,
  "displayName" to displayName,
  "startedAtMs" to startedAtEpochMs,
  "elapsedSeconds" to elapsedSeconds,
  "localDate" to localDate,
  "occurredAtMs" to occurredAtEpochMs,
  "reason" to reason,
  "ruleId" to ruleId,
  "runId" to runId,
  "phase" to phase,
  "vaultId" to vaultId,
)

@TauriPlugin
class MobileDistractionsPlugin(private val activity: Activity) : Plugin(activity) {
  override fun load(webView: WebView) {
    super.load(webView)
    DistractionsNotificationActionStore.capture(activity, activity.intent)
  }

  override fun onNewIntent(intent: Intent) {
    super.onNewIntent(intent)
    DistractionsNotificationActionStore.capture(activity, intent)
  }

  @Command
  fun accessStatus(invoke: Invoke) {
    invoke.resolve(JSObject().apply {
      put("usageAccess", DistractionsAccess.hasUsageAccess(activity))
      put("accessibility", DistractionsAccess.hasAccessibilityAccess(activity))
    })
  }

  @Command
  fun openUsageAccessSettings(invoke: Invoke) {
    val intents = listOf(
      Intent(Settings.ACTION_USAGE_ACCESS_SETTINGS).apply {
        data = Uri.parse("package:${activity.packageName}")
      },
      Intent(Settings.ACTION_USAGE_ACCESS_SETTINGS),
      applicationDetailsIntent(),
    )
    startFirstAvailable(intents, invoke, "Usage Access")
  }

  @Command
  fun openAccessibilitySettings(invoke: Invoke) {
    startFirstAvailable(
      listOf(
        accessibilityDetailsIntent(),
        Intent(Settings.ACTION_ACCESSIBILITY_SETTINGS),
        applicationDetailsIntent(),
      ),
      invoke,
      "Accessibility",
    )
  }

  @Command
  fun listLaunchableApps(invoke: Invoke) {
    Thread {
      try {
        val manager = activity.packageManager
        val protected = ProtectedPackages.resolve(activity)
        val intent = Intent(Intent.ACTION_MAIN).addCategory(Intent.CATEGORY_LAUNCHER)
        val apps = manager.queryIntentActivities(intent, 0)
          .asSequence()
          .mapNotNull { resolved ->
            val activityInfo = resolved.activityInfo ?: return@mapNotNull null
            val packageName = activityInfo.packageName?.trim().orEmpty()
            if (packageName.isEmpty() || packageName in protected) return@mapNotNull null
            if (activityInfo.applicationInfo?.uid?.let { it < Process.FIRST_APPLICATION_UID } != false) {
              return@mapNotNull null
            }
            val label = resolved.loadLabel(manager).toString().trim().take(120)
              .ifBlank { packageName }
            launchableAppResponse(label, packageName)
          }
          .distinctBy { it.getValue("packageName").lowercase() }
          .sortedWith(compareBy(String.CASE_INSENSITIVE_ORDER) { it.getValue("name") })
          .take(512)
          .toList()
        invoke.resolveObject(apps)
      } catch (error: Exception) {
        invoke.reject(error.message ?: "Failed to list launchable Android apps")
      }
    }.start()
  }

  @Command
  fun applyRules(invoke: Invoke) {
    val args = invoke.parseArgs(ApplyRulesArgs::class.java)
    try {
      DistractionsGuardianClient(activity).applyRules(args.snapshotJson)
      invoke.resolve()
    } catch (error: Exception) {
      invoke.reject(error.message ?: "Invalid mobile Distractions rule snapshot")
    }
  }

  @Command
  fun invalidateRules(invoke: Invoke) {
    try {
      DistractionsGuardianClient(activity.applicationContext).invalidateRules()
      invoke.resolve()
    } catch (error: Exception) {
      invoke.reject(error.message ?: "Failed to invalidate mobile Distractions rules")
    }
  }

  @Command
  fun pendingEvents(invoke: Invoke) {
    val args = invoke.parseArgs(AccountingVaultArgs::class.java)
    Thread {
      try {
        val events = DistractionsGuardianClient(activity).pendingEvents(args.vaultId, args.usageOnly)
        invoke.resolveObject(events.map(JournalEvent::toResponse))
      } catch (error: Exception) {
        invoke.reject(error.message ?: "Failed to read mobile Distractions events")
      }
    }.start()
  }

  @Command
  fun accountingSnapshot(invoke: Invoke) {
    val args = invoke.parseArgs(AccountingVaultArgs::class.java)
    Thread {
      try {
        val encoded = DistractionsGuardianClient(activity.applicationContext).accountingSnapshot(args.vaultId)
        invoke.resolve(JSObject().apply { put("snapshotJson", encoded) })
      } catch (error: Exception) {
        invoke.reject(error.message ?: "Failed to read mobile Distractions accounting")
      }
    }.start()
  }

  @Command
  fun acknowledgeEvents(invoke: Invoke) {
    val args = invoke.parseArgs(AcknowledgeEventsArgs::class.java)
    Thread {
      try {
        DistractionsGuardianClient(activity).acknowledgeEvents(args.ids)
        invoke.resolve()
      } catch (error: Exception) {
        invoke.reject(error.message ?: "Failed to acknowledge mobile Distractions events")
      }
    }.start()
  }

  @Command
  fun takeNotificationAction(invoke: Invoke) {
    invoke.resolve(JSObject().apply {
      put("target", DistractionsNotificationActionStore.take(activity))
    })
  }

  private fun startFirstAvailable(intents: List<Intent>, invoke: Invoke, label: String) {
    var lastError: Exception? = null
    for (intent in intents) {
      if (intent.resolveActivity(activity.packageManager) == null) continue
      try {
        activity.startActivity(intent)
        invoke.resolve()
        return
      } catch (error: Exception) {
        lastError = error
      }
    }
    invoke.reject(lastError?.message ?: "No compatible $label settings screen is available")
  }

  private fun applicationDetailsIntent(): Intent = Intent(
    Settings.ACTION_APPLICATION_DETAILS_SETTINGS,
    Uri.parse("package:${activity.packageName}"),
  )

  private fun accessibilityDetailsIntent(): Intent = Intent(
    ACTION_ACCESSIBILITY_DETAILS_SETTINGS,
  ).putExtra(
    Intent.EXTRA_COMPONENT_NAME,
    ComponentName(activity, DistractionsAccessibilityService::class.java),
  )

  companion object {
    private const val ACTION_ACCESSIBILITY_DETAILS_SETTINGS =
      "android.settings.ACCESSIBILITY_DETAILS_SETTINGS"
  }
}
