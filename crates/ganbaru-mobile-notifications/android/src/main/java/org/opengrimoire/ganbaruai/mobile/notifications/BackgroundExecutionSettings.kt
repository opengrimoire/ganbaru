package org.opengrimoire.ganbaruai.mobile.notifications

import java.util.Locale

internal sealed class BackgroundSettingsIntentSpec {
  data class Component(
    val packageName: String,
    val className: String,
    val dataUri: String? = null,
    val includeApplicationExtras: Boolean = false,
  ) : BackgroundSettingsIntentSpec()

  data class Action(
    val action: String,
    val packageName: String? = null,
    val integerExtras: Map<String, Int> = emptyMap(),
  ) : BackgroundSettingsIntentSpec()
}

internal enum class BackgroundSettingsDestination {
  AUTOSTART,
  BATTERY,
}

internal object BackgroundExecutionSettingsRegistry {
  fun candidates(
    destination: BackgroundSettingsDestination,
    manufacturer: String,
    brand: String,
  ): List<BackgroundSettingsIntentSpec> {
    val identifiers = setOf(normalize(manufacturer), normalize(brand)).filter(String::isNotEmpty)
    return when (destination) {
      BackgroundSettingsDestination.AUTOSTART -> autostartCandidates(identifiers)
      BackgroundSettingsDestination.BATTERY -> batteryCandidates(identifiers)
    }
  }

  private fun autostartCandidates(identifiers: List<String>): List<BackgroundSettingsIntentSpec> =
    when {
      identifiers.matches("xiaomi", "redmi", "poco", "blackshark") -> listOf(
        component(
          "com.miui.securitycenter",
          "com.miui.permcenter.autostart.AutoStartManagementActivity",
        ),
      )
      identifiers.matches("huawei", "honor") -> listOf(
        component(
          "com.huawei.systemmanager",
          "com.huawei.systemmanager.startupmgr.ui.StartupNormalAppListActivity",
        ),
        component(
          "com.huawei.systemmanager",
          "com.huawei.systemmanager.appcontrol.activity.StartupAppControlActivity",
        ),
        component(
          "com.huawei.systemmanager",
          "com.huawei.systemmanager.optimize.process.ProtectActivity",
        ),
        component(
          "com.hihonor.systemmanager",
          "com.hihonor.systemmanager.startupmgr.ui.StartupNormalAppListActivity",
        ),
        component(
          "com.hihonor.systemmanager",
          "com.hihonor.systemmanager.appcontrol.activity.StartupAppControlActivity",
        ),
        component(
          "com.hihonor.systemmanager",
          "com.hihonor.systemmanager.optimize.process.ProtectActivity",
        ),
      )
      identifiers.matches("oppo", "realme", "oneplus", "oplus") -> listOf(
        component(
          "com.oplus.safe",
          "com.oplus.safe.permission.startup.StartupAppListActivity",
        ),
        component(
          "com.oplus.safecenter",
          "com.oplus.safecenter.startupapp.StartupAppListActivity",
        ),
        component(
          "com.coloros.safecenter",
          "com.coloros.safecenter.permission.startup.StartupAppListActivity",
        ),
        component(
          "com.coloros.safecenter",
          "com.coloros.safecenter.startupapp.StartupAppListActivity",
        ),
        component(
          "com.oppo.safe",
          "com.oppo.safe.permission.startup.StartupAppListActivity",
        ),
        component(
          "com.oneplus.security",
          "com.oneplus.security.chainlaunch.view.ChainLaunchAppListActivity",
        ),
        BackgroundSettingsIntentSpec.Action("com.android.settings.action.BACKGROUND_OPTIMIZE"),
      )
      identifiers.matches("vivo", "iqoo") -> listOf(
        component(
          "com.vivo.permissionmanager",
          "com.vivo.permissionmanager.activity.BgStartUpManagerActivity",
        ),
        component(
          "com.iqoo.secure",
          "com.iqoo.secure.ui.phoneoptimize.BgStartUpManager",
        ),
        component(
          "com.iqoo.secure",
          "com.iqoo.secure.ui.phoneoptimize.AddWhiteListActivity",
        ),
      )
      identifiers.matches("asus") -> listOf(
        component(
          "com.asus.mobilemanager",
          "com.asus.mobilemanager.autostart.AutoStartActivity",
        ),
        component(
          "com.asus.mobilemanager",
          "com.asus.mobilemanager.entry.FunctionActivity",
          dataUri = "mobilemanager://function/entry/AutoStart",
        ),
      )
      identifiers.matches("infinix", "tecno", "itel", "transsion") -> listOf(
        component(
          "com.transsion.phonemaster",
          "com.cyin.himgr.autostart.AutoStartActivity",
        ),
      )
      identifiers.matches("zte", "nubia", "redmagic") -> listOf(
        component(
          "com.zte.heartyservice",
          "com.zte.heartyservice.setting.BackgroundAppManagementActivity",
        ),
        component(
          "com.zte.heartyservice",
          "com.zte.heartyservice.setting.AppBackgroundManagementActivity",
        ),
      )
      identifiers.matches("lenovo") -> listOf(
        component(
          "com.lenovo.powersetting",
          "com.lenovo.powersetting.ui.Settings\$HighPowerApplicationsActivity",
        ),
      )
      identifiers.matches("meizu") -> listOf(
        component("com.meizu.safe", "com.meizu.safe.permission.SmartBGActivity"),
        component("com.meizu.safe", "com.meizu.safe.permission.PermissionAppActivity"),
      )
      identifiers.matches("letv", "leeco") -> listOf(
        component(
          "com.letv.android.letvsafe",
          "com.letv.android.letvsafe.AutobootManageActivity",
        ),
        BackgroundSettingsIntentSpec.Action("com.letv.android.permissionautoboot"),
      )
      else -> emptyList()
    }

  private fun batteryCandidates(identifiers: List<String>): List<BackgroundSettingsIntentSpec> =
    when {
      identifiers.matches("xiaomi", "redmi", "poco", "blackshark") -> listOf(
        component(
          "com.miui.powerkeeper",
          "com.miui.powerkeeper.ui.HiddenAppsConfigActivity",
          includeApplicationExtras = true,
        ),
        component("com.miui.securitycenter", "com.miui.powercenter.PowerSettings"),
      )
      identifiers.matches("samsung") -> listOf(
        BackgroundSettingsIntentSpec.Action(
          action = "com.samsung.android.sm.ACTION_OPEN_CHECKABLE_LISTACTIVITY",
          packageName = "com.samsung.android.lool",
          integerExtras = mapOf("activity_type" to 2),
        ),
        component(
          "com.samsung.android.lool",
          "com.samsung.android.sm.battery.ui.BatteryActivity",
        ),
        component(
          "com.samsung.android.sm",
          "com.samsung.android.sm.ui.battery.BatteryActivity",
        ),
      )
      identifiers.matches("oppo", "realme", "oneplus", "oplus") -> listOf(
        component(
          "com.oplus.battery",
          "com.oplus.powermanager.fuelgaue.PowerConsumptionActivity",
        ),
        component(
          "com.coloros.oppoguardelf",
          "com.coloros.powermanager.fuelgaue.PowerConsumptionActivity",
        ),
      )
      identifiers.matches("huawei", "honor") -> listOf(
        component(
          "com.huawei.systemmanager",
          "com.huawei.systemmanager.power.ui.HwPowerManagerActivity",
        ),
      )
      identifiers.matches("vivo", "iqoo") -> listOf(
        component(
          "com.iqoo.powersaving",
          "com.iqoo.powersaving.PowerSavingManagerActivity",
        ),
      )
      identifiers.matches("asus") -> listOf(
        component(
          "com.asus.mobilemanager",
          "com.asus.mobilemanager.powersaver.PowerSaverSettings",
        ),
      )
      identifiers.matches("nokia", "hmd") -> listOf(
        component(
          "com.evenwell.powersaving.g3",
          "com.evenwell.powersaving.g3.exception.PowerSaverExceptionActivity",
        ),
      )
      else -> emptyList()
    }

  private fun List<String>.matches(vararg aliases: String): Boolean =
    any { identifier -> identifier in aliases }

  private fun normalize(value: String): String = value.trim().lowercase(Locale.ROOT)

  private fun component(
    packageName: String,
    className: String,
    dataUri: String? = null,
    includeApplicationExtras: Boolean = false,
  ): BackgroundSettingsIntentSpec.Component = BackgroundSettingsIntentSpec.Component(
    packageName = packageName,
    className = className,
    dataUri = dataUri,
    includeApplicationExtras = includeApplicationExtras,
  )
}
