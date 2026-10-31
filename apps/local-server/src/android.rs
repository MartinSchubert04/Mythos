//! Lo propio de Android: TLS de la plataforma, logs a logcat y si el aparato puede con el server
//! embebido (TV o poca RAM: no).

use std::ffi::{c_char, c_int, CString};
use std::io;

use tracing::Level;
use tracing_subscriber::fmt::MakeWriter;

/// librqbit usa reqwest con el verificador TLS de la plataforma (trackers `https://`). En Android
/// ese verificador llama al de Java por JNI y hay que darle el contexto de la app antes de usarlo:
/// sin esto, el primer tracker https entraria en panico (y en release, con `panic = "abort"`, se
/// cerraria la app). La parte de Kotlin (`org.rustls:rustls-platform-verifier`) va en Gradle
/// (gen/android/app/build.gradle.kts). Los clientes propios del server no lo usan (raices de
/// Mozilla embebidas, `mythos_media::http_client_builder`).
pub fn init_tls() {
    let r = std::panic::catch_unwind(|| {
        // tao ya dejo en ndk-context la JVM y el contexto de la aplicacion (una referencia global).
        let ctx = ndk_context::android_context();
        // SAFETY: punteros validos durante toda la vida del proceso (los guarda tao).
        let vm = unsafe { jni::JavaVM::from_raw(ctx.vm().cast()) };
        vm.attach_current_thread(|env| -> Result<(), jni::errors::Error> {
            // SAFETY: es una referencia global viva; JObject no la libera al soltarse.
            let context = unsafe { jni::objects::JObject::from_raw(env, ctx.context().cast()) };
            rustls_platform_verifier::android::init_with_env(env, context)
        })
    });
    match r {
        Ok(Ok(())) => {}
        Ok(Err(e)) => tracing::warn!("TLS de la plataforma sin inicializar: {e}"),
        Err(_) => tracing::warn!("TLS de la plataforma sin inicializar (sin contexto de Android)"),
    }
}

/// Por debajo de esto (`MemTotal` de /proc/meminfo) no se arranca el server embebido: librqbit,
/// SQLite y el WebView no entran juntos. Un equipo "de 2 GB" reporta ~1,8 GiB (el kernel y la GPU
/// se quedan con una parte); uno de 1 GB, ~0,9.
const MIN_RAM_MIB: u64 = 1536;

/// `MemTotal` en MiB.
fn mem_total_mib() -> Option<u64> {
    let s = std::fs::read_to_string("/proc/meminfo").ok()?;
    let kb: u64 = s.lines().find_map(|l| l.strip_prefix("MemTotal:"))?.trim().trim_end_matches("kB").trim().parse().ok()?;
    Some(kb / 1024)
}

/// Lo que dice Android del aparato: (es TV, `isLowRamDevice`).
fn device_class() -> Result<(bool, bool), String> {
    use jni::objects::{JObject, JValue};
    use jni::{jni_sig, jni_str};
    let r = std::panic::catch_unwind(|| {
        let ctx = ndk_context::android_context();
        // SAFETY: punteros validos durante toda la vida del proceso (los guarda tao).
        let vm = unsafe { jni::JavaVM::from_raw(ctx.vm().cast()) };
        vm.attach_current_thread(|env| -> Result<(bool, bool), jni::errors::Error> {
            // SAFETY: es una referencia global viva; JObject no la libera al soltarse.
            let context = unsafe { JObject::from_raw(env, ctx.context().cast()) };
            let pm = env
                .call_method(&context, jni_str!("getPackageManager"), jni_sig!("()Landroid/content/pm/PackageManager;"), &[])?
                .l()?;
            let leanback = env.new_string("android.software.leanback")?;
            let tv_feature = env
                .call_method(&pm, jni_str!("hasSystemFeature"), jni_sig!("(Ljava/lang/String;)Z"), &[JValue::Object(&leanback)])?
                .z()?;
            let touchscreen = env.new_string("android.hardware.touchscreen")?;
            let touch = env
                .call_method(&pm, jni_str!("hasSystemFeature"), jni_sig!("(Ljava/lang/String;)Z"), &[JValue::Object(&touchscreen)])?
                .z()?;
            let name = env.new_string("uimode")?;
            let ui = env
                .call_method(&context, jni_str!("getSystemService"), jni_sig!("(Ljava/lang/String;)Ljava/lang/Object;"), &[JValue::Object(&name)])?
                .l()?;
            // Configuration.UI_MODE_TYPE_TELEVISION = 4.
            let tv_mode = !ui.is_null() && env.call_method(&ui, jni_str!("getCurrentModeType"), jni_sig!("()I"), &[])?.i()? == 4;
            let name = env.new_string("activity")?;
            let am = env
                .call_method(&context, jni_str!("getSystemService"), jni_sig!("(Ljava/lang/String;)Ljava/lang/Object;"), &[JValue::Object(&name)])?
                .l()?;
            let low_ram = !am.is_null() && env.call_method(&am, jni_str!("isLowRamDevice"), jni_sig!("()Z"), &[])?.z()?;
            // Igual que el plugin de la interfaz (MythosPlugin.kt): sin pantalla tactil tambien es TV.
            Ok((tv_mode || tv_feature || !touch, low_ram))
        })
    });
    match r {
        Ok(Ok(v)) => Ok(v),
        Ok(Err(e)) => Err(e.to_string()),
        Err(_) => Err("sin contexto de Android".into()),
    }
}

/// Por que este aparato no corre el server embebido, si no puede: `"tv"` o `"low_ram"`. Un TV box
/// (casi siempre 1-2 GB y un procesador chico) no da para bajar y servir torrents ahi mismo.
pub fn limit() -> Option<&'static str> {
    let ram = mem_total_mib();
    let (tv, low_ram) = device_class().unwrap_or_else(|e| {
        tracing::warn!("no se pudo saber que aparato es: {e}");
        (false, false)
    });
    tracing::info!("aparato: tv {tv}, low_ram {low_ram}, MemTotal {ram:?} MiB");
    if tv {
        Some("tv")
    } else if low_ram || ram.is_some_and(|m| m < MIN_RAM_MIB) {
        Some("low_ram")
    } else {
        None
    }
}

#[link(name = "log")]
extern "C" {
    fn __android_log_write(prio: c_int, tag: *const c_char, text: *const c_char) -> c_int;
}

/// `tracing` → logcat (etiqueta `mythos`), una linea por evento.
pub struct Logcat;

pub struct Line {
    prio: c_int,
    buf: Vec<u8>,
}

impl<'a> MakeWriter<'a> for Logcat {
    type Writer = Line;

    fn make_writer(&'a self) -> Line {
        Line { prio: 4, buf: Vec::new() }
    }

    fn make_writer_for(&'a self, meta: &tracing::Metadata<'_>) -> Line {
        // android/log.h: VERBOSE 2, DEBUG 3, INFO 4, WARN 5, ERROR 6.
        let prio = match *meta.level() {
            Level::TRACE => 2,
            Level::DEBUG => 3,
            Level::INFO => 4,
            Level::WARN => 5,
            Level::ERROR => 6,
        };
        Line { prio, buf: Vec::new() }
    }
}

impl io::Write for Line {
    fn write(&mut self, b: &[u8]) -> io::Result<usize> {
        self.buf.extend_from_slice(b);
        Ok(b.len())
    }

    fn flush(&mut self) -> io::Result<()> {
        Ok(())
    }
}

impl Drop for Line {
    fn drop(&mut self) {
        while self.buf.last() == Some(&b'\n') {
            self.buf.pop();
        }
        self.buf.retain(|&c| c != 0);
        if self.buf.is_empty() {
            return;
        }
        if let Ok(text) = CString::new(std::mem::take(&mut self.buf)) {
            // SAFETY: cadenas C validas terminadas en 0.
            unsafe { __android_log_write(self.prio, c"mythos".as_ptr(), text.as_ptr()) };
        }
    }
}
