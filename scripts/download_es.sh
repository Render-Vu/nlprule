#!/usr/bin/env bash
# Descarga los binarios de español de nlprule 0.6.4 a storage/ (necesarios para los tests,
# el CLI `complete` y nlprule-server).
#
# Frecuencias (opcional): cualquier lista "palabra conteo" sirve, p. ej. la de OpenSubtitles en
# https://github.com/hermitdave/FrequencyWords (content/2018/es/es_50k.txt). Revisa su licencia
# antes de redistribuirla; no se incluye en este repositorio.
set -euo pipefail

VERSION="${NLPRULE_VERSION:-0.6.4}"
LANGS="${*:-es}"
DIR="$(cd "$(dirname "$0")/.." && pwd)/storage"
mkdir -p "$DIR"

for lang in $LANGS; do
  for kind in tokenizer rules; do
    file="${lang}_${kind}.bin"
    if [ -f "$DIR/$file" ]; then
      echo "ya existe: storage/$file"
      continue
    fi
    echo "descargando $file ($VERSION)"
    curl -fsSL "https://github.com/bminixhofer/nlprule/releases/download/${VERSION}/${file}.gz" \
      | gunzip > "$DIR/$file.tmp"
    mv "$DIR/$file.tmp" "$DIR/$file"
  done
done
