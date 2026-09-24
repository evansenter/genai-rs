#!/usr/bin/env python3
"""Print a normalized, sorted schema of google-genai's generated `_gaos` bindings.

Usage: dump_bindings_schema.py <path to google/genai/_gaos>

Diffing two releases' output shows API surface changes without the noise a
raw `diff -r` carries (docstring rewraps, import order, union member order,
lint headers). Parsed with `ast`, so no dependencies are imported.

Output lines:
  model <file>:<Class>.<wire name>: <annotation> [required] [deprecated]
  alias <file>:<Name> = <annotation>
  endpoint <METHOD> <path>          (path parameters normalized to {})
"""

import ast
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


def dump_endpoints(root, out):
    for path in sorted(root.glob("*.py")):
        for method, url in ENDPOINT.findall(path.read_text(encoding="utf-8")):
            out.add(f"endpoint {method} {re.sub(r'{[^}]*}', '{}', url)}")


def main():
    if len(sys.argv) != 2:
        sys.exit(__doc__)
    root = Path(sys.argv[1])
    if not (root / "types").is_dir():
        sys.exit(f"{root}/types not found: google-genai has restructured its bindings")
    out = set()
    dump_types(root, out)
    dump_endpoints(root, out)
    if not any(line.startswith("model ") for line in out):
        sys.exit(f"no BaseModel classes found under {root}/types: the parser is inert")
    print("\n".join(sorted(out)))


if __name__ == "__main__":
    main()
