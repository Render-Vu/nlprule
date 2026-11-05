#!/usr/bin/env bash
# Prepara los binarios de español en storage/ (necesarios para los tests, el CLI `complete`
# y nlprule-server).
#
# Por defecto usa los del fork, compilados desde LanguageTool 6.6 y guardados en binaries/es/.
# Con --upstream descarga los originales de nlprule 0.6.4 (LanguageTool 5.2).
# Con --force sustituye los que ya haya en storage/.
#
#   scripts/download_es.sh                 # binarios del fork (LanguageTool 6.6)
#   scripts/download_es.sh --upstream es en  # originales 0.6.4 para es y en
#
# Frecuencias (opcional): cualquier lista "palabra conteo" sirve, p. ej. la de OpenSubtitles en
# https://github.com/hermitdave/FrequencyWords (content/2018/es/es_50k.txt). Revisa su licencia
# antes de redistribuirla; no se incluye en este repositorio.
set -euo pipefail

ROOT="$(cd "$(dirname "$0")/.." && pwd)"
DIR="$ROOT/storage"
VERSION="${NLPRULE_VERSION:-0.6.4}"
UPSTREAM=0
FORCE=0
LANGS=()
for arg in "$@"; do
  case "$arg" in
    --upstream) UPSTREAM=1 ;;
    --force) FORCE=1 ;;
    *) LANGS+=("$arg") ;;
  esac
done
[ ${#LANGS[@]} -eq 0 ] && LANGS=(es)
mkdir -p "$DIR"

for lang in "${LANGS[@]}"; do
  for kind in tokenizer rules; do
    file="${lang}_${kind}.bin"
    if [ -f "$DIR/$file" ] && [ "$FORCE" -eq 0 ]; then
      echo "ya existe: storage/$file (usa --force para sustituirlo)"
      continue
    fi
    local_gz="$ROOT/binaries/$lang/$file.gz"
    if [ "$UPSTREAM" -eq 0 ] && [ -f "$local_gz" ]; then
      echo "copiando $file desde binaries/$lang/"
      gunzip -c "$local_gz" > "$DIR/$file.tmp"
    else
      echo "descargando $file (nlprule $VERSION)"
      curl -fsSL "https://github.com/bminixhofer/nlprule/releases/download/${VERSION}/${file}.gz" \
        | gunzip > "$DIR/$file.tmp"
    fi
    mv "$DIR/$file.tmp" "$DIR/$file"
  done
done
