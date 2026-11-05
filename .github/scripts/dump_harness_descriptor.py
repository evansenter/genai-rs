#!/usr/bin/env python3
"""Dumps the protobuf surface of an unpacked google-antigravity wheel.

Usage: dump_harness_descriptor.py <unpacked-wheel-root>

Prints one sorted line per message field and per enum value of every
`*_pb2.py` module under `<root>/google/antigravity/`, so two releases diff to
exactly what changed on the harness's wire: a new `HarnessSideTools` toggle
(outbound, and on by default unless written), a new enum value the bridge
does not model (inbound), a renamed or retyped field.

The wheel is untrusted input, so nothing from it is imported or executed. A
`_pb2.py` module registers its descriptor with
`AddSerializedFile(b'...')`; the bytes literal is read with `ast` and the
`FileDescriptorProto` decoded by the small wire-format reader below, which
also keeps this script free of a `protobuf` dependency.

Exits non-zero when no descriptor is found: an empty dump would diff as "no
change" forever, which is how a detector goes silently inert.
"""

import ast
import pathlib
import sys

# FieldDescriptorProto.Type, by number.
TYPES = {
    1: "double", 2: "float", 3: "int64", 4: "uint64", 5: "int32",
    6: "fixed64", 7: "fixed32", 8: "bool", 9: "string", 10: "group",
    11: "message", 12: "bytes", 13: "uint32", 14: "enum", 15: "sfixed32",
    16: "sfixed64", 17: "sint32", 18: "sint64",
}


def read_varint(data, pos):
    result = shift = 0
    while True:
        if pos >= len(data):
            raise ValueError("truncated varint")
        byte = data[pos]
        pos += 1
        result |= (byte & 0x7F) << shift
        if not byte & 0x80:
            return result, pos
        shift += 7


def fields(data):
    """Yields (number, wire_type, value) for each field of a message."""
    pos = 0
    while pos < len(data):
        key, pos = read_varint(data, pos)
        number, wire = key >> 3, key & 7
        if wire == 0:
            value, pos = read_varint(data, pos)
        elif wire == 1:
            value, pos = data[pos:pos + 8], pos + 8
        elif wire == 2:
            length, pos = read_varint(data, pos)
            value, pos = data[pos:pos + length], pos + length
        elif wire == 5:
            value, pos = data[pos:pos + 4], pos + 4
        else:
            raise ValueError("unsupported wire type %d" % wire)
        yield number, wire, value


def text(value):
    return value.decode("utf-8")


def walk_enum(data, prefix, out, file_name):
    name = ""
    values = []
    for number, _, value in fields(data):
        if number == 1:
            name = text(value)
        elif number == 2:
            value_name, value_number = "", 0
            for n, _, v in fields(value):
                if n == 1:
                    value_name = text(v)
                elif n == 2:
                    value_number = v if v < 1 << 31 else v - (1 << 64)
            values.append((value_name, value_number))
    full = prefix + name
    for value_name, value_number in values:
        out.append("%s: enum %s: %s = %d" % (file_name, full, value_name, value_number))


def walk_message(data, prefix, out, file_name):
    name = ""
    raw_fields, nested, enums, oneofs = [], [], [], []
    for number, _, value in fields(data):
        if number == 1:
            name = text(value)
        elif number == 2:
            raw_fields.append(value)
        elif number == 3:
            nested.append(value)
        elif number == 4:
            enums.append(value)
        elif number == 8:
            oneof_name = ""
            for n, _, v in fields(value):
                if n == 1:
                    oneof_name = text(v)
            oneofs.append(oneof_name)
    full = prefix + name
    out.append("%s: message %s" % (file_name, full))
    for raw in raw_fields:
        field = {"label": 1, "type": 0, "type_name": "", "oneof": None}
        for n, _, v in fields(raw):
            if n == 1:
                field["name"] = text(v)
            elif n == 3:
                field["number"] = v
            elif n == 4:
                field["label"] = v
            elif n == 5:
                field["type"] = v
            elif n == 6:
                field["type_name"] = text(v).lstrip(".")
            elif n == 9:
                field["oneof"] = v
        kind = field["type_name"] or TYPES.get(field["type"], "type%d" % field["type"])
        if field["label"] == 3:
            kind = "repeated " + kind
        if field["oneof"] is not None and field["oneof"] < len(oneofs):
            kind += " (oneof %s)" % oneofs[field["oneof"]]
        out.append("%s: message %s: %d %s: %s"
                   % (file_name, full, field.get("number", 0), field.get("name", "?"), kind))
    for value in nested:
        walk_message(value, full + ".", out, file_name)
    for value in enums:
        walk_enum(value, full + ".", out, file_name)


def dump_file_descriptor(data, out):
    file_name, package = "?", ""
    messages, enums = [], []
    for number, _, value in fields(data):
        if number == 1:
            file_name = text(value).rsplit("/", 1)[-1]
        elif number == 2:
            package = text(value)
        elif number == 4:
            messages.append(value)
        elif number == 5:
            enums.append(value)
    prefix = package + "." if package else ""
    for value in messages:
        walk_message(value, prefix, out, file_name)
    for value in enums:
        walk_enum(value, prefix, out, file_name)


def serialized_descriptors(source):
    """The bytes passed to AddSerializedFile(...) in a _pb2 module's source."""
    found = []
    for node in ast.walk(ast.parse(source)):
        if (isinstance(node, ast.Call)
                and isinstance(node.func, ast.Attribute)
                and node.func.attr == "AddSerializedFile"
                and node.args
                and isinstance(node.args[0], ast.Constant)
                and isinstance(node.args[0].value, bytes)):
            found.append(node.args[0].value)
    return found


def main():
    if len(sys.argv) != 2:
        sys.exit("usage: dump_harness_descriptor.py <unpacked-wheel-root>")
    package = pathlib.Path(sys.argv[1]) / "google" / "antigravity"
    if not package.is_dir():
        sys.exit("no google/antigravity package under %s" % sys.argv[1])
    out = []
    modules = sorted(package.rglob("*_pb2.py"))
    for module in modules:
        for data in serialized_descriptors(module.read_text(encoding="utf-8")):
            dump_file_descriptor(data, out)
    if not out:
        sys.exit("no protobuf descriptors found under %s (%d _pb2 modules); the wheel "
                 "layout or the generated-code format changed" % (package, len(modules)))
    sys.stdout.write("".join(line + "\n" for line in sorted(set(out))))


if __name__ == "__main__":
    main()
