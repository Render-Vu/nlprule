# nlprule-server

Servidor HTTP del fork: corrección compatible con la API `/v2/check` de LanguageTool y
autocompletado con concordancia gramatical en `/complete`.

```bash
scripts/download_es.sh
cargo run --release -p nlprule-server -- \
  --tokenizer storage/es_tokenizer.bin --rules storage/es_rules.bin \
  [--freq es_50k.txt] [--port 8011] [--host 127.0.0.1] [--threads 4]
```

El puerto por defecto es 8011 para poder convivir con LanguageTool en el 8010.

## Endpoints

| Ruta | Método | Parámetros |
|------|--------|------------|
| `/v2/check` | GET, POST (form) | `text` o `data` (anotaciones de LanguageTool), `language` (`es`, `es-ES`, `auto`), `disabledRules`, `disabledCategories` |
| `/complete` | GET, POST (form o JSON) | `text` (texto hasta el cursor), `cursor` (opcional, en caracteres), `limit` (10), `tag` (prefijo de etiqueta EAGLES, p. ej. `VMIP3P`), `context` (`true`), `validate` (`false`) |
| `/v2/languages` | GET | — |
| `/rules` | GET | — (ids de las reglas cargadas, como los nombra LanguageTool) |
| `/health` | GET | — |

Las respuestas de `/v2/check` tienen la forma de LanguageTool (`matches[].offset/length`
en unidades UTF-16, `replacements[].value`, `rule.id`, `rule.category`, `context`, `sentence`).
Solo se implementa lo que usan los clientes habituales; las reglas son las de nlprule
(LanguageTool 5.2), no las de un LanguageTool actual.

```bash
curl -s localhost:8011/v2/check --data-urlencode language=es \
  --data-urlencode "text=El agua esta fría."

curl -s localhost:8011/complete --data-urlencode "text=Los niños jue" --data-urlencode limit=5
```

Respuesta de `/complete` (abreviada):

```json
{
  "prefix": "jue", "start": 10, "end": 13,
  "context": { "expected": ["A[QO]?MP*", "V?[IS]?3P*"], "boosted": [] },
  "items": [
    { "word": "juegan", "lemmas": ["jugar"], "tags": ["VMIP3P0"],
      "matches_context": true, "context_weight": 2, "frequency": 500, "score": 6.2 }
  ]
}
```

`start`/`end` son posiciones en caracteres del prefijo que hay que reemplazar.

## Doom Emacs

Basta con cambiar el puerto en la configuración de LanguageTool:

```elisp
(setq flycheck-languagetool-url "http://localhost:8011")

(after! langtool
  (setq langtool-http-server-host "localhost"
        langtool-http-server-port 8011))
```

Para el autocompletado con corfu, ver [emacs/README.md](../emacs/README.md).
