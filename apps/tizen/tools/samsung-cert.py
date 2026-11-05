#!/usr/bin/env python3
"""Certificados de Samsung para instalar Mythos en una tele en modo desarrollador (docs/tizen.md).

Hace lo mismo que el Certificate Manager de Tizen Studio con la "Samsung Certificate Extension",
sin la interfaz de Eclipse:

1. Abre el navegador en el login de la cuenta de Samsung y espera la respuesta en
   http://localhost:4794/signin/callback (el mismo puerto y la misma app de Samsung que usa la
   extension).
2. Genera la clave y el pedido (CSR) del autor y se lo manda a Samsung (svdca.samsungqbe.com):
   vuelve el certificado de autor firmado por "Samsung VD Author CA".
3. Lo mismo para el distribuidor, con el DUID de cada tele en el certificado (nivel "Public").
4. Arma author.p12 y distributor.p12 (con la cadena de Samsung) y, si se le pasa --tizen, crea el
   perfil de firma en Tizen Studio.

Las claves quedan en --out (por defecto ~/.mythos-signing/tizen), nunca en el repo. La contraseña
de los .p12 queda en password.txt al lado. El certificado de autor sirve para siempre (es el que
firma las actualizaciones: si cambia, hay que desinstalar la app); el de distribuidor se puede
volver a pedir para sumar otra tele (--reuse-author).

Uso:
  python samsung-cert.py --duid <DUID> [--duid <otro>] [--tizen C:\\Users\\yo\\tizen-studio]
  (el DUID: `sdb -s <ip>:26101 shell 0 getduid`)

Necesita `cryptography` y `requests`.
"""
import argparse, io, json, os, secrets, sys, threading, time, urllib.parse, urllib.request, webbrowser, zipfile
from http.server import BaseHTTPRequestHandler, HTTPServer

import requests
from cryptography import x509
from cryptography.hazmat.primitives import hashes, serialization
from cryptography.hazmat.primitives.asymmetric import rsa
from cryptography.hazmat.primitives.serialization import pkcs12
from cryptography.x509.oid import NameOID

LOGIN_URL = ("https://account.samsung.com/mobile/account/check.do?serviceID=v285zxnl3h&actionID=StartOAuth2"
             "&accessToken=Y&redirect_uri=http://localhost:4794/signin/callback")
AUTHOR_URL = "https://svdca.samsungqbe.com/apis/v3/authors"
DISTRIBUTOR_URL = "https://svdca.samsungqbe.com/apis/v3/distributors"
# Las CA de Samsung (publicas) vienen en la extension; se bajan una vez a <out>/ca.
EXTENSION_ZIP = "https://download.tizen.org/sdk/extensions/tizen-certificate-extension_2.0.75.zip"
AUTHOR_CA = "vd_tizen_dev_author_ca.cer"
DISTRIBUTOR_CA = "vd_tizen_dev_public2.crt"


def log(msg):
    print(msg, flush=True)


# ---- login de Samsung

def samsung_login(timeout=600):
    """Abre el login y devuelve {access_token, userId, inputEmailID} de la respuesta de Samsung
    (un POST de formulario con `code` = JSON)."""
    got = {}

    class Handler(BaseHTTPRequestHandler):
        def log_message(self, *a):
            pass

        def _done(self, raw):
            form = urllib.parse.parse_qs(raw)
            code = (form.get("code") or [""])[0]
            try:
                data = json.loads(code) if code else {k: v[0] for k, v in form.items()}
            except ValueError:
                data = {}
            ok = bool(data.get("access_token") and data.get("userId"))
            body = ("Signed in. You can close this tab and go back to the terminal." if ok
                    else "Sign-in failed: no token in the answer. Try again.").encode()
            self.send_response(200)
            self.send_header("Content-Type", "text/plain; charset=utf-8")
            self.end_headers()
            self.wfile.write(body)
            if ok:
                got.update(data)

        def do_POST(self):
            n = int(self.headers.get("Content-Length") or 0)
            self._done(self.rfile.read(n).decode("utf-8", "replace"))

        def do_GET(self):
            self._done(urllib.parse.urlsplit(self.path).query)

    srv = HTTPServer(("127.0.0.1", 4794), Handler)
    threading.Thread(target=srv.serve_forever, daemon=True).start()
    log("Abriendo el login de Samsung en el navegador (si no se abre, entra a esta direccion):")
    log("  " + LOGIN_URL)
    webbrowser.open(LOGIN_URL)
    end = time.time() + timeout
    while not got and time.time() < end:
        time.sleep(0.3)
    srv.shutdown()
    if not got:
        sys.exit("No llego el login de Samsung a tiempo.")
    log(f"Sesion de Samsung: {got.get('inputEmailID') or got['userId']}")
    return got


# ---- claves, pedidos y certificados

def new_key():
    return rsa.generate_private_key(public_exponent=65537, key_size=2048)


def csr_pem(key, name, san_uris=()):
    b = x509.CertificateSigningRequestBuilder().subject_name(name)
    if san_uris:
        b = b.add_extension(x509.SubjectAlternativeName([x509.UniformResourceIdentifier(u) for u in san_uris]), critical=False)
    return b.sign(key, hashes.SHA512()).public_bytes(serialization.Encoding.PEM)


def ask_samsung(url, auth, csr, extra):
    fields = {"access_token": auth["access_token"], "user_id": auth["userId"], "platform": "VD", **extra}
    r = requests.post(url, data=fields, files={"csr": ("request.csr", csr, "application/octet-stream")}, timeout=60)
    text = r.text.strip()
    if r.status_code != 200 or "BEGIN CERTIFICATE" not in text:
        sys.exit(f"Samsung contesto {r.status_code} en {url}:\n{text[:800]}")
    return x509.load_pem_x509_certificate(text.encode())


def save_p12(path, key, cert, chain, password, alias=b"usercertificate"):
    enc = (serialization.PrivateFormat.PKCS12.encryption_builder()
           .kdf_rounds(50000)
           # 3DES/SHA1: lo que lee el Java 8 de Tizen Studio.
           .key_cert_algorithm(pkcs12.PBES.PBESv1SHA1And3KeyTripleDESCBC)
           .hmac_hash(hashes.SHA1())
           .build(password.encode()))
    with open(path, "wb") as f:
        f.write(pkcs12.serialize_key_and_certificates(alias, key, cert, chain, enc))


def load_ca(out):
    ca_dir = os.path.join(out, "ca")
    os.makedirs(ca_dir, exist_ok=True)
    need = [n for n in (AUTHOR_CA, DISTRIBUTOR_CA) if not os.path.exists(os.path.join(ca_dir, n))]
    if need:
        log("Bajando las CA de Samsung (Samsung Certificate Extension)...")
        outer = zipfile.ZipFile(io.BytesIO(urllib.request.urlopen(EXTENSION_ZIP, timeout=120).read()))
        inner_name = next(n for n in outer.namelist() if n.endswith("_windows-64.zip"))
        inner = zipfile.ZipFile(io.BytesIO(outer.read(inner_name)))
        jar_name = next(n for n in inner.namelist() if n.startswith("data/tools/certificate-manager/plugins/") and n.endswith(".jar"))
        jar = zipfile.ZipFile(io.BytesIO(inner.read(jar_name)))
        for n in need:
            with open(os.path.join(ca_dir, n), "wb") as f:
                f.write(jar.read("res/ca/" + n))
    read = lambda n: x509.load_pem_x509_certificate(open(os.path.join(ca_dir, n), "rb").read())
    return read(AUTHOR_CA), read(DISTRIBUTOR_CA)


def main():
    ap = argparse.ArgumentParser(description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter)
    ap.add_argument("--duid", action="append", required=True, help="DUID de la tele (se puede repetir)")
    ap.add_argument("--out", default=os.path.join(os.path.expanduser("~"), ".mythos-signing", "tizen"))
    ap.add_argument("--name", default="Mythos", help="nombre del autor en el certificado")
    ap.add_argument("--profile", default="Mythos", help="nombre del perfil de firma en Tizen Studio")
    ap.add_argument("--tizen", help="carpeta de Tizen Studio: crea el perfil de firma con tizen security-profiles")
    ap.add_argument("--reuse-author", action="store_true", help="no pedir otro autor (usa author.p12 de --out)")
    a = ap.parse_args()

    os.makedirs(a.out, exist_ok=True)
    author_ca, dist_ca = load_ca(a.out)
    pw_file = os.path.join(a.out, "password.txt")
    author_p12 = os.path.join(a.out, "author.p12")
    if a.reuse_author:
        password = open(pw_file).read().strip()
        if not os.path.exists(author_p12):
            sys.exit(f"No esta {author_p12}")
    else:
        if os.path.exists(author_p12):
            sys.exit(f"Ya hay un {author_p12}: cambiar el autor obliga a desinstalar la app. "
                     "Para sumar una tele usa --reuse-author; para empezar de cero, borralo a mano.")
        password = secrets.token_urlsafe(18)

    auth = samsung_login()
    email = auth.get("inputEmailID") or ""

    if not a.reuse_author:
        key = new_key()
        csr = csr_pem(key, x509.Name([x509.NameAttribute(NameOID.COMMON_NAME, a.name)]))
        cert = ask_samsung(AUTHOR_URL, auth, csr, {})
        save_p12(author_p12, key, cert, [author_ca], password)
        with open(pw_file, "w") as f:
            f.write(password)
        log(f"Autor: {author_p12} (vence {cert.not_valid_after_utc:%Y-%m-%d})")

    key = new_key()
    attrs = [x509.NameAttribute(NameOID.COMMON_NAME, "TizenSDK")]
    if email:
        attrs.append(x509.NameAttribute(NameOID.EMAIL_ADDRESS, email))
    uris = ["URN:tizen:packageid="] + [f"URN:tizen:deviceid={d.strip()}" for d in a.duid]
    csr = csr_pem(key, x509.Name(attrs), uris)
    cert = ask_samsung(DISTRIBUTOR_URL, auth, csr, {"privilege_level": "Public", "developer_type": "Individual"})
    dist_p12 = os.path.join(a.out, "distributor.p12")
    save_p12(dist_p12, key, cert, [dist_ca], password)
    log(f"Distribuidor: {dist_p12} (teles: {', '.join(a.duid)}; vence {cert.not_valid_after_utc:%Y-%m-%d})")

    cmd = ["security-profiles", "add", "-n", a.profile, "-a", author_p12, "-p", password, "-d", dist_p12, "-dp", password]
    if a.tizen:
        import subprocess
        tizen = os.path.join(a.tizen, "tools", "ide", "bin", "tizen.bat" if os.name == "nt" else "tizen")
        subprocess.run([tizen, "security-profiles", "remove", "-n", a.profile], capture_output=True)
        r = subprocess.run([tizen, *cmd], capture_output=True, text=True)
        if r.returncode != 0:
            sys.exit(r.stdout + r.stderr)
        log(f"Perfil de firma '{a.profile}' creado en Tizen Studio.")
    else:
        log("Para crear el perfil de firma: tizen " + " ".join(cmd[:-4]) + " -d <distributor.p12> -dp <password>")


if __name__ == "__main__":
    main()
