# nlprule-complete.el

Autocompletado con concordancia gramatical en Emacs, servido por
[nlprule-server](../server/README.md). Es una función `completion-at-point`
(capf), así que funciona con corfu (y con company vía `company-capf`, o con
`M-x completion-at-point`).

```
Los niños jue|      →  juegan     VMIP3P0 · jugar
                       jueguen    VMM03P0,VMSP3P0 · jugar
si nosotros escrib| →  escribiéramos, escribiésemos
```

## Requisitos

- Emacs 27.1 o posterior (probado con 29.3 y corfu 2.16).
- `nlprule-server` en marcha (por defecto en `http://localhost:8011`):

```bash
scripts/download_es.sh
cargo run --release -p nlprule-server -- -t storage/es_tokenizer.bin -r storage/es_rules.bin
```

## Doom Emacs

`init.el`: activar corfu en `:completion` (por ejemplo `(corfu +orderless)`).

`packages.el`, desde una copia local del repositorio:

```elisp
(package! nlprule-complete :recipe (:local-repo "~/src/nlprule" :files ("emacs/nlprule-complete.el")))
```

o directamente desde GitHub (mientras no esté en `master`, con la rama del fork):

```elisp
(package! nlprule-complete
  :recipe (:host github :repo "render-vu/nlprule" :branch "claude/zealous-davinci-5xtggo"
           :files ("emacs/nlprule-complete.el")))
```

`config.el`:

```elisp
(use-package! nlprule-complete
  :hook ((text-mode org-mode markdown-mode) . nlprule-complete-mode)
  :init
  (setq nlprule-complete-url "http://localhost:8011"
        nlprule-complete-min-prefix 2))

;; corfu: abrir solo, tras 2 letras
(after! corfu
  (setq corfu-auto t
        corfu-auto-prefix 2
        corfu-auto-delay 0.15))
```

Después: `doom sync` y reiniciar Emacs. `M-x nlprule-complete-status` comprueba
si el servidor responde.

### Emacs sin Doom

```elisp
(add-to-list 'load-path "~/src/nlprule/emacs")
(require 'nlprule-complete)
(add-hook 'text-mode-hook #'nlprule-complete-mode)
(setq corfu-auto t corfu-auto-prefix 2)
(global-corfu-mode)
```

## Cómo funciona

- Envía a `/complete` el texto desde el inicio del párrafo (como mucho
  `nlprule-complete-context-chars`, 300) hasta el cursor. El servidor deduce la
  etiqueta esperada y devuelve los candidatos ordenados.
- Se respeta ese orden: la tabla declara `display-sort-function` `identity`, así
  que corfu no reordena por longitud ni historial.
- Mientras se sigue escribiendo, corfu vuelve a consultar la tabla y esta pide
  al servidor el nuevo prefijo con el mismo contexto (con caché por prefijo).
- Peticiones síncronas con `nlprule-complete-timeout` (0,5 s). Si el servidor no
  responde, el modo calla durante `nlprule-complete-retry-delay` (30 s) y otras
  capfs toman el relevo (`:exclusive 'no`).
- Cada petición tarda ~10 ms desde Emacs. Para ello no usa
  `url-retrieve-synchronously` (sondea cada 50 ms) y desactiva keep-alive (en una
  conexión reutilizada, el ACK retardado añade ~40 ms por respuesta).

## Opciones

| Variable | Defecto | |
|---|---|---|
| `nlprule-complete-url` | `"http://localhost:8011"` | URL del servidor |
| `nlprule-complete-limit` | `10` | candidatos pedidos |
| `nlprule-complete-min-prefix` | `2` | letras antes de consultar |
| `nlprule-complete-timeout` | `0.5` | segundos de espera |
| `nlprule-complete-retry-delay` | `30` | segundos en silencio si el servidor no responde |
| `nlprule-complete-context-chars` | `300` | contexto enviado |
| `nlprule-complete-only-matching` | `nil` | mostrar solo los que concuerdan con el contexto |
| `nlprule-complete-validate` | `nil` | descartar candidatos que las reglas marcan como error (más lento) |
| `nlprule-complete-annotate` | `t` | mostrar etiqueta y lema |

## Arranque automático del servidor (systemd de usuario, opcional)

`~/.config/systemd/user/nlprule-server.service`:

```ini
[Unit]
Description=nlprule-server (corrección y autocompletado)

[Service]
ExecStart=%h/src/nlprule/target/release/nlprule-server -t %h/src/nlprule/storage/es_tokenizer.bin -r %h/src/nlprule/storage/es_rules.bin
Restart=on-failure

[Install]
WantedBy=default.target
```

```bash
systemctl --user enable --now nlprule-server
```

## Tests

```bash
emacs -Q --batch -L emacs -l emacs/nlprule-complete-tests.el -f ert-run-tests-batch-and-exit
```

Los tests de integración se ejecutan si el servidor responde en
`$NLPRULE_SERVER_URL` (por defecto `http://localhost:8011`); el de corfu, si
además corfu está en el `load-path` (`-L ruta/a/corfu -L ruta/a/compat`).
