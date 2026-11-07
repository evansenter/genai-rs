#!/usr/bin/env python3
"""Lists the options the google-antigravity Python SDK exposes for the harness.

Usage: dump_sdk_options.py [--lenient] <unpacked-wheel-root>

Prints one sorted key per line: `Class.field` for every field of the SDK's
configuration models and prompt types, `Enum.MEMBER` for the enums they
take, `Class.method` for presets and the agent, conversation, response and
tool-context methods, `Class` for classes that carry nothing of their own
(the media types), and `module.name` for the public functions of the hook,
policy, trigger, types and utility modules. `docs/ANTIGRAVITY_BRIDGE_DESIGN.md`'s
parity table lists every key, modeled or not (with the reason), and
`test_python_sdk_options_are_all_in_the_parity_table` in
`tests/antigravity_scripted/parity.rs` diffs the two, so a wheel bump that
adds an option fails until the table says what this crate does with it.

Nothing the SDK exposes may go unplaced. The dump fails on:

- a public module under `google/antigravity/` that is in neither SPEC (read
  for keys) nor MODULE_SKIP (plumbing, with the reason), so a new module
  cannot slip past;
- a public top-level class in a module SPEC reads that is in neither SPEC
  nor CLASS_SKIP (outputs, errors, SDK plumbing);
- a name the SDK exports (a module's `__all__`, a package `__init__`'s
  re-exports, or an `@beta` export) that is in neither SPEC nor CLASS_SKIP,
  or, for a module MODULE_SKIP skips, EXPORT_SKIP.

The wheel is untrusted input, so nothing from it is imported or executed:
the sources are read with `ast`, as `dump_harness_descriptor.py` reads the
protobuf descriptors.

Exits non-zero on any of the above, or when a listed source file, class or
name is missing: a dump that silently shrank, or skipped a class, would
read as "no new options" forever. With `--lenient` it prints the keys it
could read and the problems on stderr, and exits 0: the release watch
diffs a new wheel that way, before SPEC knows its new names.
"""

import ast
import pathlib
import sys

# What to read from each source file under google/antigravity/. "aliases"
# are type aliases over classes read here: exported, placed, no keys.
SPEC = {
    "connections/connection.py": {
        "fields": ["AgentConfig", "DebugConfig"],
        "methods": ["AgentConfig"],
    },
    "connections/local/local_connection_config.py": {
        "fields": ["BaseLocalAgentConfig", "LocalAgentConfig"],
        "names": ["InteractionsAgentConfig"],
    },
    "connections/local/local_openai_connection_config.py": {
        "fields": ["LocalOpenAIAgentConfig"],
    },
    "connections/local/litert_connection_config.py": {
        "fields": ["LiteRTAgentConfig"],
        "enums": ["LiteRTBackend"],
    },
    "conversation/conversation.py": {"methods": ["Conversation"]},
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
            # What `Agent.chat` takes besides a string.
            "_BaseMedia", "SlashCommand",
        ],
        "enums": [
            "AgentBehavior", "BuiltinTools", "BudgetScope",
            "SessionContinuationMode", "StopDecision", "BuiltinSlashCommandName",
        ],
        "methods": ["BuiltinTools", "RetryConfig", "_BaseMedia", "ChatResponse"],
        # Classes with no fields of their own: the media types add only the
        # MIME types they accept, and the none-skills config is a marker.
        "names": ["Image", "Document", "Audio", "Video", "SubagentNoneSkillsConfig"],
        # `from_file`, `from_bytes`: media from a path or bytes.
        "functions": "types",
        "aliases": ["SystemInstructions", "McpServerConfig", "ContentPrimitive", "Content"],
    },
    "hooks/hooks.py": {"decorators": "hooks"},
    "hooks/policy.py": {
        "functions": "policy",
        "fields": ["Policy", "AutoPolicy"],
        "enums": ["Decision"],
    },
    "triggers/helpers.py": {"functions": "triggers"},
    "triggers/triggers.py": {
        "functions": "triggers",
        "methods": ["TriggerContext"],
        "fields": ["FileChange"],
        "enums": ["FileChangeKind"],
        "aliases": ["Trigger"],
    },
    "agent.py": {"methods": ["Agent"]},
    # What a custom tool that asks for it is handed, and the state store
    # behind it (and behind `HookContext`).
    "tools/tool_context.py": {"methods": ["ToolContext"]},
    "utils/state.py": {"methods": ["StateStore"]},
    # OpenTelemetry tracing as a set of hooks (the `otel` extra).
    "utils/otel.py": {"functions": "otel"},
    # A terminal chat loop around an agent.
    "utils/interactive.py": {"functions": "interactive"},
    # The beta `run_workflow`'s authoring API.
    "workflows.py": {"functions": "workflows"},
}


# Public methods that are SDK plumbing, not options: the abstract strategy
# factory and pydantic's copy override.
SKIP = {"AgentConfig.create_strategy", "AgentConfig.model_copy"}

# Public classes in SPEC's modules that are not harness options, by reason.
# A class that is in neither SPEC nor here fails the dump.
CLASS_SKIP = {
    # What the harness reports: steps, tool calls, usage, stop reasons,
    # questions. The bridge models them as events and response types
    # (`AgentEvent`, `ToolAction`, `ChatResponse`, `AgentQuestion`); new
    # fields on them are output, preserved by Evergreen `extra` maps.
    "types.py": {
        "ToolCall", "ToolResult", "SandboxStatus", "UsageMetadata", "StepType",
        "StepSource", "StepTarget", "StepStatus", "StopReason",
        "WorkflowProgress", "Step", "StreamChunk", "Thought", "Text",
        "WorkflowResult", "AskQuestionOption", "AskQuestionEntry",
        "AskQuestionInteractionSpec", "StopArgs",
        # Errors: `AntigravityError` covers them.
        "AntigravityConnectionError", "AntigravityCancelledError",
        "AntigravityExecutionError", "ToolExecutionError",
        "AntigravityValidationError",
    },
    # The class form of each hook, for subclassing; the decorators SPEC
    # lists are the same hooks. The contexts are their state stores.
    "hooks/hooks.py": {
        "HookContext", "SessionContext", "TurnContext", "OperationContext",
        "InspectHook", "DecideHook", "TransformHook", "OnSessionStartHook",
        "OnSessionEndHook", "PreTurnHook", "PostTurnHook",
        "PreToolCallDecideHook", "PostToolCallHook", "OnToolErrorHook",
        "OnInteractionHook", "OnCompactionHook", "StopHook",
    },
    # Transport plumbing under `Conversation`, which SPEC reads.
    "connections/connection.py": {"Connection", "ConnectionStrategy"},
    "triggers/triggers.py": {"TriggerConnection"},
    # The hooks `get_otel_hooks` returns.
    "utils/otel.py": {
        "OTelSessionStartHook", "OTelSessionEndHook", "OTelPreTurnHook",
        "OTelPostTurnHook", "OTelPreStepHook", "OTelPostStepHook",
        "OTelPreToolCallHook", "OTelPostToolCallHook", "OTelOnToolErrorHook",
    },
    # The terminal loop's spinner and its confirmation and question hooks.
    "utils/interactive.py": {"Spinner", "ToolConfirmationHook", "AskQuestionHook"},
    # A workflow script's errors, its agent handle and a defined workflow.
    "workflows.py": {"WorkflowError", "ToolError", "WorkflowAgent", "WorkflowDefinition"},
}

# Public modules SPEC does not read, by reason. A module that is in neither
# SPEC nor here fails the dump, and so does a name one of these exports
# that EXPORT_SKIP does not list.
MODULE_SKIP = {
    # The `@beta` decorator and its namespace. What it marks is checked
    # where it is defined.
    "beta/__init__.py",
    # Transport and protocol plumbing under `Conversation`, which SPEC reads.
    "connections/struct_converter.py",
    "connections/local/event_processor.py",
    "connections/local/hook_router.py",
    "connections/local/interactions_config_converter.py",
    "connections/local/interactions_connection.py",
    "connections/local/interactions_event_processor.py",
    "connections/local/interactions_step_assembler.py",
    "connections/local/litert_connection.py",
    "connections/local/litert_server.py",
    "connections/local/local_connection.py",
    "connections/local/local_openai_connection.py",
    "connections/local/proto_converters.py",
    # The builtin tools' typed results: output.
    "connections/local/types.py",
    # Test helpers that ship in the wheel.
    "connections/local/test_utils.py",
    # The runners behind the hooks, tools and triggers SPEC reads.
    "hooks/hook_runner.py",
    "tools/schema_utils.py",
    "tools/tool_runner.py",
    "triggers/trigger_runner.py",
}

# Names a package exports from a module MODULE_SKIP skips, by reason.
EXPORT_SKIP = {
    # A step as the local connection reports it: output.
    "connections/local/event_processor.py": {"LocalConnectionStep"},
    # The local transport, and how it declares a Python callable as a tool.
    "connections/local/local_connection.py": {
        "LocalConnection", "LocalConnectionStrategy", "callable_to_tool_proto",
    },
    # The builtin tools' results: output, which the bridge reports as
    # `ToolAction` events with Evergreen `extra` maps.
    "connections/local/types.py": {
        "EditFileResult", "FindFileResult", "GenerateImageResult",
        "ListDirectoryEntry", "ListDirectoryResult", "ReadUrlContentResult",
        "RunCommandResult", "SearchDirectoryResult", "SearchWebResult",
        "TextResult", "ToolOutput",
    },
}

# Directories under google/antigravity/ that hold no SDK surface: the
# generated protobuf modules, which dump_harness_descriptor.py reads.
EXCLUDED_DIRS = ("proto/",)

KINDS = ("fields", "enums", "methods", "names")


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


def public_classes(tree):
    return {
        node.name for node in tree.body
        if isinstance(node, ast.ClassDef) and not node.name.startswith("_")
    }


def assigned(tree):
    """Names bound by module-level assignments (type aliases, constants)."""
    names = set()
    for node in tree.body:
        targets = node.targets if isinstance(node, ast.Assign) else (
            [node.target] if isinstance(node, ast.AnnAssign) else [])
        names.update(t.id for t in targets if isinstance(t, ast.Name))
    return names


def defined(tree):
    """Every name a module binds at top level other than by an import."""
    return {
        node.name for node in tree.body
        if isinstance(node, (ast.ClassDef, ast.FunctionDef, ast.AsyncFunctionDef))
    } | assigned(tree)


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


def static_all(tree):
    """A module's `__all__` when it is a literal list of strings, else None."""
    for node in tree.body:
        if isinstance(node, ast.Assign):
            if not any(isinstance(t, ast.Name) and t.id == "__all__" for t in node.targets):
                continue
            value = node.value
        elif (isinstance(node, ast.AnnAssign) and isinstance(node.target, ast.Name)
              and node.target.id == "__all__" and node.value is not None):
            value = node.value
        else:
            continue
        if isinstance(value, (ast.List, ast.Tuple)) and all(
                isinstance(e, ast.Constant) and isinstance(e.value, str) for e in value.elts):
            return [e.value for e in value.elts]
    return None


def is_beta(node):
    """Whether an expression is the `beta` decorator (`beta`, `x.beta`, `x.beta(..)`)."""
    if isinstance(node, ast.Call):
        node = node.func
    return ((isinstance(node, ast.Name) and node.id == "beta")
            or (isinstance(node, ast.Attribute) and node.attr == "beta"))


def beta_exports(tree):
    """Top-level names exported through `@beta`, and whether the module
    exports itself (`beta(sys.modules[__name__])`)."""
    names = [
        node.name for node in tree.body
        if isinstance(node, (ast.ClassDef, ast.FunctionDef, ast.AsyncFunctionDef))
        and any(is_beta(d) for d in node.decorator_list)
    ]
    whole = any(
        isinstance(node, ast.Expr) and isinstance(node.value, ast.Call)
        and is_beta(node.value.func) and node.value.args
        and ast.unparse(node.value.args[0]) == "sys.modules[__name__]"
        for node in tree.body
    )
    return names, whole


class Package:
    """The sources under google/antigravity/, parsed on demand."""

    def __init__(self, root, problems):
        self.root = root
        self.problems = problems
        self.trees = {}

    def tree(self, relative):
        """A module's syntax tree; an empty one, and a problem, when it
        does not parse."""
        if relative not in self.trees:
            try:
                self.trees[relative] = ast.parse(
                    (self.root / relative).read_text(encoding="utf-8"))
            except (SyntaxError, UnicodeDecodeError, ValueError) as e:
                self.problems.append("%s does not parse: %s" % (relative, e))
                self.trees[relative] = ast.Module(body=[], type_ignores=[])
        return self.trees[relative]

    def modules(self):
        """Every source module, tests and the excluded directories left out."""
        return sorted(
            p.relative_to(self.root).as_posix() for p in self.root.rglob("*.py")
            if not p.name.endswith("_test.py")
            and not p.relative_to(self.root).as_posix().startswith(EXCLUDED_DIRS)
        )

    def module_path(self, dotted):
        """`google.antigravity.a.b` as a path under the package: a module
        file, a package's `__init__.py`, or a namespace directory ending in
        `/`. None outside the package or when nothing is there."""
        parts = dotted.split(".")
        if parts[:2] != ["google", "antigravity"]:
            return None
        relative = "/".join(parts[2:])
        if not relative:
            return "__init__.py"
        if (self.root / (relative + ".py")).is_file():
            return relative + ".py"
        if (self.root / relative / "__init__.py").is_file():
            return relative + "/__init__.py"
        if (self.root / relative).is_dir():
            return relative + "/"
        return None

    def package_of(self, relative):
        """The dotted package a module is in, for relative imports."""
        parts = ["google", "antigravity"] + relative.split("/")[:-1]
        return ".".join(parts)

    def imports(self, relative):
        """Local name -> (dotted module, imported name), for every top-level
        `from .. import ..` of the package's own modules."""
        found = {}
        for node in self.tree(relative).body:
            if not isinstance(node, ast.ImportFrom):
                continue
            if node.level:
                base = self.package_of(relative).split(".")
                base = base[:len(base) - (node.level - 1)]
                dotted = ".".join(base + ([node.module] if node.module else []))
            else:
                dotted = node.module or ""
            if not dotted.startswith("google.antigravity"):
                continue
            for alias in node.names:
                found[alias.asname or alias.name] = (dotted, alias.name)
        return found

    def resolve(self, relative, name, depth=0):
        """Where `name`, as `relative` exports it, is defined: ("name",
        module, name) or ("module", module). None when it is nowhere."""
        if depth > 10 or relative.endswith("/"):
            return None
        if name in defined(self.tree(relative)):
            return ("name", relative, name)
        imported = self.imports(relative).get(name)
        if imported is None:
            return None
        dotted, source = imported
        submodule = self.module_path("%s.%s" % (dotted, source))
        if submodule is not None:
            return ("module", submodule)
        target = self.module_path(dotted)
        if target is None:
            return None
        return self.resolve(target, source, depth + 1)

    def exports(self, relative):
        """The names a module exports: its `__all__`, or for a package
        `__init__` without one, its re-exports of the package's names."""
        listed = static_all(self.tree(relative))
        if listed is not None:
            return listed
        if relative.endswith("__init__.py"):
            return sorted(n for n in self.imports(relative) if not n.startswith("_"))
        return []


def main():
    args = sys.argv[1:]
    lenient = "--lenient" in args
    args = [a for a in args if a != "--lenient"]
    if len(args) != 1:
        sys.exit("usage: dump_sdk_options.py [--lenient] <unpacked-wheel-root>")
    root = pathlib.Path(args[0]) / "google" / "antigravity"
    if not root.is_dir():
        sys.exit("no google/antigravity package under %s" % args[0])
    keys = set()
    problems = []
    package = Package(root, problems)
    # Module -> names SPEC or CLASS_SKIP places there.
    placed = {}

    for relative, spec in SPEC.items():
        path = root / relative
        if not path.is_file():
            problems.append("missing %s: the wheel layout changed; update SPEC" % path)
            continue
        tree = package.tree(relative)
        found = classes(tree)
        here = placed.setdefault(relative, set(CLASS_SKIP.get(relative, ())))
        for kind in KINDS:
            for name in spec.get(kind, []):
                if name not in found:
                    problems.append("class %s is gone from %s; update SPEC" % (name, relative))
                    continue
                here.add(name)
                if kind == "names":
                    keys.add(name)
                    continue
                members = {
                    "fields": fields,
                    "enums": enum_members,
                    "methods": methods,
                }[kind](found[name])
                keys.update("%s.%s" % (name, member) for member in members)
        for name in spec.get("aliases", []):
            if name not in assigned(tree):
                problems.append("alias %s is gone from %s; update SPEC" % (name, relative))
            here.add(name)
        problems += ["class in neither SPEC nor CLASS_SKIP: %s (%s)" % (name, relative)
                     for name in sorted(public_classes(tree) - here)]
        stale = set(CLASS_SKIP.get(relative, ())) - set(found)
        if stale:
            problems.append("CLASS_SKIP names classes gone from %s: %s"
                            % (relative, ", ".join(sorted(stale))))
        if "functions" in spec:
            names = functions(tree)
            here.update(names)
            keys.update("%s.%s" % (spec["functions"], f) for f in names)
        if "decorators" in spec:
            names = decorators(tree)
            here.update(names)
            keys.update("%s.%s" % (spec["decorators"], d) for d in names)

    modules = package.modules()
    for relative in sorted(MODULE_SKIP - set(modules)):
        problems.append("MODULE_SKIP names a module the wheel lacks: %s" % relative)
    for relative, names in EXPORT_SKIP.items():
        if relative not in MODULE_SKIP:
            problems.append("EXPORT_SKIP lists %s, which MODULE_SKIP does not skip" % relative)
        elif relative in modules:
            tree = package.tree(relative)
            gone = names - defined(tree) - set(package.imports(relative))
            if gone:
                problems.append("EXPORT_SKIP names gone from %s: %s"
                                % (relative, ", ".join(sorted(gone))))

    for relative in modules:
        tree = package.tree(relative)
        if relative not in SPEC and relative not in MODULE_SKIP:
            exports_only = relative.endswith("__init__.py") and not (
                functions(tree) or public_classes(tree))
            if not exports_only:
                problems.append("module in neither SPEC nor MODULE_SKIP: %s" % relative)
        exported = [(relative, name) for name in package.exports(relative)]
        beta_names, beta_module = beta_exports(tree)
        exported += [(relative, name) for name in beta_names]
        if beta_module and relative not in SPEC:
            problems.append("%s exports itself through @beta, but SPEC does not read it"
                            % relative)
        for exporter, name in exported:
            target = package.resolve(exporter, name)
            if target is None:
                problems.append("%s exports %s, which it neither defines nor imports "
                                "from the package" % (exporter, name))
                continue
            if target[0] == "module":
                continue
            _, home, source = target
            if home in SPEC:
                if source not in placed.get(home, set()):
                    problems.append("exported name in neither SPEC nor CLASS_SKIP: "
                                    "%s (%s, exported by %s)" % (source, home, exporter))
            elif home in MODULE_SKIP:
                if source not in EXPORT_SKIP.get(home, set()):
                    problems.append("exported name in neither SPEC nor EXPORT_SKIP: "
                                    "%s (%s, exported by %s)" % (source, home, exporter))

    keys -= SKIP
    if len(keys) < 50:
        problems.append("only %d options found; the extraction is broken" % len(keys))
    problems = sorted(set(problems))
    if problems and not lenient:
        sys.exit("dump_sdk_options.py: every option the SDK exposes must be placed. "
                 "Read each new name into SPEC (and the parity table), or skip it "
                 "with the reason:\n  " + "\n  ".join(problems))
    for problem in problems:
        sys.stderr.write("dump_sdk_options.py: %s\n" % problem)
    sys.stdout.write("".join(key + "\n" for key in sorted(keys)))


if __name__ == "__main__":
    main()
