package dev.mrtn.mythos

import android.graphics.Color
import android.os.Bundle
import androidx.activity.SystemBarStyle
import androidx.activity.enableEdgeToEdge

class MainActivity : TauriActivity() {
  override fun onCreate(savedInstanceState: Bundle?) {
    // De borde a borde: la interfaz deja lugar a las barras con env(safe-area-inset-*). Arranca
    // con iconos claros (el tema por defecto es oscuro); despues la interfaz los ajusta al tema
    // (comando set_bars del plugin).
    val bars = SystemBarStyle.dark(Color.TRANSPARENT)
    enableEdgeToEdge(bars, bars)
    super.onCreate(savedInstanceState)
  }
}
