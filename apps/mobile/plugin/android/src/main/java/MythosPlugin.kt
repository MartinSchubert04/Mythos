package dev.mrtn.mythos.plugin

import android.app.Activity
import android.os.Build
import android.content.pm.ActivityInfo
import android.view.WindowManager
import android.webkit.WebView
import androidx.core.view.WindowCompat
import androidx.core.view.WindowInsetsCompat
import androidx.core.view.WindowInsetsControllerCompat
import app.tauri.Logger
import app.tauri.PermissionState
import app.tauri.annotation.Command
import app.tauri.annotation.InvokeArg
import app.tauri.annotation.Permission
import app.tauri.annotation.PermissionCallback
import app.tauri.annotation.TauriPlugin
import app.tauri.plugin.Channel
import app.tauri.plugin.Invoke
import app.tauri.plugin.JSObject
import app.tauri.plugin.Plugin
import java.util.concurrent.Executors

/** `Manifest.permission.ACCESS_LOCAL_NETWORK`, que aparece recien en el SDK 37. */
private const val LOCAL_NETWORK = "android.permission.ACCESS_LOCAL_NETWORK"

@InvokeArg
class OnArgs {
  var on: Boolean = false
}

@InvokeArg
class PlayingArgs {
  var playing: Boolean = false
}

@InvokeArg
class BarsArgs {
  var light: Boolean = false
}

@InvokeArg
class UpdateArgs {
  lateinit var url: String
  lateinit var sha256: String
  lateinit var channel: Channel
}

/**
 * Lo que Rust le pide a Android (src/android.rs). Solo lo llama Rust, nunca la interfaz
 * directo. Los comandos llegan en el hilo principal.
 */
@TauriPlugin(permissions = [Permission(strings = [LOCAL_NETWORK], alias = "network")])
class MythosPlugin(private val activity: Activity) : Plugin(activity) {
  private val context get() = activity.applicationContext

  /** Red y disco (actualizaciones): fuera del hilo principal. */
  private val io = Executors.newSingleThreadExecutor()

  /** Hay un video reproduciendose (lo avisa la interfaz). */
  @Volatile
  private var playing = false

  private var fullscreen = false

  /** Lo bajado en una sesion anterior ya se instalo (o quedo viejo): se borra. */
  override fun load(webView: WebView) {
    io.execute { Updater.clean(context) }
  }

  private fun bars() = WindowCompat.getInsetsController(activity.window, activity.window.decorView)

  private fun applyFullscreen() {
    val c = bars()
    if (fullscreen) {
      // Las barras vuelven un momento al deslizar desde el borde y se van solas.
      c.systemBarsBehavior = WindowInsetsControllerCompat.BEHAVIOR_SHOW_TRANSIENT_BARS_BY_SWIPE
      c.hide(WindowInsetsCompat.Type.systemBars())
      // Horizontal, girando entre las dos (como cualquier reproductor).
      activity.requestedOrientation = ActivityInfo.SCREEN_ORIENTATION_SENSOR_LANDSCAPE
    } else {
      c.show(WindowInsetsCompat.Type.systemBars())
      activity.requestedOrientation = ActivityInfo.SCREEN_ORIENTATION_UNSPECIFIED
    }
  }

  @Command
  fun setFullscreen(invoke: Invoke) {
    fullscreen = invoke.parseArgs(OnArgs::class.java).on
    activity.runOnUiThread {
      applyFullscreen()
      invoke.resolve()
    }
  }

  /** Mientras se reproduce la pantalla no se apaga, y no se instala ninguna actualizacion. */
  @Command
  fun setPlaying(invoke: Invoke) {
    playing = invoke.parseArgs(PlayingArgs::class.java).playing
    activity.runOnUiThread {
      if (playing) {
        activity.window.addFlags(WindowManager.LayoutParams.FLAG_KEEP_SCREEN_ON)
      } else {
        activity.window.clearFlags(WindowManager.LayoutParams.FLAG_KEEP_SCREEN_ON)
      }
      invoke.resolve()
    }
  }

  /** Iconos oscuros sobre temas claros, claros sobre temas oscuros. */
  @Command
  fun setBars(invoke: Invoke) {
    val light = invoke.parseArgs(BarsArgs::class.java).light
    activity.runOnUiThread {
      val c = bars()
      c.isAppearanceLightStatusBars = light
      c.isAppearanceLightNavigationBars = light
      invoke.resolve()
    }
  }

  /** "Atras" en el inicio sin nada abierto: al fondo, como las demas apps, sin cerrarse. */
  @Command
  fun minimize(invoke: Invoke) {
    activity.moveTaskToBack(true)
    invoke.resolve()
  }

  /**
   * Android 17 corta las conexiones a la red local (un server con IP de la casa, o el emulador
   * hablando con la PC) si no esta este permiso. Antes no existia: se da por concedido.
   */
  private fun networkGranted() = Build.VERSION.SDK_INT < 37 || getPermissionState("network") == PermissionState.GRANTED

  @Command
  fun requestNetwork(invoke: Invoke) {
    if (networkGranted()) resolveNetwork(invoke) else requestPermissionForAlias("network", invoke, "networkResult")
  }

  @PermissionCallback
  fun networkResult(invoke: Invoke) {
    resolveNetwork(invoke)
  }

  private fun resolveNetwork(invoke: Invoke) {
    val result = JSObject()
    result.put("granted", networkGranted())
    invoke.resolve(result)
  }

  /** Al volver a la app el sistema puede haber mostrado las barras: se esconden de nuevo. */
  override fun onResume() {
    if (fullscreen) activity.runOnUiThread { applyFullscreen() }
  }

  // ------------------------------------------------------------------ actualizaciones

  /** El ultimo Release de Android en GitHub: {version, url, sha256} o {} si no hay. */
  @Command
  fun updateLatest(invoke: Invoke) {
    io.execute {
      try {
        val latest = Updater.latest(context)
        invoke.resolve(if (latest == null) JSObject() else JSObject(latest.toString()))
      } catch (e: Throwable) {
        invoke.reject(e.message ?: "Couldn't check GitHub")
      }
    }
  }

  /** Baja y verifica el APK. El avance va por el canal como {progress}. */
  @Command
  fun updateDownload(invoke: Invoke) {
    val a = invoke.parseArgs(UpdateArgs::class.java)
    io.execute {
      try {
        Updater.download(context, a.url, a.sha256) { p ->
          val o = JSObject()
          o.put("progress", p.toDouble())
          a.channel.send(o)
        }
        invoke.resolve()
      } catch (e: Throwable) {
        invoke.reject(e.message ?: "Couldn't download the update")
      }
    }
  }

  /**
   * Instala lo bajado. Sin el permiso "Instalar apps desconocidas" abre ese ajuste y responde
   * `needsPermission`: al volver, se toca "Update now" de nuevo.
   */
  @Command
  fun updateInstall(invoke: Invoke) {
    val result = JSObject()
    if (!Updater.allowed(context)) {
      Updater.openPermission(activity)
      result.put("needsPermission", true)
      invoke.resolve(result)
      return
    }
    io.execute {
      try {
        Updater.install(context)
        result.put("needsPermission", false)
        invoke.resolve(result)
      } catch (e: Throwable) {
        invoke.reject(e.message ?: "Couldn't install the update")
      }
    }
  }

  /**
   * Al salir de la app sin nada reproduciendose se instala lo que haya listo, si Android deja
   * hacerlo sin preguntar. Si no, queda "Update now" en Settings. Al volver a abrirla, la
   * interfaz vuelve a la misma pantalla (y el video al mismo segundo).
   */
  override fun onStop() {
    if (Updater.ready != null && !playing && Updater.silent(context)) {
      io.execute {
        try {
          Updater.install(context)
        } catch (e: Throwable) {
          Logger.error("mythos: no se pudo instalar la actualizacion: ${e.message}")
        }
      }
    }
  }
}
