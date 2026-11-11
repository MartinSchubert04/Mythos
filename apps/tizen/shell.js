// La pagina local de la app de Samsung (docs/tizen.md). Elige el server, lo guarda, comprueba que
// sea un Mythos y navega a la interfaz que sirve el server (en modo TV, la detecta por el user agent).
// ES5 a proposito: en una tele vieja tiene que poder decir que no alcanza en vez de quedar en negro.
//
// - Primera vez: el campo para la direccion (el teclado de la tele se abre con el centro del
//   control) y "Buscar en esta red", que busca un Mythos en :7700 en la red de la tele.
// - Despues: "Conectando a ..." con "Cambiar de servidor" un momento, y a la interfaz.
// - Al volver aca desde la interfaz (atras en el inicio, si esa pagina no puede cerrar la app), se
//   cierra la app.
// - Idioma: español, o ingles si la tele esta en ingles (navigator.language). Los textos fijos de
//   index.html llevan data-t y estan en español; los de aca salen de TXT.
(function () {
  var KEY = "mythos.tizen.server";
  // Lo que pide la interfaz (apps/web/index.html): color-mix, :has, AbortSignal.timeout.
  var MIN_CHROME = 111;
  // Las teclas multimedia del control solo llegan a la pagina si se registran.
  var MEDIA_KEYS = ["MediaPlayPause", "MediaPlay", "MediaPause", "MediaStop", "MediaFastForward", "MediaRewind"];
  var BACK = 10009;
  var IME_DONE = 65376;
  var IME_CANCEL = 65385;
  var PORT = 7700;

  var LANG = /^en/i.test(navigator.language || "") ? "en" : "es";
  var TXT = {
    es: {
      connecting: "Conectando…",
      connectingTo: "Conectando a {0}…",
      change: "Cambiar de servidor",
      setupLead: "Escribí la dirección de tu servidor de Mythos en la red de tu casa.",
      addrLabel: "Dirección del servidor",
      addrHint: "Alcanza con la IP: sin <b>http://</b> usa <b>http://</b>, y sin puerto, <b>:7700</b>. La tele no puede usar Tailscale: usá la dirección del servidor en tu casa.",
      connect: "Conectar",
      find: "Buscar en esta red",
      searching: "Buscando…",
      local: "{0} es el servidor que trae una app de Mythos, no tu servidor.",
      unreachable: "No hay un servidor de Mythos en {0}. Revisá la dirección y que el servidor esté andando.",
      notAddress: "Eso no parece una dirección. Por ejemplo: http://192.168.0.20:7700",
      noNetwork: "No se puede saber la red de esta tele. Escribí la dirección a mano.",
      notFound: "No se encontró un servidor de Mythos en {0}. Escribí la dirección a mano.",
      old: "El motor web de esta tele (Chromium {0}) es demasiado viejo para Mythos, que necesita Chromium {1} o más nuevo (teles Samsung de 2024 en adelante). Usá Mythos en el navegador de otro dispositivo."
    },
    en: {
      connecting: "Connecting…",
      connectingTo: "Connecting to {0}…",
      change: "Change server",
      setupLead: "Enter the address of your Mythos server on your home network.",
      addrLabel: "Server address",
      addrHint: "The IP is enough: without <b>http://</b> it uses <b>http://</b>, and without a port, <b>:7700</b>. The TV can't use Tailscale: use the server's address at home.",
      connect: "Connect",
      find: "Find on this network",
      searching: "Searching…",
      local: "{0} is the server inside a Mythos app, not your server.",
      unreachable: "Can't reach a Mythos server at {0}. Check the address and that the server is running.",
      notAddress: "That doesn't look like an address. Example: http://192.168.0.20:7700",
      noNetwork: "Can't tell this TV's network. Enter the address by hand.",
      notFound: "No Mythos server found on {0}. Enter the address by hand.",
      old: "This TV's web engine (Chromium {0}) is too old for Mythos, which needs Chromium {1} or newer (Samsung TVs from 2024 on). Use Mythos in a browser on another device instead."
    }
  };
  /** El texto `k` en el idioma de la tele, con {0} y {1} reemplazados. */
  function tr(k, a, b) {
    return String(TXT[LANG][k]).replace("{0}", a === undefined ? "" : a).replace("{1}", b === undefined ? "" : b);
  }
  document.documentElement.lang = LANG;
  if (LANG !== "es") {
    var fixed = document.querySelectorAll("[data-t]");
    for (var ti = 0; ti < fixed.length; ti++) fixed[ti].innerHTML = tr(fixed[ti].getAttribute("data-t"));
  }

  var $ = function (id) { return document.getElementById(id); };
  var hasTizen = typeof tizen !== "undefined" && !!tizen.application;
  var cancelled = false;
  var fromChange = false;

  function exitApp() {
    try {
      tizen.application.getCurrentApplication().exit();
    } catch (e) {
      window.close();
    }
  }

  // Volvio aca desde la interfaz del server (history.back en el inicio): salir.
  function returned() {
    if (location.hash !== "#away") return false;
    history.replaceState(null, "", location.pathname);
    exitApp();
    return true;
  }
  window.addEventListener("pageshow", function (e) {
    if (e.persisted) returned();
  });
  if (returned()) return;

  function registerKeys() {
    if (!hasTizen || !tizen.tvinputdevice) return;
    var supported = {};
    try {
      var list = tizen.tvinputdevice.getSupportedKeys();
      for (var i = 0; i < list.length; i++) supported[list[i].name] = true;
    } catch (e) {}
    for (var j = 0; j < MEDIA_KEYS.length; j++) {
      if (!supported[MEDIA_KEYS[j]]) continue;
      try {
        tizen.tvinputdevice.registerKey(MEDIA_KEYS[j]);
      } catch (e) {}
    }
  }

  function read() {
    try { return localStorage.getItem(KEY) || ""; } catch (e) { return ""; }
  }
  function save(url) {
    try { localStorage.setItem(KEY, url); } catch (e) {}
  }

  /** "192.168.0.5" → "http://192.168.0.5:7700". Sin http(s):// va http://; sin puerto, :7700. */
  function normalize(s) {
    var t = String(s || "").replace(/^\s+|\s+$/g, "").replace(/\/+$/, "");
    if (!t) return "";
    if (!/^https?:\/\//i.test(t)) t = "http://" + t;
    if (!/:\d+$/.test(t.replace(/^https?:\/\//i, ""))) t += ":" + PORT;
    return t;
  }
  var isAddress = function (u) { return /^https?:\/\/[^\/\s]+$/i.test(u); };
  var host = function (u) { return u.replace(/^https?:\/\//i, ""); };

  /** El icono de la interfaz: si carga, ahi hay algo que sirve /icon.svg (la interfaz de Mythos).
   *  No depende de CORS (sirve aunque la pagina local no pueda leer /api/health). */
  function probeIcon(url, ms, done) {
    var img = new Image();
    var end = false;
    var finish = function (ok) {
      if (end) return;
      end = true;
      img.onload = img.onerror = null;
      img.src = "";
      done(ok);
    };
    var t = setTimeout(function () { finish(false); }, ms);
    img.onload = function () { clearTimeout(t); finish(true); };
    img.onerror = function () { clearTimeout(t); finish(false); };
    img.src = url + "/icon.svg?probe=" + Date.now();
  }

  /** Para la busqueda en la red: /api/health con fetch cancelable. Con <img> no sirve: cortar la
   *  imagen no cierra la conexion, y las de direcciones sin nadie llenan las conexiones del motor
   *  hasta que ninguna prueba llega (pasaba en la UN50DU7000). Sin AbortController, el icono. */
  function probeHealth(url, ms, done) {
    if (!window.fetch || !window.AbortController) return probeIcon(url, ms, done);
    var c = new AbortController();
    var t = setTimeout(function () { c.abort(); }, ms);
    fetch(url + "/api/health", { signal: c.signal })
      .then(function (r) { return r.ok ? r.json() : null; })
      .then(function (j) { clearTimeout(t); done(!!(j && j.ok && j.mode !== "local")); },
            function () { clearTimeout(t); done(false); });
  }

  /** ¿Hay un Mythos ahi? done("ok" | "local" | "no"). Primero /api/health; si la pagina local
   *  no lo puede leer (sin respuesta: caido o CORS), el icono. */
  function check(url, done) {
    var x = new XMLHttpRequest();
    var end = false;
    var finish = function (r) {
      if (!end) {
        end = true;
        done(r);
      }
    };
    try {
      x.open("GET", url + "/api/health", true);
      x.timeout = 6000;
    } catch (e) {
      return finish("no");
    }
    x.onload = function () {
      var j = null;
      try { j = JSON.parse(x.responseText); } catch (e) {}
      if (!j || !j.ok) return finish("no");
      finish(j.mode === "local" ? "local" : "ok");
    };
    var fallback = function () { probeIcon(url, 6000, function (ok) { finish(ok ? "ok" : "no"); }); };
    x.onerror = fallback;
    x.ontimeout = function () { finish("no"); };
    x.send();
  }

  function open(url) {
    save(url);
    registerKeys();
    // Esta entrada del historial queda marcada: si la interfaz vuelve aca, se cierra la app.
    history.replaceState(null, "", "#away");
    location.href = url + "/";
  }

  // ---- pantallas

  function show(id) {
    var all = ["connecting", "setup", "old"];
    for (var i = 0; i < all.length; i++) $(all[i]).hidden = all[i] !== id;
  }

  function setError(msg) {
    $("error").textContent = msg || "";
  }

  function setup(err, value) {
    cancelled = true;
    show("setup");
    var input = $("addr");
    if (value !== undefined) input.value = value;
    setError(err);
    quiet(input);
    focus(err ? $("connect") : input);
  }

  function connecting(url) {
    cancelled = false;
    show("connecting");
    $("connecting-text").textContent = tr("connectingTo", host(url));
    focus($("change"));
    var t0 = Date.now();
    check(url, function (r) {
      if (cancelled) return;
      if (r !== "ok") {
        return setup(tr(r === "local" ? "local" : "unreachable", host(url)), url);
      }
      // Un momento para poder apretar "Cambiar de servidor".
      setTimeout(function () {
        if (!cancelled) open(url);
      }, Math.max(0, 900 - (Date.now() - t0)));
    });
  }

  function connect() {
    var url = normalize($("addr").value);
    if (!isAddress(url)) return setError(tr("notAddress"));
    setError("");
    var b = $("connect");
    b.disabled = true;
    b.textContent = tr("connecting");
    check(url, function (r) {
      b.disabled = false;
      b.textContent = tr("connect");
      if (r === "ok") return open(url);
      setError(tr(r === "local" ? "local" : "unreachable", host(url)));
      focus(b);
    });
  }

  // ---- buscar en la red de la tele (:7700 en las 254 direcciones de su /24)

  function localIp(done) {
    if (!hasTizen || !tizen.systeminfo) return done("");
    var props = ["WIFI_NETWORK", "ETHERNET_NETWORK"];
    var next = function (i) {
      if (i >= props.length) return done("");
      try {
        tizen.systeminfo.getPropertyValue(props[i], function (p) {
          if (p && p.ipAddress && p.ipAddress !== "0.0.0.0") done(p.ipAddress);
          else next(i + 1);
        }, function () { next(i + 1); });
      } catch (e) {
        next(i + 1);
      }
    };
    next(0);
  }

  function find() {
    var b = $("find");
    var list = $("found");
    if (b.disabled) return;
    b.disabled = true;
    b.textContent = tr("searching");
    list.innerHTML = "";
    setError("");
    localIp(function (ip) {
      var m = /^(\d+\.\d+\.\d+)\.(\d+)$/.exec(ip);
      if (!m) {
        b.disabled = false;
        b.textContent = tr("find");
        return setError(tr("noNetwork"));
      }
      var base = m[1];
      var found = 0;
      var pending = [];
      for (var i = 1; i < 255; i++) if (String(i) !== m[2]) pending.push(base + "." + i);
      var running = 0;
      var pump = function () {
        while (running < 40 && pending.length) {
          running++;
          (function (h) {
            var url = "http://" + h + ":" + PORT;
            probeHealth(url, 2500, function (ok) {
              running--;
              if (ok) {
                found++;
                var btn = document.createElement("button");
                btn.className = "btn";
                btn.textContent = url;
                btn.onclick = function () {
                  $("addr").value = url;
                  connect();
                };
                list.appendChild(btn);
                if (found === 1) focus(btn);
              }
              pump();
            });
          })(pending.shift());
        }
        if (!running && !pending.length) {
          b.disabled = false;
          b.textContent = tr("find");
          if (!found) setError(tr("notFound", base + ".x:" + PORT));
        }
      };
      pump();
    });
  }

  // ---- control remoto

  function focusables() {
    var all = document.querySelectorAll("button, input");
    var out = [];
    for (var i = 0; i < all.length; i++) {
      var el = all[i];
      if (!el.disabled && el.offsetParent !== null) out.push(el);
    }
    return out;
  }

  function focus(el) {
    if (el) el.focus();
  }

  // El campo no abre el teclado de la tele al pasar por encima: solo con el centro.
  function quiet(input) {
    input.readOnly = true;
  }
  function type(input) {
    input.readOnly = false;
    input.blur();
    input.focus();
  }

  function move(step) {
    var list = focusables();
    var i = list.indexOf(document.activeElement);
    var next = list[i < 0 ? 0 : Math.max(0, Math.min(list.length - 1, i + step))];
    if (next && next !== document.activeElement) {
      if (next.tagName === "INPUT") quiet(next);
      focus(next);
    }
  }

  document.addEventListener("keydown", function (e) {
    var k = e.keyCode;
    var t = document.activeElement;
    var typing = t && t.tagName === "INPUT" && !t.readOnly;
    if (k === BACK || e.key === "Escape") {
      e.preventDefault();
      if (typing) {
        quiet(t);
        return;
      }
      if (!$("setup").hidden && fromChange && read()) {
        fromChange = false;
        return connecting(read());
      }
      return exitApp();
    }
    if (typing) {
      if (k === IME_DONE || k === 13) {
        e.preventDefault();
        quiet(t);
        connect();
      } else if (k === IME_CANCEL) {
        quiet(t);
      }
      return;
    }
    if (k === 13 && t && t.tagName === "INPUT") {
      e.preventDefault();
      return type(t);
    }
    if (k === 37 || k === 38) {
      e.preventDefault();
      move(-1);
    } else if (k === 39 || k === 40) {
      e.preventDefault();
      move(1);
    }
  });

  $("change").onclick = function () {
    fromChange = true;
    setup("", read());
  };
  $("connect").onclick = connect;
  $("find").onclick = find;
  $("addr").addEventListener("blur", function () { quiet($("addr")); });

  // ---- arranque

  var m = /Chrom(?:e|ium)\/(\d+)/.exec(navigator.userAgent);
  var v = m ? parseInt(m[1], 10) : 0;
  if (v && v < MIN_CHROME) {
    show("old");
    $("old-text").textContent = tr("old", v, MIN_CHROME);
    return;
  }
  registerKeys();
  var saved = read();
  if (saved) connecting(saved);
  else setup("", "");
})();
