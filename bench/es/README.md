# Banco de pruebas en español

Mide el fork frente a LanguageTool con la misma API `/v2/check` y comprueba el
autocompletado de `/complete`. Solo necesita Python 3 (biblioteca estándar).

```bash
# servidores: LanguageTool en :8010 (Docker) y el fork en :8011
./target/release/nlprule-server -t storage/es_tokenizer.bin -r storage/es_rules.bin

python3 bench/es/run.py            # resumen y comparación con baseline.json
python3 bench/es/run.py -v         # detalle por frase
python3 bench/es/run.py --lt none  # sin LanguageTool: usa sus resultados de la línea base
```

Para saber qué reglas de LanguageTool son XML (se recuperan actualizando las reglas, paso 3)
y cuáles son Java, pasa la carpeta de reglas del repositorio de LanguageTool:

```bash
python3 bench/es/run.py --lt-rules ~/src/languagetool/languagetool-language-modules/es/src/main/resources/org/languagetool/rules/es
```

## Archivos

- `check.jsonl`: una frase por línea. `errors` lista los errores esperados con el fragmento
  (`span`, la primera aparición como palabra completa; `occ` para la n-ésima) y la corrección
  esperada (`fix`). Las frases con `"errors": []` son correctas: cualquier aviso en ellas cuenta
  como falso positivo.
- `complete.jsonl`: texto hasta el cursor y palabras que deben salir entre las `top` primeras.
- `baseline.json`: resultados guardados (fork y LanguageTool) con los que se compara cada ejecución.
- `run.py`: el script.

## Métricas

- **Errores detectados**: algún aviso se solapa con el fragmento esperado.
- **Con la corrección esperada**: además, `fix` está entre sus 5 primeras sugerencias.
- **Falsos positivos**: frases correctas con algún aviso.
- **Cobertura del fork sobre LanguageTool**: de los errores que detecta LanguageTool, cuántos
  detecta también el fork. Es la meta del documento (al empezar, 2 de 7).
- **Por regla**: lo que detecta LanguageTool y el fork no, con el tipo de regla:
  `XML` (portable actualizando las reglas), `Java` (corrector ortográfico, reglas programadas),
  `en el fork` (la regla existe pero no se activa con esa frase).

## Línea base y regresiones

`run.py` compara con `baseline.json` y termina con código 1 si el fork pierde una detección, una
corrección o un caso de autocompletado, o si aparece un falso positivo nuevo. Tras una mejora
intencionada, guarda la nueva línea base:

```bash
python3 bench/es/run.py --lt-rules <carpeta rules/es> --save bench/es/baseline.json
```

La línea base actual (`baseline.json`) es la del fork con los binarios compilados desde
LanguageTool 6.6, medida contra LanguageTool 6.6: el fork detecta 26 de 56 errores (54 % de lo que
detecta LanguageTool). `baseline-lt5.2.json` guarda la de los binarios originales (LanguageTool 5.2):
17 de 56 (35 %). Con otra versión de LanguageTool sus resultados pueden cambiar; vuelve a guardarla
para comparar.

El corpus refleja la norma académica tal como la entendemos; algunas frases son discutibles
(p. ej. «Se lo di a el ayer»: LanguageTool propone «al»). Hay casos que ninguno de los dos detecta
(«Mis amigos viene», «No se nada», «Si tendría dinero», «boy al médico»…): quedan como metas
más allá de LanguageTool.
