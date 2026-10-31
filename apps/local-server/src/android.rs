//! Lo propio de Android: TLS de la plataforma y logs a logcat.

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
