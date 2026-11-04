#!/usr/bin/env python3
"""Print a normalized, sorted schema of google-genai's generated `_gaos` bindings.

Usage: dump_bindings_schema.py [--json] <path to google/genai/_gaos>

Diffing two releases' output shows API surface changes without the noise a
raw `diff -r` carries (docstring rewraps, import order, union member order,
lint headers). Parsed with `ast`, so no dependencies are imported.

Output lines:
  model <file>:<Class>.<wire name>: <annotation> [required] [deprecated]
  alias <file>:<Name> = <annotation>
  endpoint <METHOD> <path>          (path parameters normalized to {})

`--json` prints the same classes and endpoints with every type reference
resolved through the module's imports, for the offline coverage oracle
(`src/coverage_oracle/`, and the endpoint check in `tests/ci_coverage.rs`),
plus the Files, File Search Store and Document classes from the hand-written
`google/genai/types.py` next to `_gaos` (camelCase on the wire). The snapshot
lives in `tests/fixtures/api_surface/bindings.json`; see
`.github/scripts/refresh_api_snapshot.sh`.
"""

import ast
import json
import re
import sys
from pathlib import Path


def unwrap(node):
    """Returns (annotation without Annotated wrappers, wire alias, deprecated)."""
    alias, deprecated = None, False
    while isinstance(node, ast.Subscript):
        name = ast.unparse(node.value)
        if name.endswith("Annotated"):
            elts = node.slice.elts if isinstance(node.slice, ast.Tuple) else [node.slice]
            for meta in elts[1:]:
                for kw in getattr(meta, "keywords", []):
                    if kw.arg == "alias" and isinstance(kw.value, ast.Constant):
                        alias = kw.value.value
                    deprecated |= kw.arg == "deprecated"
            node = elts[0]
            continue
        break
    return node, alias, deprecated


def norm(node):
    """Normalizes an annotation: sorted unions/literals, Optional as a union."""
    node, _, _ = unwrap(node)
    if isinstance(node, ast.Subscript):
        head = ast.unparse(node.value).split(".")[-1]
        elts = node.slice.elts if isinstance(node.slice, ast.Tuple) else [node.slice]
        if head == "Optional":
            return norm_union([*elts, ast.Constant(None)])
        if head == "Union":
            return norm_union(elts)
        if head == "Literal":
            return "Literal[" + ", ".join(sorted(ast.unparse(e) for e in elts)) + "]"
        return f"{head}[" + ", ".join(norm(e) for e in elts) + "]"
    if isinstance(node, ast.BinOp) and isinstance(node.op, ast.BitOr):
        return norm_union([node.left, node.right])
    return ast.unparse(node)


def norm_union(elts):
    members = set()
    for e in elts:
        text = norm(e)
        if text.startswith("Union[") and text.endswith("]"):
            members.update(split_top(text[6:-1]))
        else:
            members.add(text)
    return "Union[" + ", ".join(sorted(members)) + "]"


def split_top(text):
    parts, depth, cur = [], 0, ""
    for ch in text:
        if ch == "," and depth == 0:
            parts.append(cur.strip())
            cur = ""
            continue
        depth += ch in "[("
        depth -= ch in "])"
        cur += ch
    if cur.strip():
        parts.append(cur.strip())
    return parts


def is_model(cls):
    return any(ast.unparse(b).split(".")[-1] == "BaseModel" for b in cls.bases)


def dump_types(root, out):
    for path in sorted((root / "types").rglob("*.py")):
        rel = path.relative_to(root).as_posix()
        tree = ast.parse(path.read_text(encoding="utf-8"))
        for stmt in tree.body:
            if isinstance(stmt, ast.ClassDef) and is_model(stmt):
                for item in stmt.body:
                    if isinstance(item, ast.AnnAssign) and isinstance(item.target, ast.Name):
                        _, alias, deprecated = unwrap(item.annotation)
                        wire = alias or item.target.id
                        flags = (" required" if item.value is None else "") + (
                            " deprecated" if deprecated else ""
                        )
                        out.add(f"model {rel}:{stmt.name}.{wire}: {norm(item.annotation)}{flags}")
            elif isinstance(stmt, ast.Assign) and len(stmt.targets) == 1:
                target = stmt.targets[0]
                if not isinstance(target, ast.Name) or target.id.endswith(("Param", "TypedDict")):
                    continue
                value = stmt.value
                if isinstance(value, ast.Call) and ast.unparse(value.func).endswith("TypeAliasType"):
                    value = value.args[1] if len(value.args) > 1 else None
                if isinstance(value, ast.Subscript) and ast.unparse(value.value).split(".")[-1] in (
                    "Union",
                    "Literal",
                    "Annotated",
                ):
                    out.add(f"alias {rel}:{target.id} = {norm(value)}")


ENDPOINT = re.compile(r'method="([A-Z]+)",\s*path="([^"]+)"')


def endpoints(root):
    """`METHOD /path` for every endpoint, path parameters normalized to {}."""
    return sorted(
        {
            f"{method} {re.sub(r'{[^}]*}', '{}', url)}"
            for path in sorted(root.glob("*.py"))
            for method, url in ENDPOINT.findall(path.read_text(encoding="utf-8"))
        }
    )


def dump_endpoints(root, out):
    for endpoint in endpoints(root):
        out.add(f"endpoint {endpoint}")


# ---------------------------------------------------------------------------
# Resolved model (--json), and the hand-written Files / File Search Store
# classes in google/genai/types.py (both modes).
# ---------------------------------------------------------------------------

# The hand-written classes that model the Files, File Search Store and
# Document request and response bodies, plus everything they reference. The
# request bodies are the `*Config` classes: google-genai's converters lift
# their fields to the top level of the JSON body.
LEGACY_ROOTS = (
    "File",
    "ListFilesResponse",
    "FileSearchStore",
    "ListFileSearchStoresResponse",
    "CreateFileSearchStoreConfig",
    "Document",
    "ListDocumentsResponse",
    "UploadToFileSearchStoreConfig",
    "ImportFileConfig",
)
# Client-side settings on those classes; never on the wire.
LEGACY_CLIENT_ONLY = {"http_options", "sdk_http_response", "should_return_http_response"}

SCALARS = {
    "str": {"kind": "str"},
    "int": {"kind": "int"},
    "float": {"kind": "float"},
    "bool": {"kind": "bool"},
    "bytes": {"kind": "bytes"},
    "Any": {"kind": "any"},
    "object": {"kind": "any"},
    "datetime": {"kind": "datetime"},
    "Base64EncodedString": {"kind": "bytes"},
}
# Speakeasy's open-enum marker: a literal union that also takes any string.
OPEN = "UnrecognizedStr"


def to_camel(name):
    head, *rest = name.split("_")
    return head + "".join(part[:1].upper() + part[1:] for part in rest)


def module_file(base):
    if base.with_suffix(".py").is_file():
        return base.with_suffix(".py")
    if (base / "__init__.py").is_file():
        return base / "__init__.py"
    return None


class Module:
    """One parsed source file: its classes, type aliases and imports."""

    def __init__(self, path):
        self.path = path
        tree = ast.parse(path.read_text(encoding="utf-8"))
        self.classes, self.assigns, self.imports = {}, {}, {}
        queue = list(tree.body)
        while queue:
            stmt = queue.pop(0)
            if isinstance(stmt, ast.If):  # `if TYPE_CHECKING:` re-exports
                queue.extend(stmt.body)
            elif isinstance(stmt, ast.ClassDef):
                self.classes[stmt.name] = stmt
            elif isinstance(stmt, ast.Assign) and len(stmt.targets) == 1:
                if isinstance(stmt.targets[0], ast.Name):
                    self.assigns[stmt.targets[0].id] = stmt.value
            elif isinstance(stmt, ast.ImportFrom) and stmt.level > 0:
                base = path.parent
                for _ in range(stmt.level - 1):
                    base = base.parent
                if stmt.module:
                    base = base.joinpath(*stmt.module.split("."))
                for alias in stmt.names:
                    local = alias.asname or alias.name
                    submodule = module_file(base / alias.name)
                    if submodule is not None:
                        self.imports[local] = ("module", submodule)
                    elif module_file(base) is not None:
                        self.imports[local] = ("symbol", module_file(base), alias.name)


class Resolver:
    """Turns annotations into JSON type trees with every reference resolved.

    A class becomes `{"kind": "ref", "name": "<namespace>.<Class>"}` and is
    emitted under `classes`; a union or literal alias is emitted under
    `aliases` the same way. The namespace is the directory under `types/`
    (`interactions`, `webhooks`, ...), or `legacy` for `google/genai/types.py`.
    """

    def __init__(self, root):
        self.types_root = root / "types"
        self.modules = {}
        self.classes, self.aliases, self.lines = {}, {}, set()
        self.pending = []

    def module(self, path):
        if path not in self.modules:
            self.modules[path] = Module(path)
        return self.modules[path]

    def namespace(self, path):
        if path.name == "types.py" and path.parent == self.types_root.parent.parent:
            return "legacy"
        rel = path.relative_to(self.types_root).parent.as_posix()
        return "types" if rel == "." else rel.replace("/", ".")

    def key(self, mod, name):
        return f"{self.namespace(mod.path)}.{name}"

    def lookup(self, mod, node, depth=0):
        """Resolves a Name/Attribute to ("class"|"alias", Module, name)."""
        if depth > 20:
            return None
        if isinstance(node, ast.Attribute) and isinstance(node.value, ast.Name):
            target = mod.imports.get(node.value.id)
            if target and target[0] == "module":
                return self.lookup_name(self.module(target[1]), node.attr, depth + 1)
            return None
        if isinstance(node, ast.Name):
            return self.lookup_name(mod, node.id, depth + 1)
        return None

    def lookup_name(self, mod, name, depth):
        if name in mod.classes:
            return ("class", mod, name)
        if name in mod.assigns:
            return ("alias", mod, name)
        target = mod.imports.get(name)
        if target and target[0] == "symbol":
            return self.lookup_name(self.module(target[1]), target[2], depth + 1)
        return None

    def ty(self, mod, node):
        node, _, _ = unwrap(node)
        if isinstance(node, ast.Constant):
            if node.value is None:
                return None
            if isinstance(node.value, str):
                return self.ty(mod, ast.parse(node.value, mode="eval").body)
        if isinstance(node, ast.BinOp) and isinstance(node.op, ast.BitOr):
            return self.union(mod, [node.left, node.right])
        if isinstance(node, ast.Subscript):
            head = ast.unparse(node.value).split(".")[-1]
            elts = node.slice.elts if isinstance(node.slice, ast.Tuple) else [node.slice]
            if head == "Optional":
                return self.union(mod, elts)
            if head == "Union":
                return self.union(mod, elts)
            if head in ("List", "list", "Sequence"):
                return {"kind": "list", "item": self.ty(mod, elts[0]) or {"kind": "any"}}
            if head in ("Dict", "dict", "Mapping"):
                return {"kind": "map", "value": self.ty(mod, elts[-1]) or {"kind": "any"}}
            if head == "Literal":
                return {"kind": "literal", "values": sorted(e.value for e in elts), "open": False}
            if head in ("IO", "PathLike"):
                return {"kind": "bytes"}
            raise SystemExit(f"{mod.path}: unhandled annotation {ast.unparse(node)}")
        if isinstance(node, (ast.Name, ast.Attribute)):
            last = ast.unparse(node).split(".")[-1]
            if last in SCALARS:
                return dict(SCALARS[last])
            if last == OPEN:
                return {"kind": "literal", "values": [], "open": True}
            found = self.lookup(mod, node)
            if found is None:
                raise SystemExit(f"{mod.path}: cannot resolve {ast.unparse(node)}")
            kind, owner, name = found
            if kind == "class":
                return self.class_ref(owner, name)
            return self.alias_ref(owner, name)
        raise SystemExit(f"{mod.path}: unhandled annotation {ast.unparse(node)}")

    def union(self, mod, elts):
        arms, literals, is_open = [], set(), False
        for elt in elts:
            arm = self.ty(mod, elt)
            if arm is None:
                continue
            stack = [arm]
            while stack:
                arm = stack.pop()
                if arm["kind"] == "union":
                    stack.extend(arm["arms"])
                elif arm["kind"] == "literal":
                    literals.update(arm["values"])
                    is_open |= arm["open"]
                elif arm not in arms:
                    arms.append(arm)
        if literals:
            arms.append({"kind": "literal", "values": sorted(literals), "open": is_open})
        elif is_open and {"kind": "str"} not in arms:
            arms.append({"kind": "str"})
        if len(arms) == 1:
            return arms[0]
        return {"kind": "union", "arms": sorted(arms, key=lambda a: json.dumps(a, sort_keys=True))}

    def is_unknown_fallback(self, cls):
        return any(
            isinstance(item, ast.AnnAssign)
            and isinstance(item.target, ast.Name)
            and item.target.id == "is_unknown"
            for item in cls.body
        )

    def class_ref(self, mod, name):
        cls = mod.classes[name]
        if self.is_unknown_fallback(cls):
            # The SDK's catch-all for unrecognized union arms, not API surface.
            return None
        bases = {ast.unparse(b).split(".")[-1] for b in cls.bases}
        key = self.key(mod, name)
        if bases & {"CaseInSensitiveEnum", "Enum", "StrEnum"}:
            values = sorted(
                stmt.value.value
                for stmt in cls.body
                if isinstance(stmt, ast.Assign) and isinstance(stmt.value, ast.Constant)
            )
            if self.namespace(mod.path) == "legacy":
                literal = ", ".join(repr(v) for v in values)
                self.lines.add(f"alias legacy/types.py:{name} = Literal[{literal}]")
            return {"kind": "literal", "values": values, "open": True}
        if key not in self.classes:
            self.classes[key] = None  # reserve first: classes can be recursive
            self.pending.append((mod, name, key))
        return {"kind": "ref", "name": key}

    def alias_ref(self, mod, name):
        value = mod.assigns[name]
        if isinstance(value, ast.Call) and ast.unparse(value.func).endswith("TypeAliasType"):
            value = value.args[1]
        value, _, _ = unwrap(value)
        if isinstance(value, ast.Subscript) and ast.unparse(value.value).split(".")[-1] in (
            "Union",
            "Literal",
        ):
            key = self.key(mod, name)
            if key not in self.aliases:
                self.aliases[key] = None
                self.aliases[key] = self.ty(mod, value)
            return {"kind": "ref", "name": key}
        return self.ty(mod, value)

    def build_class(self, mod, name, key):
        cls = mod.classes[name]
        legacy = self.namespace(mod.path) == "legacy"
        fields, is_open = {}, False
        for item in cls.body:
            if not (isinstance(item, ast.AnnAssign) and isinstance(item.target, ast.Name)):
                continue
            field = item.target.id
            if field == "__pydantic_extra__":
                is_open = True
                continue
            _, alias, deprecated = unwrap(item.annotation)
            if legacy:
                if field in LEGACY_CLIENT_ONLY:
                    continue
                wire = to_camel(field)
                required = False
                self.lines.add(f"model legacy/types.py:{name}.{wire}: {norm(item.annotation)}")
            else:
                wire = alias or field
                required = item.value is None
            ty = self.ty(mod, item.annotation) or {"kind": "any"}
            fields[wire] = {"type": ty, "required": required, "deprecated": deprecated}
        rel = mod.path.relative_to(self.types_root.parent.parent).as_posix()
        self.classes[key] = {"file": rel, "open": is_open, "fields": fields}

    def drain(self):
        while self.pending:
            self.build_class(*self.pending.pop())


def resolve_all(root):
    """The `_gaos` classes, and the legacy roots when `types.py` is present."""
    resolver = Resolver(root)
    for path in sorted((root / "types").rglob("*.py")):
        mod = resolver.module(path)
        for name, cls in mod.classes.items():
            if is_model(cls):
                resolver.class_ref(mod, name)
        # Union aliases too, referenced or not: some are request bodies
        # (`CredentialCreateParams`) that no class field names.
        for name, value in mod.assigns.items():
            if name.endswith(("Param", "TypedDict")):
                continue
            if isinstance(value, ast.Call) and ast.unparse(value.func).endswith("TypeAliasType"):
                value = value.args[1]
            if isinstance(value, ast.Subscript) and ast.unparse(value.value).split(".")[-1] in (
                "Union",
                "Literal",
                "Annotated",
            ):
                resolver.alias_ref(mod, name)
        resolver.drain()
    legacy = root.parent / "types.py"
    if legacy.is_file():
        mod = resolver.module(legacy)
        missing = [name for name in LEGACY_ROOTS if name not in mod.classes]
        if missing:
            sys.exit(f"{legacy}: classes not found: {', '.join(missing)}")
        for name in LEGACY_ROOTS:
            resolver.class_ref(mod, name)
        resolver.drain()
    else:
        print(f"warning: {legacy} not found; Files and File Search Store classes skipped",
              file=sys.stderr)
    return resolver


def sdk_version(root):
    version = root.parent / "version.py"
    if version.is_file():
        match = re.search(r"__version__\s*=\s*['\"]([^'\"]+)['\"]", version.read_text())
        if match:
            return match.group(1)
    return "unknown"


def compact(value):
    return json.dumps(value, sort_keys=True, separators=(", ", ": "))


def render_json(root, resolver):
    """Line-oriented JSON: one class field or alias per line, so it greps and diffs."""
    out = [
        "{",
        f'  "sdk_version": {compact(sdk_version(root))},',
        f'  "endpoints": {compact(endpoints(root))},',
        '  "aliases": {',
    ]
    aliases = sorted(resolver.aliases.items())
    for i, (key, ty) in enumerate(aliases):
        out.append(f"    {compact(key)}: {compact(ty)}" + ("," if i + 1 < len(aliases) else ""))
    out += ["  },", '  "classes": {']
    classes = sorted(resolver.classes.items())
    for i, (key, cls) in enumerate(classes):
        out.append(
            f'    {compact(key)}: {{"file": {compact(cls["file"])}, "open": {compact(cls["open"])}, "fields": {{'
        )
        fields = sorted(cls["fields"].items())
        for j, (wire, field) in enumerate(fields):
            out.append(f"      {compact(wire)}: {compact(field)}" + ("," if j + 1 < len(fields) else ""))
        out.append("    }}" + ("," if i + 1 < len(classes) else ""))
    out += ["  }", "}"]
    return "\n".join(out)


def main():
    args = sys.argv[1:]
    as_json = "--json" in args
    args = [a for a in args if a != "--json"]
    if len(args) != 1:
        sys.exit(__doc__)
    root = Path(args[0])
    if not (root / "types").is_dir():
        sys.exit(f"{root}/types not found: google-genai has restructured its bindings")
    resolver = resolve_all(root)
    if not resolver.classes:
        sys.exit(f"no BaseModel classes found under {root}/types: the parser is inert")
    if as_json:
        print(render_json(root, resolver))
        return
    out = set(resolver.lines)
    dump_types(root, out)
    dump_endpoints(root, out)
    if not any(line.startswith("model ") for line in out):
        sys.exit(f"no BaseModel classes found under {root}/types: the parser is inert")
    print("\n".join(sorted(out)))


if __name__ == "__main__":
    main()
