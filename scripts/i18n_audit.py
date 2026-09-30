"""Auditoría de textos de la interfaz para el castellano.

Uso:
  python scripts/i18n_audit.py pick   -> frases inglesas de pick() sin traducción es-ES
  python scripts/i18n_audit.py cjk    -> literales con chino fuera de pick()/tests/comentarios
  python scripts/i18n_audit.py pick --json  -> igual, en JSON {"English": ""} para traducir

Salida vacía = nada pendiente. Omite los módulos #[cfg(test)] y los directorios tests/.
"""
import json
import re
import sys
from pathlib import Path

ROOT = Path(__file__).resolve().parent.parent
I18N = ROOT / "nebula_app" / "i18n"
CJK = re.compile(r"[\u3400-\u9fff\uff00-\uffef\u3000-\u303f]")
SOURCES = [ROOT / name for name in ("nebula_app/src", "nebula_settings/src", "nebula_terminal/src",
                                     "nebula_split/src", "nebula_gpui/src", "nebula_hook/src",
                                     "nebula-completions/src")]


def tokens(text):
    """Tokeniza lo justo de Rust: literales de cadena, identificadores y puntuación."""
    i, line, n = 0, 1, len(text)
    while i < n:
        c = text[i]
        if c == "\n":
            line += 1
            i += 1
        elif text.startswith("//", i):
            i = text.find("\n", i)
            i = n if i < 0 else i
        elif text.startswith("/*", i):
            end = text.find("*/", i + 2)
            end = n if end < 0 else end + 2
            line += text.count("\n", i, end)
            i = end
        elif c == "r" and re.match(r'r#*"', text[i:i + 12]) and (i == 0 or not text[i - 1].isalnum()):
            hashes = len(re.match(r"r(#*)", text[i:]).group(1))
            start = i + 2 + hashes
            end = text.find('"' + "#" * hashes, start)
            yield "str", text[start:end], line
            line += text.count("\n", i, end)
            i = end + 1 + hashes
        elif c == '"' or (c == "b" and text.startswith('b"', i)):
            start = i + (2 if c == "b" else 1)
            j = start
            while text[j] != '"':
                j += 2 if text[j] == "\\" else 1
            yield "str", decode(text[start:j]), line
            line += text.count("\n", i, j)
            i = j + 1
        elif c == "'":
            m = re.match(r"'(\\.|\\u\{[0-9a-fA-F]+\}|[^\\'])'", text[i:])
            i += len(m.group(0)) if m else 1
        elif c.isalnum() or c == "_":
            m = re.match(r"\w+", text[i:])
            yield "ident", m.group(0), line
            i += len(m.group(0))
        else:
            if not c.isspace():
                yield "punct", c, line
            i += 1


def decode(raw):
    raw = re.sub(r"\\\n\s*", "", raw)
    simple = {"n": "\n", "t": "\t", "r": "\r", "0": "\0", "\\": "\\", '"': '"', "'": "'"}

    def one(m):
        esc = m.group(1)
        if esc.startswith("u{"):
            return chr(int(esc[2:-1], 16))
        if esc.startswith("x"):
            return chr(int(esc[1:], 16))
        return simple.get(esc, esc)
    return re.sub(r'\\(u\{[0-9a-fA-F]+\}|x[0-9a-fA-F]{2}|.)', one, raw)


def block_end(text, open_brace):
    """Índice justo después de la llave que cierra `text[open_brace]`, saltando
    cadenas, caracteres y comentarios."""
    depth, i, n = 0, open_brace, len(text)
    while i < n:
        c = text[i]
        if text.startswith("//", i):
            i = text.find("\n", i)
            i = n if i < 0 else i
            continue
        if text.startswith("/*", i):
            i = text.find("*/", i + 2)
            i = n if i < 0 else i + 2
            continue
        raw = re.match(r'r(#*)"', text[i:i + 12]) if c == "r" and not text[i - 1].isalnum() else None
        if raw:
            end = text.find('"' + raw.group(1), i + len(raw.group(0)))
            i = n if end < 0 else end + 1 + len(raw.group(1))
            continue
        if c == '"':
            i += 1
            while i < n and text[i] != '"':
                i += 2 if text[i] == "\\" else 1
            i += 1
            continue
        if c == "'":
            m = re.match(r"'(\\.|\\u\{[0-9a-fA-F]+\}|[^\\'])'", text[i:])
            i += len(m.group(0)) if m else 1
            continue
        if c == "{":
            depth += 1
        elif c == "}":
            depth -= 1
            if depth == 0:
                return i + 1
        i += 1
    return n


def production_source(path):
    """El código sin los módulos `#[cfg(test)] mod x { … }`, estén donde estén."""
    text = path.read_text(encoding="utf-8")
    pattern = re.compile(r"^[ \t]*#\[cfg\(test\)\]\s*\n\s*(pub(\(\w+\))? )?mod \w+\s*\{", re.M)
    while (found := pattern.search(text)):
        end = block_end(text, found.end() - 1)
        # Se conservan los saltos de línea para que los números de línea no cambien.
        text = text[:found.start()] + "\n" * text.count("\n", found.start(), end) + text[end:]
    return text


def files():
    for root in SOURCES:
        for path in root.rglob("*.rs"):
            parts = set(path.relative_to(ROOT).parts)
            if "tests" in parts or path.name in ("tests.rs", "test.rs") or path.stem.endswith("_tests"):
                continue
            yield path


def scan():
    """Devuelve (picks, raw_cjk): picks = [(en, zh, loc)], raw_cjk = [(texto, loc)]."""
    picks, raw = [], []
    for path in files():
        toks = list(tokens(production_source(path)))
        rel = path.relative_to(ROOT).as_posix()
        in_pick = set()
        for k in range(len(toks) - 5):
            if toks[k][1] == "pick" and toks[k + 1][1] == "(" and toks[k + 2][0] == "str" \
                    and toks[k + 3][1] == "," and toks[k + 4][0] == "str":
                picks.append((toks[k + 4][1], toks[k + 2][1], f"{rel}:{toks[k][2]}"))
                in_pick.add(k + 2)
        for k, (kind, value, line) in enumerate(toks):
            if kind == "str" and k not in in_pick and CJK.search(value):
                raw.append((value, f"{rel}:{line}"))
    return picks, raw


def catalog_values(locale):
    def flatten(tree):
        for value in tree.values():
            yield from flatten(value) if isinstance(value, dict) else [value]
    return set(flatten(json.loads((I18N / f"{locale}.json").read_text(encoding="utf-8"))))


def phrases():
    path = I18N / "es-ES.phrases.json"
    return json.loads(path.read_text(encoding="utf-8")) if path.exists() else {}


def untranslated_picks(picks):
    known = catalog_values("en-US") | set(phrases())
    pending = {}
    for en, zh, loc in picks:
        if en not in known and en.strip():
            pending.setdefault(en, (zh, loc))
    return pending


if __name__ == "__main__":
    sys.stdout.reconfigure(encoding="utf-8")
    picks, raw = scan()
    if sys.argv[1] == "pick":
        pending = untranslated_picks(picks)
        if "--json" in sys.argv:
            print(json.dumps({en: {"zh": zh, "at": loc} for en, (zh, loc) in pending.items()},
                             ensure_ascii=False, indent=1))
        else:
            for en, (_, loc) in pending.items():
                print(f"{loc}\t{en!r}")
    else:
        for value, loc in raw:
            print(f"{loc}\t{value!r}")
