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
  query <METHOD> <path>?<name>      (one per query parameter)
  revision <Api-Revision>

`--json` prints the same classes and endpoints with every type reference
resolved through the module's imports, for the offline coverage oracle
(`src/coverage_oracle/`, `tests/request_wire_oracle.rs`, and the endpoint
and Api-Revision checks in `tests/ci_coverage.rs`).

Both modes cover the hand-written Files, File Search Store, Document and
Operation surface next to `_gaos` too (`LEGACY_MODULES`, camelCase on the
wire): each module's endpoints, read from the methods that reach the Gemini
Developer API, and the classes in `google/genai/types.py` those methods'
signatures name (their return types, their `config`, and the `*OrDict`
arguments), with everything those reference. An endpoint's query parameters
are the `QueryParamMetadata` fields of its `_gaos/models/` request class, or
the `_query` keys its legacy converters set. The snapshot lives in
`tests/fixtures/api_surface/bindings.json`; see
`.github/scripts/refresh_api_snapshot.sh`.
"""

import ast
import json
import re
import sys
import warnings
from pathlib import Path

# Upstream docstrings carry invalid escapes (`"\\/"`), which `ast.parse`
# warns about on every run; the dump reads no docstring.
warnings.filterwarnings("ignore", category=SyntaxWarning)


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


# ---------------------------------------------------------------------------
# Endpoints (both modes): `METHOD /path`, path parameters normalized to {},
# and each one's query parameters.
# ---------------------------------------------------------------------------

# Every `_gaos` request, as text: the cross-check that the AST walk below
# missed none (a renamed `_build_request` would otherwise drop endpoints).
ENDPOINT = re.compile(r'method="([A-Z]+)",\s*path="([^"]+)"')
BUILD_REQUEST = ("_build_request", "_build_request_async")


def normalize_path(path):
    return re.sub(r"{[^}]*}", "{}", path)


def model_classes(root):
    """Every class under `_gaos/models/`, by name: the `*Request` classes
    carry each endpoint's path, query and header parameters."""
    classes = {}
    models = root / "models"
    for path in sorted(models.rglob("*.py")) if models.is_dir() else []:
        for stmt in ast.parse(path.read_text(encoding="utf-8")).body:
            if isinstance(stmt, ast.ClassDef):
                classes[stmt.name] = stmt
    return classes


def query_params(cls):
    """The wire names of a `*Request` class's `QueryParamMetadata` fields."""
    names = set()
    for item in cls.body:
        if (
            isinstance(item, ast.AnnAssign)
            and isinstance(item.target, ast.Name)
            and "QueryParamMetadata" in ast.unparse(item.annotation)
        ):
            _, alias, _ = unwrap(item.annotation)
            names.add(alias or item.target.id)
    return names


def request_models(fn, name):
    """The `models.<Class>` names assigned to the variable `name` in `fn`."""
    found = set()
    for node in ast.walk(fn):
        if isinstance(node, ast.Assign) and any(
            isinstance(t, ast.Name) and t.id == name for t in node.targets
        ):
            for ref in ast.walk(node.value):
                if (
                    isinstance(ref, ast.Attribute)
                    and isinstance(ref.value, ast.Name)
                    and ref.value.id == "models"
                ):
                    found.add(ref.attr)
    return found


def gaos_endpoints(root):
    """`{"METHOD /path": {query params}}` for every `_gaos` request: each
    `_build_request` call's method and path, and the query fields of the
    `models.*Request` its `request=` argument is built from."""
    models = model_classes(root)
    found, listed = {}, set()
    for path in sorted(root.glob("*.py")):
        text = path.read_text(encoding="utf-8")
        listed.update(f"{m} {normalize_path(u)}" for m, u in ENDPOINT.findall(text))
        for fn in ast.walk(ast.parse(text)):
            if not isinstance(fn, (ast.FunctionDef, ast.AsyncFunctionDef)):
                continue
            for call in ast.walk(fn):
                if not (
                    isinstance(call, ast.Call)
                    and isinstance(call.func, ast.Attribute)
                    and call.func.attr in BUILD_REQUEST
                ):
                    continue
                kw = {k.arg: k.value for k in call.keywords}
                method, url = kw.get("method"), kw.get("path")
                if not (isinstance(method, ast.Constant) and isinstance(url, ast.Constant)):
                    sys.exit(f"{path}:{call.lineno}: a request whose method or path is not a literal")
                query = set()
                if isinstance(kw.get("request"), ast.Name):
                    for name in request_models(fn, kw["request"].id):
                        if name not in models:
                            sys.exit(f"{path}:{call.lineno}: models.{name} is not under {root}/models")
                        query |= query_params(models[name])
                found.setdefault(f"{method.value} {normalize_path(url.value)}", set()).update(query)
    missed = sorted(listed - found.keys())
    if missed:
        sys.exit(f"{root}: requests not read from a _build_request call: {', '.join(missed)}")
    return found


def api_revision(root):
    """The `Api-Revision` the bindings pin: the one `*API_REVISION` string
    constant under `_gaos`."""
    values = set()
    for path in sorted(root.rglob("*.py")):
        for stmt in ast.parse(path.read_text(encoding="utf-8")).body:
            if (
                isinstance(stmt, ast.Assign)
                and len(stmt.targets) == 1
                and isinstance(stmt.targets[0], ast.Name)
                and stmt.targets[0].id.endswith("API_REVISION")
                and isinstance(stmt.value, ast.Constant)
                and isinstance(stmt.value.value, str)
            ):
                values.add(stmt.value.value)
    if len(values) != 1:
        sys.exit(f"{root}: expected one *API_REVISION string constant, found {sorted(values)}")
    return values.pop()


# The hand-written modules beside `_gaos` whose surface the crate covers. The
# rest of google/genai (models, caches, batches, tunings, live, ...) is the
# generateContent family, which this crate does not wrap.
LEGACY_MODULES = ("files", "file_search_stores", "documents", "operations")


def is_vertex_test(node):
    return isinstance(node, ast.Attribute) and node.attr == "vertexai"


def mldev_body(stmts):
    """The statements a Gemini Developer API client runs, with each
    `if self._api_client.vertexai:` branch resolved; None when the method
    raises for that client (it is Vertex-only)."""
    out = []
    for stmt in stmts:
        if isinstance(stmt, ast.If):
            test = stmt.test
            negated = isinstance(test, ast.UnaryOp) and isinstance(test.op, ast.Not)
            if is_vertex_test(test.operand if negated else test):
                branch = stmt.body if negated else stmt.orelse
                if len(branch) == 1 and isinstance(branch[0], ast.Raise):
                    return None
                inner = mldev_body(branch)
                if inner is None:
                    return None
                out.extend(inner)
                continue
        out.append(stmt)
    return out


def render_path(node):
    """The path template an assignment builds: a literal, a literal's
    `.format_map(...)`, or an f-string with each expression as `{expr}`."""
    if isinstance(node, ast.Call) and isinstance(node.func, ast.Attribute):
        if node.func.attr == "format_map":
            node = node.func.value
    if isinstance(node, ast.Constant) and isinstance(node.value, str):
        return node.value.replace("{{", "{").replace("}}", "}")
    if isinstance(node, ast.JoinedStr):
        parts = []
        for value in node.values:
            if isinstance(value, ast.Constant):
                parts.append(str(value.value))
            else:
                parts.append("{" + ast.unparse(value.value) + "}")
        return "".join(parts)
    return None


def converters(tree):
    """Each module-level `_*_to_mldev` converter: the `_query` keys it sets,
    the fields it routes into the URL or the query, and the converters it
    calls."""
    out = {}
    for fn in tree.body:
        if not (isinstance(fn, ast.FunctionDef) and fn.name.endswith("_to_mldev")):
            continue
        query, routed, calls = set(), set(), set()
        for call in ast.walk(fn):
            if not isinstance(call, ast.Call):
                continue
            name = ast.unparse(call.func)
            if name.endswith("_to_mldev"):
                calls.add(name.split(".")[-1])
            if name != "setv" or len(call.args) < 3 or not isinstance(call.args[1], ast.List):
                continue
            route = [e.value for e in call.args[1].elts if isinstance(e, ast.Constant)]
            if not route or route[0] not in ("_query", "_url"):
                continue
            if route[0] == "_query" and len(route) > 1:
                query.add(route[1])
            for get in ast.walk(call.args[2]):
                if (
                    isinstance(get, ast.Call)
                    and ast.unparse(get.func) == "getv"
                    and len(get.args) == 2
                    and isinstance(get.args[1], ast.List)
                ):
                    routed.update(e.value for e in get.args[1].elts if isinstance(e, ast.Constant))
        out[fn.name] = {"query": query, "routed": routed, "calls": calls}
    return out


def converter_query(convs, names):
    """The `_query` keys the named converters set, through the ones they call."""
    keys, seen, queue = set(), set(), list(names)
    while queue:
        name = queue.pop()
        if name in seen or name not in convs:
            continue
        seen.add(name)
        keys |= convs[name]["query"]
        queue.extend(convs[name]["calls"])
    return keys


def to_snake(name):
    return re.sub(r"(?<!^)(?=[A-Z])", "_", name).lower()


class LegacyClass:
    """One sync resource class of a legacy module (`Files`, `Documents`)."""

    def __init__(self, module, cls, convs, typevars):
        self.module, self.cls, self.name = module, cls, cls.name
        self.typevars = typevars
        self.calls = []  # (method name, HTTP method, raw path, query keys)
        self.signatures = []  # annotations naming the classes a method takes or returns
        for fn in cls.body:
            if not isinstance(fn, ast.FunctionDef):
                continue
            body = mldev_body(fn.body)
            if body is None:
                continue
            self.signatures += signature_roots(fn, typevars)
            self.read_calls(fn, body, convs)

    def read_calls(self, fn, body, convs):
        holder = ast.Module(body=body, type_ignores=[])
        paths, dict_query, called = [], set(), set()
        for node in ast.walk(holder):
            if isinstance(node, ast.Assign) and len(node.targets) == 1:
                target = node.targets[0]
                if isinstance(target, ast.Name) and target.id == "path":
                    rendered = render_path(node.value)
                    if rendered is not None and not rendered.startswith("{path}"):
                        paths.append(rendered)
                if (
                    isinstance(target, ast.Name)
                    and target.id == "query_params"
                    and isinstance(node.value, ast.Dict)
                ):
                    dict_query.update(k.value for k in node.value.keys if isinstance(k, ast.Constant))
            if isinstance(node, ast.Call) and isinstance(node.func, ast.Name):
                if node.func.id.endswith("_to_mldev"):
                    called.add(node.func.id)
        for node in ast.walk(holder):
            if not (
                isinstance(node, ast.Call)
                and isinstance(node.func, ast.Attribute)
                and isinstance(node.func.value, ast.Attribute)
                and node.func.value.attr == "_api_client"
            ):
                continue
            if node.func.attr == "request" and node.args and isinstance(node.args[0], ast.Constant):
                verb = node.args[0].value.upper()
            elif node.func.attr == "download_file":
                verb = "GET"
            else:
                continue  # upload_file goes to the session URL a start request returned
            if len(set(paths)) != 1:
                sys.exit(
                    f"{self.module}.{self.name}.{fn.name}: expected one request path, "
                    f"found {sorted(set(paths))}"
                )
            raw, _, literal_query = paths[0].partition("?")
            query = dict_query | converter_query(convs, called)
            query |= {pair.split("=")[0] for pair in literal_query.split("&") if pair}
            self.calls.append((fn.name, verb, raw, query))

    def accessors(self, names):
        """The legacy classes a method of this one returns (`documents()`)."""
        return {
            fn.returns.id
            for fn in self.cls.body
            if isinstance(fn, ast.FunctionDef)
            and isinstance(fn.returns, ast.Name)
            and fn.returns.id in names
        }


def signature_roots(fn, typevars):
    """The annotations of what a method returns and takes as request data:
    its return type, its `config`, any `*OrDict` argument and any TypeVar."""
    out = [fn.returns] if fn.returns is not None else []
    for arg in fn.args.args + fn.args.kwonlyargs:
        ann = arg.annotation
        if ann is None:
            continue
        text = ast.unparse(ann)
        if arg.arg == "config" or "OrDict" in text or text in typevars:
            out.append(ann)
    return out


def type_names(ann, typevars):
    """The `types.<Class>` names an annotation mentions, `XOrDict` read as
    `X` and a TypeVar as its bound."""
    names = set()
    for node in ast.walk(ann):
        if isinstance(node, ast.Name) and node.id in typevars:
            names |= type_names(typevars[node.id], {})
        if isinstance(node, ast.Attribute) and isinstance(node.value, ast.Name):
            if node.value.id == "types":
                name = node.attr
                names.add(name[: -len("OrDict")] if name.endswith("OrDict") else name)
    return names


def read_legacy(genai):
    """The legacy modules' sync resource classes, and their converters."""
    classes, convs = {}, {}
    for module in LEGACY_MODULES:
        path = genai / f"{module}.py"
        if not path.is_file():
            sys.exit(f"{path} not found: google-genai has restructured its {module} module")
        tree = ast.parse(path.read_text(encoding="utf-8"))
        module_convs = converters(tree)
        convs.update(module_convs)
        for cls in tree.body:
            if not isinstance(cls, ast.ClassDef) or cls.name.startswith("Async"):
                continue
            if not any(ast.unparse(b).endswith("BaseModule") for b in cls.bases):
                continue
            typevars = {}
            for stmt in tree.body + cls.body:
                if (
                    isinstance(stmt, ast.Assign)
                    and isinstance(stmt.value, ast.Call)
                    and ast.unparse(stmt.value.func).endswith("TypeVar")
                ):
                    bound = next((k.value for k in stmt.value.keywords if k.arg == "bound"), None)
                    if bound is not None:
                        typevars[stmt.targets[0].id] = bound
            classes[cls.name] = LegacyClass(module, cls, module_convs, typevars)
        if not any(c.module == module for c in classes.values()):
            sys.exit(f"{path}: no resource class (a BaseModule subclass) found")
    return classes, convs


def legacy_endpoints(classes):
    """`{"METHOD /path": {query keys}}` for the legacy modules.

    A legacy path names a resource by its full name (`{name}`, `{parent}`),
    which the crate's URLs spell out. A leading `{name}` or `{<resource>_name}`
    is expanded to the class's collection path (its `_list` path) plus `/{}`,
    and `{parent}` to the item of the class whose accessor returns this one
    (`FileSearchStores.documents()`), so `{name}` in `Documents` reads
    `fileSearchStores/{}/documents/{}`. A leading parameter that names
    nothing it can expand stays named (`{operationName}`): normalized to {}
    it would collide with another. Every other parameter becomes {}.
    """
    parent = {}
    for owner in classes.values():
        for child in owner.accessors(classes):
            parent[child] = owner.name

    def collection(name):
        lists = [raw for method, verb, raw, _ in classes[name].calls if method == "_list"]
        return expand(lists[0], name) if lists else None

    def item(name):
        base = collection(name)
        return None if base is None else base + "/{}"

    def expand(raw, name):
        match = re.match(r"{([^}]*)}", raw)
        if match:
            key = match.group(1)
            singular = to_snake(name).rstrip("s")
            if key == "parent" and name in parent:
                head = item(parent[name])
            elif key in ("name", f"{singular}_name"):
                head = item(name)
            else:
                head = None
            if head is not None:
                return head + normalize_path(raw[match.end():])
            return match.group(0) + normalize_path(raw[match.end():])
        return normalize_path(raw)

    found = {}
    for name, cls in classes.items():
        for _, verb, raw, query in cls.calls:
            prefix = "/{}/"
            if raw.startswith("upload/{api_version}/"):
                prefix, raw = "/upload/{}/", raw[len("upload/{api_version}/"):]
            found.setdefault(f"{verb} {prefix}{expand(raw, name)}", set()).update(query)
    if not found:
        sys.exit("no legacy endpoints found: google-genai has restructured its modules")
    return found


def endpoints(root):
    """Every endpoint, with its query parameters: `_gaos`, and the legacy
    modules when `google/genai` has them."""
    found = gaos_endpoints(root)
    if (root.parent / "types.py").is_file():
        classes, _ = read_legacy(root.parent)
        for key, query in legacy_endpoints(classes).items():
            found.setdefault(key, set()).update(query)
    return found


def dump_endpoints(root, out):
    for endpoint, query in endpoints(root).items():
        out.add(f"endpoint {endpoint}")
        out.update(f"query {endpoint}?{name}" for name in query)
    out.add(f"revision {api_revision(root)}")


# ---------------------------------------------------------------------------
# Resolved model (--json), and the hand-written Files / File Search Store
# classes in google/genai/types.py (both modes).
# ---------------------------------------------------------------------------

# The hand-written classes that model the Files, File Search Store, Document
# and Operation request and response bodies are the ones the legacy methods'
# signatures name (`legacy_roots`), plus everything they reference. The
# request bodies are the `*Config` classes: google-genai's converters lift
# their fields to the top level of the JSON body, all but the ones they route
# into the URL or the query string, which are the endpoint's parameters.
# Client-side settings on those classes; never on the wire.
LEGACY_CLIENT_ONLY = {"http_options", "sdk_http_response", "should_return_http_response"}
# Bases that add no fields of their own.
FRAMEWORK_BASES = {"BaseModel", "ABC", "Generic", "Enum", "CaseInSensitiveEnum", "StrEnum"}

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
        # Legacy class name -> the fields its converter routes into the URL
        # or the query string (`ListFilesConfig.page_size`).
        self.routed = {}

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

    def items(self, mod, name, depth=0):
        """A class's annotated fields, its bases' first (a legacy class can
        inherit them: `ImportFileOperation(_common.BaseModel, Operation)`)."""
        cls = mod.classes[name]
        out = []
        for base in cls.bases:
            base_name = ast.unparse(base)
            if base_name in mod.classes and base_name not in FRAMEWORK_BASES and depth < 10:
                out += self.items(mod, base_name, depth + 1)
        out += [
            item
            for item in cls.body
            if isinstance(item, ast.AnnAssign) and isinstance(item.target, ast.Name)
        ]
        return out

    def legacy_wire_fields(self, mod, name):
        """A legacy class's fields that reach the JSON body."""
        skip = LEGACY_CLIENT_ONLY | self.routed.get(name, set())
        return [item for item in self.items(mod, name) if item.target.id not in skip]

    def build_class(self, mod, name, key):
        legacy = self.namespace(mod.path) == "legacy"
        fields, is_open = {}, False
        items = self.legacy_wire_fields(mod, name) if legacy else self.items(mod, name)
        for item in items:
            field = item.target.id
            if field == "__pydantic_extra__":
                is_open = True
                continue
            _, alias, deprecated = unwrap(item.annotation)
            if legacy:
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
        classes, convs = read_legacy(root.parent)
        for conv, info in convs.items():
            resolver.routed.setdefault(conv.strip("_").removesuffix("_to_mldev"), set()).update(
                info["routed"]
            )
        for name in legacy_roots(classes, mod):
            # A root with no wire field (`GetFileConfig`: only `http_options`)
            # carries nothing to check; it becomes one when a field arrives.
            if resolver.legacy_wire_fields(mod, name):
                resolver.class_ref(mod, name)
        resolver.drain()
    else:
        print(f"warning: {legacy} not found; Files and File Search Store classes skipped",
              file=sys.stderr)
    return resolver


def legacy_roots(classes, mod):
    """The `types.py` classes the legacy methods' signatures name."""
    names = set()
    for cls in classes.values():
        for ann in cls.signatures:
            names |= type_names(ann, cls.typevars)
    unknown = sorted(n for n in names if n not in mod.classes and n not in mod.assigns)
    if unknown:
        sys.exit(f"{mod.path}: signature types not found: {', '.join(unknown)}")
    roots = sorted(n for n in names if n in mod.classes and not n.endswith("Dict"))
    if not roots:
        sys.exit(f"{mod.path}: the legacy methods' signatures name no class")
    return roots


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
    """Line-oriented JSON: one endpoint, class field or alias per line, so it
    greps and diffs."""
    out = [
        "{",
        f'  "sdk_version": {compact(sdk_version(root))},',
        f'  "api_revision": {compact(api_revision(root))},',
        '  "endpoints": {',
    ]
    found = sorted(endpoints(root).items())
    for i, (key, query) in enumerate(found):
        entry = compact({"query": sorted(query)})
        out.append(f"    {compact(key)}: {entry}" + ("," if i + 1 < len(found) else ""))
    out += ["  },", '  "aliases": {']
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
