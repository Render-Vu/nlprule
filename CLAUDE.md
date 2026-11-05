# Fork de nlprule: corrector y autocompletado en español

Fork de [nlprule](https://github.com/bminixhofer/nlprule) 0.6.4 (Rust) que sirve a la vez de
corrector gramatical (reglas de LanguageTool) y de autocompletado con concordancia en español.
Documento de diseño y bitácora: «Corrector y autocompletado en español: LanguageTool y fork de
nlprule» (Claude Docs, https://claude.ai/artifact/T4K2qF5axjog57GtiANmbe).

Trabajo en la rama `claude/zealous-davinci-5xtggo` (sin PR todavía).

## Puesta en marcha

```bash
scripts/download_es.sh                     # binarios del fork (LanguageTool 6.6) -> storage/
cargo build --release -p nlprule-server
cargo build --release -p nlprule --features bin --bins
./target/release/nlprule-server -t storage/es_tokenizer.bin -r storage/es_rules.bin   # :8011
./target/release/complete -t storage/es_tokenizer.bin "Los niños jue"
```

LanguageTool de referencia en :8010 (Docker `erikvl87/languagetool`, o el standalone:
`java -cp languagetool-server.jar org.languagetool.server.HTTPServer --port 8010`).

## Tests y banco de pruebas

```bash
cargo test -p nlprule -p nlprule-server          # necesita storage/es_*.bin y storage/en_*.bin
scripts/download_es.sh --upstream en             # binarios en inglés para tests/tests.rs
python3 bench/es/run.py -v                       # fork :8011 vs LanguageTool :8010 + autocompletado
python3 bench/es/run.py --lt none                # sin LanguageTool (usa la línea base)
emacs -Q --batch -L emacs -l emacs/nlprule-complete-tests.el -f ert-run-tests-batch-and-exit
./target/release/test_disambiguation -t storage/es_tokenizer.bin
./target/release/test -t storage/es_tokenizer.bin -r storage/es_rules.bin
```

`bench/es/run.py` sale con código 1 si el fork empeora frente a `bench/es/baseline.json`.
**Pasa el banco antes y después de tocar reglas, el compilador o el autocompletado.** Tras una
mejora intencionada: `--save bench/es/baseline.json`.

Estado actual (LanguageTool 6.6 como referencia): fork 26/56 errores, LanguageTool 48/56,
cobertura 54 %, 0/20 falsos positivos, autocompletado 12/12. Con los binarios 5.2: 17/56 (35 %).

## Mapa del código

| Ruta | Qué es |
|---|---|
| `nlprule/src/tokenizer/tag.rs` | Tagger. `word_fst` (no serializado) + `words_with_prefix`, `is_known_form` |
| `nlprule/src/complete/` | Autocompletado: `mod.rs` (Completer, ranking), `context.rs` (reglas EAGLES de concordancia), `freq.rs` (frecuencias en FST) |
| `nlprule/src/bin/complete.rs` | CLI de autocompletado |
| `server/` | `nlprule-server`: `/v2/check` compatible con LanguageTool, `/complete`, `/rules`, `/health` |
| `emacs/nlprule-complete.el` | capf para corfu; tests ERT en `emacs/nlprule-complete-tests.el` |
| `bench/es/` | corpus (`check.jsonl`, `complete.jsonl`), `run.py`, líneas base |
| `build/make_build_dir.py` | extrae de LanguageTool el directorio de compilación (`data/es`, fuera de git) |
| `nlprule/src/compile/` | compilador XML -> binarios (`structure.rs`, `parse_structure.rs`, `impls.rs`) |
| `nlprule/configs/es/` | opciones por idioma: `tagger.json` (extra_tags), `tokenizer.json` (`skip_unsupported`, ignore_ids), `rules.json` (ignore_ids) |
| `binaries/es/` | binarios del fork comprimidos (LGPL), con instrucciones para regenerarlos en `binaries/README.md` |

## Recompilar el español desde LanguageTool

Instrucciones completas en `binaries/README.md`. Resumen: LanguageTool 6.6 standalone en `$LT`,
`unzip $LT/libs/spanish-pos-dict.jar`, `make_build_dir.py` (pip: lxml, wordfreq, chardet),
`compile`, luego `test_disambiguation`, `test` y el banco. **La compilación tarda ~12 min**
(el paso de reglas es el lento); lánzala en segundo plano.

Para decidir qué reglas desactivar: clasificar los fallos de `test` (RUST_LOG=warn) en «no detecta»
(inofensivo, se mantiene), «falso positivo» y «otra sugerencia». Solo los falsos positivos van a
`configs/es/rules.json`. Los `ignore_ids` van por índice: al cambiar de versión de LanguageTool,
revisar la lista entera.

## Trampas encontradas (no repetir)

- **No añadir campos a structs serializados** (`TokenizerLangOptions`, `TaggerFields`, etc.):
  bincode es posicional y rompe la carga de todos los `.bin` existentes. Las opciones solo de
  compilación se leen del JSON crudo (`compile/utils.rs::tokenizer_skip_unsupported`).
- Desambiguación: sin `skip_unsupported`, la primera regla no soportada descarta todas las
  siguientes (con LanguageTool 6.6 quedaba 1 de ~800 y el fork caía a 8/56).
- `clap_derive` no compila: `python/Cargo.toml` fija `syn = "=1.0.57"`. Todos los bins usan la API
  builder de clap 3.
- Emacs: `url-retrieve-synchronously` sondea cada 50 ms y keep-alive con tiny_http añade ~40 ms
  (ACK retardado); el paquete usa `url-retrieve` + espera de 5 ms y keep-alive desactivado (~10 ms).
- Emacs en batch sin `LANG=C.UTF-8` decodifica mal los `--eval` con tildes.
- Los offsets de `/v2/check` son UTF-16 (como LanguageTool); `/complete` devuelve caracteres.
- `pkill -f <patrón>` dentro de un `bash -c` se mata a sí mismo; usar `pgrep` + `kill`.

## Roadmap

Hecho: autocompletado (prefijo + concordancia + frecuencias opcionales + validación), servidor,
integración con corfu, banco de pruebas, reglas de LanguageTool 6.6.

Siguiente, por impacto:

1. **Síntesis de formas** (`postag_replace` / `postag` en `<match>`, 224 reglas; error
   «postag, postag_regex, postag_replace and text in `match` are not implemented» en
   `parse_structure.rs::parse_match`). Hace falta un índice lema+etiqueta -> forma (se puede
   construir en compilación a partir de `tags/output.dump` y guardarlo en el binario de reglas, o
   derivarlo del tagger). Desbloquea haber impersonal, «Han habido», concordancia de determinante
   (4 errores del banco).
2. **Frecuencias reales** para el autocompletado (pregunta abierta: fuente y licencia; p. ej.
   FrequencyWords/OpenSubtitles). Es lo que más se nota al usarlo.
3. **Carga directa del FST** (no expandir a BiMap): carga < 1 s y menos memoria; ahora que el
   formato de reglas está estabilizado.
4. **Corrector ortográfico** sobre el diccionario (3,69 M formas): cubriría `MORFOLOGIK_RULE_ES`
   (6 errores del banco: fuistes, interezante, exámen…).
5. **Reglas propias / portar reglas Java** sencillas: signos de apertura (¿ ¡), repetición de
   palabra, mayúscula inicial, espacio antes de coma, `ES_SIMPLE_REPLACE` (replace.txt).
6. Filtros Java más usados (173 reglas): `ConfusionCheckFilter`, `FindSuggestionsFilter`,
   `AdvancedSynthesizerFilter` (depende de 1), `PostponedAdjectiveConcordanceFilter`.
7. Autocompletado: más contexto (preposiciones, verbos + infinitivo), predicción sin prefijo.
8. Ampliar el corpus del banco y revisar casos discutibles (p. ej. «Se lo di a el ayer»).

Pendiente de decisión: licencias de distribución (código MIT/Apache, binarios LGPL-2.1, lista de
frecuencias), y si abrir PR de la rama.
