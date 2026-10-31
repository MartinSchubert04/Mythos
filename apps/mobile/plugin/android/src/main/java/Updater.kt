package dev.mrtn.mythos.plugin

import android.app.Activity
import android.app.PendingIntent
import android.content.BroadcastReceiver
import android.content.Context
import android.content.Intent
import android.content.pm.ApplicationInfo
import android.content.pm.PackageInstaller
import android.net.Uri
import android.os.Build
import android.provider.Settings
import app.tauri.Logger
import org.json.JSONObject
import java.io.File
import java.net.HttpURLConnection
import java.net.URL
import java.security.MessageDigest

/**
 * Actualizaciones desde GitHub Releases (lo mismo que Haze): cada version (tag `vX.Y.Z`) trae
 * el APK y `latest-android.json` (version, url, sha256). El APK es generico: no lleva la
 * direccion del server ni ningun secreto.
 *
 * La firma la controla Android: un APK firmado con otra clave no se instala encima. Aca se
 * verifica que lo bajado sea lo publicado (sha256).
 */
internal object Updater {
  private const val LATEST = "https://github.com/MartinSchubert04/Mythos/releases/latest/download/latest-android.json"

  /** El APK bajado y verificado, listo para instalar. */
  @Volatile
  var ready: File? = null
    private set

  /**
   * De donde sale `latest-android.json`. Solo en builds de debug, para probar sin publicar:
   * `adb shell run-as dev.mrtn.mythos sh -c 'echo http://10.0.2.2:8000/latest-android.json > files/update-url.txt'`.
   */
  private fun latestUrl(context: Context): String {
    if (context.applicationInfo.flags and ApplicationInfo.FLAG_DEBUGGABLE != 0) {
      val f = File(context.filesDir, "update-url.txt")
      if (f.exists()) return f.readText().trim()
    }
    return LATEST
  }

  /** Borra un APK de una sesion anterior (el que ya se instalo pesa lo mismo que la app). */
  fun clean(context: Context) {
    if (ready != null) return
    File(context.cacheDir, "update.apk").delete()
    File(context.cacheDir, "update.apk.part").delete()
  }

  /** El ultimo Release: {version, url, sha256}, o null si no trae APK. Bloquea. */
  fun latest(context: Context): JSONObject? {
    val c = URL(latestUrl(context)).openConnection() as HttpURLConnection
    c.connectTimeout = 15_000
    c.readTimeout = 30_000
    c.setRequestProperty("User-Agent", "Mythos-Android")
    try {
      return when (c.responseCode) {
        200 -> JSONObject(c.inputStream.bufferedReader().use { it.readText() })
        // Sin Releases todavia, o el ultimo salio sin APK (fallo la parte de Android).
        404 -> null
        else -> throw IllegalStateException("GitHub answered ${c.responseCode}")
      }
    } finally {
      c.disconnect()
    }
  }

  /** Baja el APK y lo deja en [ready] si el sha256 coincide. Bloquea. */
  fun download(context: Context, url: String, sha256: String, onProgress: (Float) -> Unit) {
    val dest = File(context.cacheDir, "update.apk")
    val part = File(context.cacheDir, "update.apk.part")
    ready = null
    val c = URL(url).openConnection() as HttpURLConnection
    c.connectTimeout = 15_000
    c.readTimeout = 60_000
    c.setRequestProperty("User-Agent", "Mythos-Android")
    try {
      if (c.responseCode != 200) throw IllegalStateException("GitHub answered ${c.responseCode}")
      val total = c.contentLengthLong
      val digest = MessageDigest.getInstance("SHA-256")
      var done = 0L
      var last = 0f
      c.inputStream.use { input ->
        part.outputStream().use { out ->
          val buf = ByteArray(64 * 1024)
          while (true) {
            val n = input.read(buf)
            if (n < 0) break
            out.write(buf, 0, n)
            digest.update(buf, 0, n)
            done += n
            if (total > 0) {
              val p = done.toFloat() / total
              if (p - last >= 0.02f) {
                last = p
                onProgress(p)
              }
            }
          }
        }
      }
      val hex = digest.digest().joinToString("") { "%02x".format(it) }
      if (!hex.equals(sha256.trim(), ignoreCase = true)) {
        part.delete()
        throw IllegalStateException("The downloaded APK doesn't match the published one")
      }
      dest.delete()
      part.renameTo(dest)
      ready = dest
    } finally {
      c.disconnect()
    }
  }

  /** Si Android deja que Mythos instale APKs ("Instalar apps desconocidas"). */
  fun allowed(context: Context): Boolean =
    Build.VERSION.SDK_INT < 26 || context.packageManager.canRequestPackageInstalls()

  /** Abre el ajuste del sistema para dar ese permiso. */
  fun openPermission(activity: Activity) {
    if (Build.VERSION.SDK_INT >= 26) {
      activity.startActivity(
        Intent(Settings.ACTION_MANAGE_UNKNOWN_APP_SOURCES, Uri.parse("package:${activity.packageName}"))
      )
    }
  }

  /**
   * Si se puede instalar sin preguntar: Android 12 o mas nuevo, con el permiso, y Mythos fue
   * quien instalo la version actual (no se reinstalo a mano).
   */
  fun silent(context: Context): Boolean {
    if (Build.VERSION.SDK_INT < 31 || !allowed(context)) return false
    return try {
      context.packageManager.getInstallSourceInfo(context.packageName).installingPackageName == context.packageName
    } catch (e: Exception) {
      false
    }
  }

  /**
   * Instala [ready]. Android cierra Mythos para reemplazarla; al abrirla, la interfaz vuelve a
   * la misma pantalla. Si hace falta confirmar, lo pide el sistema.
   */
  fun install(context: Context) {
    val apk = ready ?: return
    val installer = context.packageManager.packageInstaller
    val params = PackageInstaller.SessionParams(PackageInstaller.SessionParams.MODE_FULL_INSTALL)
    params.setAppPackageName(context.packageName)
    if (Build.VERSION.SDK_INT >= 31) {
      params.setRequireUserAction(PackageInstaller.SessionParams.USER_ACTION_NOT_REQUIRED)
    }
    val id = installer.createSession(params)
    installer.openSession(id).use { session ->
      session.openWrite("mythos.apk", 0, apk.length()).use { out ->
        apk.inputStream().use { it.copyTo(out) }
        session.fsync(out)
      }
      val intent = Intent(context, InstallResult::class.java)
      val flags = PendingIntent.FLAG_UPDATE_CURRENT or (if (Build.VERSION.SDK_INT >= 31) PendingIntent.FLAG_MUTABLE else 0)
      val pending = PendingIntent.getBroadcast(context, id, intent, flags)
      session.commit(pending.intentSender)
    }
  }
}

/** Lo que responde el instalador. Si pide confirmacion, se muestra el dialogo del sistema. */
class InstallResult : BroadcastReceiver() {
  override fun onReceive(context: Context, intent: Intent) {
    when (val status = intent.getIntExtra(PackageInstaller.EXTRA_STATUS, PackageInstaller.STATUS_FAILURE)) {
      PackageInstaller.STATUS_PENDING_USER_ACTION -> {
        @Suppress("DEPRECATION")
        val confirm = intent.getParcelableExtra<Intent>(Intent.EXTRA_INTENT) ?: return
        confirm.addFlags(Intent.FLAG_ACTIVITY_NEW_TASK)
        try {
          context.startActivity(confirm)
        } catch (e: Exception) {
          // Desde segundo plano Android no deja abrir el dialogo: queda "Update now" en Settings.
          Logger.error("mythos: no se pudo pedir confirmacion para actualizar: ${e.message}")
        }
      }
      PackageInstaller.STATUS_SUCCESS -> {}
      else -> Logger.error("mythos: la actualizacion fallo ($status): ${intent.getStringExtra(PackageInstaller.EXTRA_STATUS_MESSAGE)}")
    }
  }
}
