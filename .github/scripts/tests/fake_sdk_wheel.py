#!/usr/bin/env python3
"""Writes a synthetic google-antigravity Python SDK for the dump harnesses.

Usage: fake_sdk_wheel.py <dump_sdk_options.py> <out-dir> [Class.field ...]

Lays out `<out-dir>/google/antigravity/` from the dump script's own tables,
so the result places every name and dumps cleanly whatever SPEC holds:

- each SPEC module, with its classes (one field `option`, enum member
  `MEMBER` or method `method` each, as SPEC reads them), its aliases, a
  public function `function` where SPEC reads functions, and a hook
  decorator `on_event` where it reads decorators;
- each CLASS_SKIP class, and each MODULE_SKIP module with the names
  EXPORT_SKIP lists for it;
- a top-level `__init__.py` whose `__all__` exports one class of every
  SPEC module and the `beta` subpackage, and a `connections/local/__init__.py`
  that re-exports the EXPORT_SKIP names without an `__all__`, as the real
  wheel does.

Each `Class.field` argument adds that field to the class, the way a new
release adds an option. The harnesses append further code to the files to
plant what the dump must refuse.
"""

import importlib.util
import pathlib
import sys
from collections import defaultdict


def main():
    if len(sys.argv) < 3:
        sys.exit("usage: fake_sdk_wheel.py <dump_sdk_options.py> <out-dir> [Class.field ...]")
    spec = importlib.util.spec_from_file_location("dump_sdk_options", sys.argv[1])
    dump = importlib.util.module_from_spec(spec)
    spec.loader.exec_module(dump)
    root = pathlib.Path(sys.argv[2]) / "google" / "antigravity"
    extra_fields = defaultdict(list)
    for arg in sys.argv[3:]:
        cls, field = arg.split(".", 1)
        extra_fields[cls].append(field)

    sources = defaultdict(list)
    exports = []
    for relative, entry in dump.SPEC.items():
        body = defaultdict(list)
        for name in entry.get("fields", []):
            body[name].append("option: int = 0")
        for name in entry.get("enums", []):
            body[name].append("MEMBER = 1")
        for name in entry.get("methods", []):
            body[name].append("def method(self):\n    pass")
        for name in entry.get("names", []):
            body[name]
        for name in dump.CLASS_SKIP.get(relative, ()):
            body[name]
        lines = sources[relative]
        for name, members in body.items():
            members = members + ["%s: int = 0" % f for f in extra_fields.get(name, [])]
            lines.append("class %s:\n    %s\n" % (
                name, "\n    ".join(m.replace("\n", "\n    ") for m in members) or "pass"))
        for name in entry.get("aliases", []):
            lines.append("%s = int\n" % name)
        if "functions" in entry:
            lines.append("def function():\n    pass\n")
        if "decorators" in entry:
            lines.append("def _make_hook_decorator(kind):\n    return kind\n")
            lines.append("on_event = _make_hook_decorator('event')\n")
        public = sorted(n for n in body if not n.startswith("_"))
        if public:
            module = "google.antigravity." + relative[:-3].replace("/", ".")
            lines.insert(0, "from google.antigravity import beta as _beta_lib\n")
            exports.append((module, public[0]))
    for relative in dump.MODULE_SKIP:
        names = sorted(dump.EXPORT_SKIP.get(relative, ()))
        sources[relative] += ["class %s:\n    pass\n" % n for n in names]

    init = ["from %s import %s\n" % export for export in exports]
    init.append("from google.antigravity import beta\n")
    init.append("__all__ = %r\n" % ([name for _, name in exports] + ["beta"]))
    sources["__init__.py"] = init
    local = "connections/local/__init__.py"
    sources[local] = [
        "from google.antigravity.%s import %s\n"
        % (relative[:-3].replace("/", "."), name)
        for relative, names in sorted(dump.EXPORT_SKIP.items())
        for name in sorted(names)
    ]
    for relative, lines in sources.items():
        path = root / relative
        path.parent.mkdir(parents=True, exist_ok=True)
        path.write_text("\n".join(lines) + "\n", encoding="utf-8")


if __name__ == "__main__":
    main()
