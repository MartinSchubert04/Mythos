#!/usr/bin/env bash
# Sube archivos al Release del tag actual (vX.Y.Z), creandolo si todavia no existe. Lo usan
# desktop.yml y android.yml (igual que en Haze): el que termina primero crea el Release y el
# otro suma lo suyo, asi cada version trae el instalador de Windows y el APK.
#
#   bash .github/scripts/release.sh 0.3.0 Mythos-0.3.0-android.apk latest-android.json
set -euo pipefail

V="$1"
shift
TAG="$GITHUB_REF_NAME"

cat > notas.md <<NOTAS
**Windows:** bajá \`Mythos-$V-setup.exe\` y abrilo. Se instala solo para tu usuario, sin pedir permisos de administrador.

**Android:** bajá \`Mythos-$V-android.apk\` en el celular y abrilo. Android va a pedir permiso para instalar apps de esta fuente.

La primera vez la app pide la dirección de tu server (\`https://<tu-server>.ts.net:7700\`). Si ya tenés Mythos instalado, no hace falta bajar nada: se actualiza sola.
NOTAS

# Las apps instaladas buscan latest.json / latest-android.json en el ultimo Release ("latest").
# Solo se marca como ultimo si es la version mas alta: con dos tags seguidos, el viejo puede
# terminar despues y no tiene que pisar al nuevo. Si el otro workflow ya lo creo, solo se suben
# los archivos.
LATEST=$(gh release view --json tagName -q .tagName 2>/dev/null || true)
ES_ULTIMO=true
if [ -n "$LATEST" ] && [ "$(printf '%s\n%s\n' "${LATEST#v}" "$V" | sort -V | tail -1)" != "$V" ]; then
  ES_ULTIMO=false
fi
if ! gh release view "$TAG" >/dev/null 2>&1; then
  gh release create "$TAG" --title "Mythos $V" --notes-file notas.md --latest="$ES_ULTIMO" || gh release view "$TAG" >/dev/null
fi
gh release upload "$TAG" "$@" --clobber
