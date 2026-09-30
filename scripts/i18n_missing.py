"""Lista las claves de en-US que faltan (o son idénticas al inglés) en un catálogo.

Uso: python scripts/i18n_missing.py es-ES [--json]
Salida vacía = catálogo completo.
"""
import json
import sys
from pathlib import Path

I18N = Path(__file__).resolve().parent.parent / "nebula_app" / "i18n"


def flatten(tree, prefix=""):
    for key, value in tree.items():
        path = f"{prefix}.{key}" if prefix else key
        if isinstance(value, dict):
            yield from flatten(value, path)
        else:
            yield path, value


def missing(locale):
    english = dict(flatten(json.loads((I18N / "en-US.json").read_text(encoding="utf-8"))))
    target = dict(flatten(json.loads((I18N / f"{locale}.json").read_text(encoding="utf-8"))))
    return {k: v for k, v in english.items() if target.get(k, v) == v}


if __name__ == "__main__":
    sys.stdout.reconfigure(encoding="utf-8")
    result = missing(sys.argv[1])
    if "--json" in sys.argv:
        print(json.dumps(result, ensure_ascii=False, indent=1))
    else:
        for key, value in result.items():
            print(f"{key}\t{value}")
