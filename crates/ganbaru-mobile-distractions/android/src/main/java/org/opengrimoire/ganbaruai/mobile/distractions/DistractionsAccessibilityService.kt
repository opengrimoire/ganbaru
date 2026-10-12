package org.opengrimoire.ganbaruai.mobile.distractions

import android.annotation.SuppressLint
import android.accessibilityservice.AccessibilityService
import android.content.BroadcastReceiver
import android.content.Context
import android.content.Intent
import android.content.IntentFilter
import android.os.Build
import android.os.Handler
import android.os.Looper
import android.util.Log
import android.view.accessibility.AccessibilityEvent
import java.util.concurrent.ExecutorService
import java.util.concurrent.Executors
import java.util.concurrent.RejectedExecutionException

class DistractionsAccessibilityService : AccessibilityService() {
  private lateinit var engine: DistractionsEngine
  private var receiverRegistered = false
  private var destroyed = false
  private val handler = Handler(Looper.getMainLooper())
  private val engineExecutor: ExecutorService = Executors.newSingleThreadExecutor()
  private val checkpoint = object : Runnable {
    override fun run() {
      if (destroyed) return
      submitEngineCheck("periodic checkpoint") {
        engine.onCheckpoint(System.currentTimeMillis())
      }
      handler.postDelayed(this, CHECKPOINT_INTERVAL_MS)
    }
  }
  private val runtimeReceiver = object : BroadcastReceiver() {
    override fun onReceive(context: Context, intent: Intent) {
      val runtimeChanged = intent.action == ACTION_DISTRACTIONS_RULES_CHANGED
        || failOpen("runtime projection update") {
          DistractionsRuntimeStore.updatePhaseFromIntent(context, intent)
        }
      if (runtimeChanged) {
        submitEngineCheck("runtime-change checkpoint") {
          engine.onCheckpoint(System.currentTimeMillis())
        }
      }
    }
  }
  private val screenOffReceiver = object : BroadcastReceiver() {
    override fun onReceive(context: Context, intent: Intent) {
      if (intent.action != Intent.ACTION_SCREEN_OFF) return
      submitEngineCheck("screen-off checkpoint") {
        engine.onCheckpoint(System.currentTimeMillis())
      }
    }
  }

  override fun onServiceConnected() {
    super.onServiceConnected()
    engine = DistractionsEngine(applicationContext)
    submitEngineCheck("startup reconciliation") {
      engine.onCheckpoint(System.currentTimeMillis())
    }
    val filter = IntentFilter().apply {
      addAction(ACTION_DISTRACTIONS_PHASE)
      addAction(ACTION_DISTRACTIONS_PHASE_CLEAR)
      addAction(ACTION_DISTRACTIONS_RULES_CHANGED)
    }
    if (!receiverRegistered) {
      failOpen("runtime receiver registration") {
        registerRuntimeReceivers(filter)
        receiverRegistered = true
        true
      }
    }
    handler.removeCallbacks(checkpoint)
    handler.postDelayed(checkpoint, CHECKPOINT_INTERVAL_MS)
  }

  override fun onAccessibilityEvent(event: AccessibilityEvent?) {
    val packageName = event?.packageName?.toString()?.trim().orEmpty()
    if (packageName.isEmpty()) return
    submitEngineCheck("foreground-package evaluation") {
      engine.onAccessibilityPackage(packageName, System.currentTimeMillis())
    }
  }

  override fun onInterrupt() {
    submitEngineCheck("service interruption checkpoint") {
      engine.onCheckpoint(System.currentTimeMillis())
    }
  }

  override fun onDestroy() {
    destroyed = true
    handler.removeCallbacks(checkpoint)
    if (receiverRegistered) {
      unregisterIfRegistered(runtimeReceiver)
      unregisterIfRegistered(screenOffReceiver)
      receiverRegistered = false
    }
    if (::engine.isInitialized) {
      try {
        engineExecutor.execute {
          synchronized(DISTRACTIONS_GUARDIAN_LOCK) {
            failOpen("service destruction checkpoint") {
              engine.onCheckpoint(System.currentTimeMillis())
            }
          }
        }
      } catch (_: RejectedExecutionException) {
        Unit
      }
    }
    engineExecutor.shutdown()
    super.onDestroy()
  }

  @SuppressLint("UnspecifiedRegisterReceiverFlag")
  private fun registerRuntimeReceivers(filter: IntentFilter) {
    val permission = guardianPermission(this)
    if (Build.VERSION.SDK_INT >= Build.VERSION_CODES.TIRAMISU) {
      registerReceiver(
        runtimeReceiver,
        filter,
        permission,
        null,
        Context.RECEIVER_NOT_EXPORTED,
      )
    } else {
      registerReceiver(runtimeReceiver, filter, permission, null)
    }
    try {
      registerReceiver(screenOffReceiver, IntentFilter(Intent.ACTION_SCREEN_OFF))
    } catch (error: Exception) {
      unregisterIfRegistered(runtimeReceiver)
      throw error
    }
  }

  private fun unregisterIfRegistered(receiver: BroadcastReceiver) {
    try {
      unregisterReceiver(receiver)
    } catch (_: IllegalArgumentException) {
      Unit
    }
  }

  private fun submitEngineCheck(operation: String, check: () -> Boolean) {
    if (destroyed || !::engine.isInitialized) return
    try {
      engineExecutor.execute {
        val blocked = synchronized(DISTRACTIONS_GUARDIAN_LOCK) {
          failOpen(operation, check)
        }
        if (!blocked) return@execute
        handler.post {
          if (destroyed) return@post
          failOpen("Home enforcement") {
            performGlobalAction(GLOBAL_ACTION_HOME)
          }
        }
      }
    } catch (error: RejectedExecutionException) {
      if (!destroyed) Log.e(LOG_TAG, "Distractions worker rejected $operation", error)
    }
  }

  private fun failOpen(operation: String, action: () -> Boolean): Boolean = try {
    action()
  } catch (error: Exception) {
    Log.e(LOG_TAG, "Distractions failed during $operation", error)
    false
  }

  companion object {
    private const val LOG_TAG = "GanbaruDistractions"
    private const val CHECKPOINT_INTERVAL_MS = 30_000L
  }
}
