#!/usr/bin/env python3
"""Banco de pruebas del fork en español.

Compara nlprule-server (el fork) con LanguageTool usando la misma API /v2/check,
y comprueba el autocompletado de /complete. Solo usa la biblioteca estándar.

    python3 bench/es/run.py                       # fork en :8011, LanguageTool en :8010
    python3 bench/es/run.py --lt none             # sin LanguageTool (usa la línea base para él)
    python3 bench/es/run.py --save bench/es/baseline.json   # guardar resultados como línea base
    python3 bench/es/run.py --verbose             # detalle por frase

Con --baseline (por defecto bench/es/baseline.json si existe) informa de regresiones del
fork respecto a la línea base y termina con código 1 si las hay.
"""

import argparse
import json
import re
import sys
import time
import urllib.error
import urllib.parse
import urllib.request
from collections import defaultdict
from pathlib import Path

HERE = Path(__file__).resolve().parent


def load_jsonl(path):
    with open(path, encoding="utf-8") as f:
        return [json.loads(line) for line in f if line.strip() and not line.startswith("#")]


def post(url, params, timeout=30):
    data = urllib.parse.urlencode(params).encode("utf-8")
    request = urllib.request.Request(url, data=data, method="POST")
    with urllib.request.urlopen(request, timeout=timeout) as response:
        return json.loads(response.read().decode("utf-8"))


def reachable(base):
    for path in ("/health", "/v2/languages"):
        try:
            with urllib.request.urlopen(base + path, timeout=3):
                return True
        except (urllib.error.URLError, OSError):
            continue
    return False


def utf16_len(text):
    return len(text.encode("utf-16-le")) // 2


def find_span(text, span, occurrence=1):
    """Char range of the n-th occurrence of `span`, preferring whole-word matches."""
    pattern = r"(?<!\w)" + re.escape(span) + r"(?!\w)"
    starts = [m.start() for m in re.finditer(pattern, text)]
    if not starts:
        starts = [m.start() for m in re.finditer(re.escape(span), text)]
    if len(starts) < occurrence:
        raise ValueError(f"{span!r} no aparece {occurrence} vez/veces en {text!r}")
    start = starts[occurrence - 1]
    return start, start + len(span)


def check(base, text):
    """Matches as (start, end) UTF-16 offsets with rule id and replacements."""
    answer = post(base + "/v2/check", {"language": "es", "text": text})
    return [
        {
            "start": m["offset"],
            "end": m["offset"] + m["length"],
            "rule": m["rule"]["id"],
            "replacements": [r["value"] for r in m.get("replacements", [])][:5],
            "message": m.get("message", ""),
        }
        for m in answer["matches"]
    ]


def evaluate_case(case, matches):
    """Detection and correction of each expected error, plus unexpected matches."""
    text = case["text"]
    used = set()
    errors = []
    for error in case["errors"]:
        start, end = find_span(text, error["span"], error.get("occ", 1))
        start16, end16 = utf16_len(text[:start]), utf16_len(text[:end])
        hits = [
            i
            for i, m in enumerate(matches)
            # overlap; a zero-length match at the span counts too
            if (m["start"] < end16 and start16 < m["end"]) or m["start"] == start16
        ]
        used.update(hits)
        fix = error.get("fix")
        corrected = any(
            fix is not None
            and (fix in matches[i]["replacements"]
                 # a match may cover more than the span: compare the corrected text
                 or any(
                     _apply(text, matches[i], r) == _apply_fix(text, start, end, fix)
                     for r in matches[i]["replacements"]
                 ))
            for i in hits
        )
        errors.append(
            {
                "span": error["span"],
                "fix": fix,
                "detected": bool(hits),
                "corrected": corrected,
                "rules": sorted({matches[i]["rule"] for i in hits}),
            }
        )
    extra = [m for i, m in enumerate(matches) if i not in used]
    return {"errors": errors, "extra": extra}


def _char_index(text, offset16):
    count = 0
    for i, c in enumerate(text):
        if count >= offset16:
            return i
        count += utf16_len(c)
    return len(text)


def _apply(text, match, replacement):
    start, end = _char_index(text, match["start"]), _char_index(text, match["end"])
    return text[:start] + replacement + text[end:]


def _apply_fix(text, start, end, fix):
    return text[:start] + fix + text[end:]


def run_checks(cases, engines):
    results = {}
    for case in cases:
        results[case["id"]] = {}
        for name, base in engines.items():
            started = time.perf_counter()
            matches = check(base, case["text"])
            result = evaluate_case(case, matches)
            result["ms"] = round((time.perf_counter() - started) * 1000, 1)
            results[case["id"]][name] = result
    return results


def run_completions(cases, base):
    results = {}
    for case in cases:
        answer = post(base + "/complete", {"text": case["text"], "limit": 10})
        words = [item["word"] for item in answer["items"]]
        top = words[: case.get("top", 1)]
        ok = all(word in top for word in case["expect"])
        if case.get("expect_no_context"):
            ok = ok and not answer["context"]["expected"]
        results[case["id"]] = {"ok": ok, "words": words[:5], "context": answer["context"]["expected"]}
    return results


def summarize(cases, results, engine):
    total = detected = corrected = 0
    false_positive_sentences = extra = 0
    by_cat = defaultdict(lambda: [0, 0])
    for case in cases:
        result = results[case["id"]].get(engine)
        if result is None:
            continue
        if not case["errors"]:
            false_positive_sentences += bool(result["extra"])
        else:
            extra += len(result["extra"])
        for error in result["errors"]:
            total += 1
            detected += error["detected"]
            corrected += error["corrected"]
            by_cat[case["cat"]][0] += error["detected"]
            by_cat[case["cat"]][1] += 1
    return {
        "errors": total,
        "detected": detected,
        "corrected": corrected,
        "correct_sentences": sum(1 for c in cases if not c["errors"]),
        "false_positive_sentences": false_positive_sentences,
        "extra_matches": extra,
        "by_category": dict(by_cat),
    }


def coverage(cases, results, reference="languagetool", engine="fork"):
    """Share of the reference's detections that the engine also makes."""
    both = ref = 0
    for case in cases:
        r = results[case["id"]]
        if reference not in r or engine not in r:
            continue
        for e_ref, e_eng in zip(r[reference]["errors"], r[engine]["errors"]):
            if e_ref["detected"]:
                ref += 1
                both += e_eng["detected"]
    return both, ref


def fork_rule_ids(base):
    try:
        with urllib.request.urlopen(base + "/rules", timeout=5) as response:
            return set(json.loads(response.read().decode("utf-8"))["rules"])
    except (urllib.error.URLError, OSError, ValueError, KeyError):
        return None


def lt_xml_rule_ids(directory):
    """Ids of the rules and rule groups defined in LanguageTool's XML files."""
    ids = set()
    for path in Path(directory).glob("*.xml"):
        ids.update(re.findall(r'<(?:rule|rulegroup)\b[^>]*\bid="([^"]+)"',
                              path.read_text(encoding="utf-8", errors="replace")))
    return ids


def missed(cases, results, fork_rules, lt_rules):
    """Errors LanguageTool detects and the fork does not, by LanguageTool rule.

    kind: "en el fork" (the fork has a rule with that id, it does not fire),
    "XML" (defined in LanguageTool's XML files: portable by updating the rules),
    "Java" (not in the XML files: coded in Java, or the speller), or "?" (unknown)."""
    rows = defaultdict(list)
    for case in cases:
        r = results[case["id"]]
        if "languagetool" not in r or "fork" not in r:
            continue
        for e_lt, e_fork in zip(r["languagetool"]["errors"], r["fork"]["errors"]):
            if e_lt["detected"] and not e_fork["detected"]:
                for rule in e_lt["rules"] or ["?"]:
                    rows[rule].append(case["id"])
    out = []
    for rule, ids in sorted(rows.items(), key=lambda kv: (-len(kv[1]), kv[0])):
        if fork_rules is not None and rule in fork_rules:
            kind = "en el fork"
        elif lt_rules is not None:
            kind = "XML" if rule in lt_rules else "Java"
        else:
            kind = "?"
        out.append({"rule": rule, "kind": kind, "cases": ids})
    return out


def pct(a, b):
    return f"{100 * a / b:.0f} %" if b else "—"


def print_report(cases, results, completions, engines, verbose, missing=None):
    names = [n for n in ("fork", "languagetool") if any(n in r for r in results.values())]
    summaries = {n: summarize(cases, results, n) for n in names}

    if verbose:
        print("\n## Detalle\n")
        for case in cases:
            print(f"{case['id']}: {case['text']}")
            for name in names:
                r = results[case["id"]].get(name)
                if r is None:
                    continue
                marks = []
                for e in r["errors"]:
                    mark = "✓" if e["corrected"] else ("~" if e["detected"] else "✗")
                    marks.append(f"{mark} {e['span']}→{e['fix']} {','.join(e['rules'])}")
                extra = [f"+{m['rule']}({m['replacements'][:1]})" for m in r["extra"]]
                print(f"   {name:<13} {' | '.join(marks + extra) or 'sin avisos'}")

    print("\n## Corrección\n")
    header = "| Medida | " + " | ".join(names) + " |"
    print(header)
    print("|" + "---|" * (len(names) + 1))

    def row(label, f):
        print(f"| {label} | " + " | ".join(f(summaries[n]) for n in names) + " |")

    row("Errores detectados", lambda s: f"{s['detected']}/{s['errors']} ({pct(s['detected'], s['errors'])})")
    row("Con la corrección esperada", lambda s: f"{s['corrected']}/{s['errors']} ({pct(s['corrected'], s['errors'])})")
    row(
        "Frases correctas con avisos (falsos positivos)",
        lambda s: f"{s['false_positive_sentences']}/{s['correct_sentences']}",
    )
    row("Avisos extra en frases con error", lambda s: str(s["extra_matches"]))

    if "languagetool" in names:
        both, ref = coverage(cases, results)
        print(f"\n**Cobertura del fork sobre LanguageTool:** {both}/{ref} ({pct(both, ref)}) "
              "de los errores que detecta LanguageTool.")

    print("\n### Por categoría (detectados / esperados)\n")
    cats = sorted({c["cat"] for c in cases if c["errors"]})
    print("| Categoría | " + " | ".join(names) + " |")
    print("|" + "---|" * (len(names) + 1))
    for cat in cats:
        cells = []
        for n in names:
            d, t = summaries[n]["by_category"].get(cat, [0, 0])
            cells.append(f"{d}/{t}")
        print(f"| {cat} | " + " | ".join(cells) + " |")

    if missing:
        print("\n### Lo que detecta LanguageTool y el fork no, por regla\n")
        print("| Regla de LanguageTool | Tipo | Casos |")
        print("|---|---|---|")
        for row in missing:
            print(f"| {row['rule']} | {row['kind']} | {', '.join(row['cases'])} |")
        kinds = defaultdict(int)
        for row in missing:
            kinds[row["kind"]] += len(row["cases"])
        print("\nTipos: " + ", ".join(f"{k}: {v}" for k, v in sorted(kinds.items()))
              + ". «XML» se puede recuperar actualizando las reglas (paso 3); «Java» hay que"
              " portarlo a mano o con reglas propias; «en el fork» es una regla que existe pero"
              " no se activa con esa frase.")

    if completions is not None:
        ok = sum(r["ok"] for r in completions.values())
        print(f"\n## Autocompletado\n\n{ok}/{len(completions)} casos correctos.")
        for cid, r in completions.items():
            if not r["ok"] or verbose:
                print(f"   {'✓' if r['ok'] else '✗'} {cid}: {r['words']} contexto={r['context']}")

    return summaries


def compare(cases, results, completions, baseline):
    """Regressions and improvements of the fork against the baseline."""
    regressions, improvements = [], []
    for case in cases:
        old = baseline.get("checks", {}).get(case["id"], {}).get("fork")
        new = results[case["id"]].get("fork")
        if old is None or new is None:
            continue
        for e_old, e_new in zip(old["errors"], new["errors"]):
            label = f"{case['id']} «{e_new['span']}»"
            if e_old["detected"] and not e_new["detected"]:
                regressions.append(f"{label}: ya no se detecta")
            elif not e_old["detected"] and e_new["detected"]:
                improvements.append(f"{label}: ahora se detecta")
            elif e_old["corrected"] and not e_new["corrected"]:
                regressions.append(f"{label}: ya no propone «{e_new['fix']}»")
            elif not e_old["corrected"] and e_new["corrected"]:
                improvements.append(f"{label}: ahora propone «{e_new['fix']}»")
        if not case["errors"] and not old["extra"] and new["extra"]:
            regressions.append(f"{case['id']}: nuevo falso positivo {[m['rule'] for m in new['extra']]}")
        if not case["errors"] and old["extra"] and not new["extra"]:
            improvements.append(f"{case['id']}: ya no hay falso positivo")
    for cid, r in (completions or {}).items():
        old = baseline.get("completions", {}).get(cid)
        if old is None:
            continue
        if old["ok"] and not r["ok"]:
            regressions.append(f"autocompletado {cid}: falla ({r['words']})")
        elif not old["ok"] and r["ok"]:
            improvements.append(f"autocompletado {cid}: ahora pasa")
    return regressions, improvements


def main():
    parser = argparse.ArgumentParser(description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter)
    parser.add_argument("--fork", default="http://localhost:8011", help="URL de nlprule-server")
    parser.add_argument("--lt", default="http://localhost:8010",
                        help="URL de LanguageTool, o 'none' para no consultarlo")
    parser.add_argument("--checks", default=HERE / "check.jsonl", type=Path)
    parser.add_argument("--completions", default=HERE / "complete.jsonl", type=Path)
    parser.add_argument("--baseline", default=HERE / "baseline.json", type=Path,
                        help="línea base con la que comparar (si existe)")
    parser.add_argument("--lt-rules", type=Path,
                        help="carpeta con grammar.xml y style.xml de LanguageTool (rules/es), para "
                             "distinguir reglas XML de reglas Java")
    parser.add_argument("--save", type=Path, help="guardar los resultados como JSON")
    parser.add_argument("--verbose", "-v", action="store_true")
    args = parser.parse_args()

    cases = load_jsonl(args.checks)
    for case in cases:  # validate spans early
        for error in case["errors"]:
            find_span(case["text"], error["span"], error.get("occ", 1))

    baseline = None
    if args.baseline and args.baseline.exists() and args.baseline != args.save:
        baseline = json.loads(args.baseline.read_text(encoding="utf-8"))

    engines = {}
    if not reachable(args.fork):
        sys.exit(f"nlprule-server no responde en {args.fork}")
    engines["fork"] = args.fork
    if args.lt != "none":
        if reachable(args.lt):
            engines["languagetool"] = args.lt
        else:
            print(f"aviso: LanguageTool no responde en {args.lt}; se usa la línea base si la hay",
                  file=sys.stderr)

    results = run_checks(cases, engines)
    if "languagetool" not in engines and baseline:
        for cid, r in baseline.get("checks", {}).items():
            if cid in results and "languagetool" in r:
                results[cid]["languagetool"] = r["languagetool"]

    completions = run_completions(load_jsonl(args.completions), args.fork) if args.completions else None

    lt_rules = lt_xml_rule_ids(args.lt_rules) if args.lt_rules else None
    missing = missed(cases, results, fork_rule_ids(args.fork), lt_rules)
    summaries = print_report(cases, results, completions, engines, args.verbose, missing)

    status = 0
    if baseline:
        regressions, improvements = compare(cases, results, completions, baseline)
        print(f"\n## Frente a la línea base ({args.baseline.name})\n")
        for line in improvements:
            print(f"   + {line}")
        for line in regressions:
            print(f"   - {line}")
        if not regressions and not improvements:
            print("   sin cambios")
        status = 1 if regressions else 0

    if args.save:
        lt_version = None
        if "languagetool" in engines:
            try:
                lt_version = post(engines["languagetool"] + "/v2/check",
                                  {"language": "es", "text": "Hola."})["software"]["version"]
            except Exception:
                pass
        both, ref = coverage(cases, results)
        args.save.write_text(
            json.dumps(
                {
                    "date": time.strftime("%Y-%m-%d"),
                    "languagetool_version": lt_version,
                    "summary": summaries,
                    "coverage": {"fork_and_lt": both, "lt": ref},
                    "missed": missing,
                    "checks": results,
                    "completions": completions,
                },
                ensure_ascii=False,
                indent=1,
            )
            + "\n",
            encoding="utf-8",
        )
        print(f"\nresultados guardados en {args.save}")

    sys.exit(status)


if __name__ == "__main__":
    main()
