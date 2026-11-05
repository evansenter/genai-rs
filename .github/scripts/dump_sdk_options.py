#!/usr/bin/env python3
"""Lists the options the google-antigravity Python SDK exposes for the harness.

Usage: dump_sdk_options.py <unpacked-wheel-root>

Prints one sorted key per line: `Class.field` for every field of the SDK's
configuration models, `Enum.MEMBER` for the enums they take, `Class.method`
for presets and agent methods, and `module.name` for the hook, policy and
trigger constructors. `docs/ANTIGRAVITY_BRIDGE_DESIGN.md`'s parity table
lists every key, modeled or not (with the reason), and
`test_python_sdk_options_are_all_in_the_parity_table` in
`tests/antigravity_scripted.rs` diffs the two, so a wheel bump that adds an
option fails until the table says what this crate does with it.

The wheel is untrusted input, so nothing from it is imported or executed:
the sources are read with `ast`, as `dump_harness_descriptor.py` reads the
protobuf descriptors.

Exits non-zero when a listed source file or class is missing: a dump that
silently shrank would read as "no new options" forever.
"""

import ast
import pathlib
import sys

# What to read from each source file under google/antigravity/.
SPEC = {
    "connections/connection.py": {
        "fields": ["AgentConfig", "DebugConfig"],
        "methods": ["AgentConfig"],
    },
    "connections/local/local_connection_config.py": {
        "fields": ["BaseLocalAgentConfig", "LocalAgentConfig"],
    },
    "connections/local/local_openai_connection_config.py": {
        "fields": ["LocalOpenAIAgentConfig"],
    },
    "connections/local/litert_connection_config.py": {
        "fields": ["LiteRTAgentConfig"],
    },
    "models.py": {
        "fields": [
            "ModelEndpoint", "GeminiModelOptions", "GeminiAPIEndpoint",
            "VertexEndpoint", "ModelTarget",
        ],
        "enums": ["ThinkingLevel", "ServiceTier", "ModelType"],
    },
    "types.py": {
        "fields": [
            "SystemInstructionSection", "CustomSystemInstructions",
            "TemplatedSystemInstructions", "ToolOutputTruncationConfig",
            "RunCommandConfig", "SubagentCapabilities", "InlineSkill",
            "SubagentInheritSkillsConfig", "SubagentOverrideSkillsConfig",
            "SubagentSkillsConfig", "SubagentConfig", "CapabilitiesConfig",
            "CompactionConfig", "ModelAPIRetryConfig", "ModelOutputRetryConfig",
            "RetryConfig", "BaseMcpServerConfig", "McpStdioServer",
            "McpStreamableHttpServer", "BudgetConfig", "HookResult",
            "QuestionResponse", "QuestionHookResult", "StopHookResult",
        ],
        "enums": [
            "AgentBehavior", "BuiltinTools", "BudgetScope",
            "SessionContinuationMode", "StopDecision",
        ],
        "methods": ["BuiltinTools", "RetryConfig"],
    },
    "hooks/hooks.py": {"decorators": "hooks"},
    "hooks/policy.py": {
        "functions": "policy",
        "fields": ["Policy", "AutoPolicy"],
    },
    "triggers/helpers.py": {"functions": "triggers"},
    "triggers/triggers.py": {"functions": "triggers", "methods": ["TriggerContext"]},
    "agent.py": {"methods": ["Agent"]},
}


# Public methods that are SDK plumbing, not options: the abstract strategy
# factory and pydantic's copy override.
SKIP = {"AgentConfig.create_strategy", "AgentConfig.model_copy"}


def classes(tree):
    return {node.name: node for node in tree.body if isinstance(node, ast.ClassDef)}


def fields(cls):
    """Class-level annotated assignments: pydantic and dataclass fields."""
    names = []
    for node in cls.body:
        if isinstance(node, ast.AnnAssign) and isinstance(node.target, ast.Name):
            name = node.target.id
            if not name.startswith("_") and name != "model_config":
                names.append(name)
    return names


def enum_members(cls):
    names = []
    for node in cls.body:
        if isinstance(node, ast.Assign):
            for target in node.targets:
                if isinstance(target, ast.Name) and target.id.isupper():
                    names.append(target.id)
    return names


def methods(cls):
    return [
        node.name
        for node in cls.body
        if isinstance(node, (ast.FunctionDef, ast.AsyncFunctionDef))
        and not node.name.startswith("_")
    ]


def functions(tree):
    return sorted({
        node.name
        for node in tree.body
        if isinstance(node, (ast.FunctionDef, ast.AsyncFunctionDef))
        and not node.name.startswith("_")
    })


def decorators(tree):
    """Module-level `name = _make_hook_decorator(...)` assignments."""
    names = []
    for node in tree.body:
        if (isinstance(node, ast.Assign)
                and isinstance(node.value, ast.Call)
                and isinstance(node.value.func, ast.Name)
                and node.value.func.id == "_make_hook_decorator"):
            for target in node.targets:
                if isinstance(target, ast.Name) and not target.id.startswith("_"):
                    names.append(target.id)
    return names


def main():
    if len(sys.argv) != 2:
        sys.exit("usage: dump_sdk_options.py <unpacked-wheel-root>")
    package = pathlib.Path(sys.argv[1]) / "google" / "antigravity"
    if not package.is_dir():
        sys.exit("no google/antigravity package under %s" % sys.argv[1])
    keys = set()
    for relative, spec in SPEC.items():
        path = package / relative
        if not path.is_file():
            sys.exit("missing %s: the wheel layout changed; update SPEC" % path)
        tree = ast.parse(path.read_text(encoding="utf-8"))
        found = classes(tree)
        for kind in ("fields", "enums", "methods"):
            for name in spec.get(kind, []):
                if name not in found:
                    sys.exit("class %s is gone from %s; update SPEC" % (name, relative))
                members = {
                    "fields": fields,
                    "enums": enum_members,
                    "methods": methods,
                }[kind](found[name])
                keys.update("%s.%s" % (name, member) for member in members)
        if "functions" in spec:
            keys.update("%s.%s" % (spec["functions"], f) for f in functions(tree))
        if "decorators" in spec:
            keys.update("%s.%s" % (spec["decorators"], d) for d in decorators(tree))
    keys -= SKIP
    if len(keys) < 50:
        sys.exit("only %d options found; the extraction is broken" % len(keys))
    sys.stdout.write("".join(key + "\n" for key in sorted(keys)))


if __name__ == "__main__":
    main()
