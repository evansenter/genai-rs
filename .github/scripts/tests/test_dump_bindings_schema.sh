#!/usr/bin/env bash
# Fixture tests for dump_bindings_schema.py.
#
# Run directly: .github/scripts/tests/test_dump_bindings_schema.sh
set -euo pipefail

script_dir=$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)
DUMP="$script_dir/../dump_bindings_schema.py"

work=$(mktemp -d)
trap 'rm -rf "$work"' EXIT

failures=0
fail() {
    echo "FAIL: $*" >&2
    failures=$((failures + 1))
}

# Writes a minimal `_gaos` tree. $2 is the body of VideoContent, $3 the
# endpoint path.
make_tree() {
    local root=$1 fields=$2 path=$3
    mkdir -p "$root/types/interactions" "$root/models" "$root/_hooks"
    cat >"$root/types/interactions/videocontent.py" <<EOF
from .. import BaseModel
from typing import Literal, Optional, Union

Mime = Union[Literal["video/webm", "video/mp4"], str]


class VideoContent(BaseModel):
$fields
EOF
    cat >"$root/interactions.py" <<EOF
from . import models


class Interactions:
    def get(self):
        request = models.GetInteractionRequest(id="i")
        return self._build_request(
            method="GET",
            path="$path",
            request=request,
        )
EOF
    cat >"$root/models/getinteraction.py" <<'EOF'
class GetInteractionRequest(BaseModel):
    id: Annotated[str, FieldMetadata(path=PathParamMetadata(style="simple"))]
    stream: Annotated[
        Optional[bool],
        FieldMetadata(query=QueryParamMetadata(style="form", explode=True)),
    ] = None
EOF
    cat >"$root/_hooks/auth.py" <<'EOF'
GOOGLE_GENAI_API_REVISION = "2026-05-20"
EOF
}

# Base, and a release that only reorders a union, rewraps a docstring and
# renames a path parameter: none of that is API surface.
make_tree "$work/a" '    r"""A video."""
    uri: Optional[str] = None' '/{api_version}/interactions/{id}'
make_tree "$work/b" '    r"""A video,
    rewrapped."""
    uri: Union[None, str] = None' '/{api_version}/interactions/{interactionsId}'
sed -i.bak 's/"video\/webm", "video\/mp4"/"video\/mp4", "video\/webm"/' "$work/b/types/interactions/videocontent.py"

if ! diff <(python3 "$DUMP" "$work/a") <(python3 "$DUMP" "$work/b") >/dev/null; then
    fail "formatting-only changes produced a schema diff"
fi

# A new field is surface.
make_tree "$work/c" '    uri: Optional[str] = None
    name: Optional[str] = None' '/{api_version}/interactions/{id}'
added=$(diff <(python3 "$DUMP" "$work/a") <(python3 "$DUMP" "$work/c") | grep -c '^>' || true)
if [ "$added" != "1" ]; then
    fail "expected exactly one added line for a new field, got $added"
fi
# Captured first: `grep -q` exits early, and under pipefail the writer's
# broken pipe would read as a failure.
dump_c=$(python3 "$DUMP" "$work/c")
dump_a=$(python3 "$DUMP" "$work/a")
if ! grep -qx 'model types/interactions/videocontent.py:VideoContent.name: Union\[None, str\]' <<<"$dump_c"; then
    fail "new field not rendered as expected"
fi
if ! grep -qx 'endpoint GET /{}/interactions/{}' <<<"$dump_a"; then
    fail "endpoint line missing"
fi
if ! grep -qx 'query GET /{}/interactions/{}?stream' <<<"$dump_a"; then
    fail "query line missing"
fi
if ! grep -qx 'revision 2026-05-20' <<<"$dump_a"; then
    fail "revision line missing"
fi

# A new query parameter is surface: one added line.
make_tree "$work/q" '    uri: Optional[str] = None' '/{api_version}/interactions/{id}'
cat >>"$work/q/models/getinteraction.py" <<'EOF'
    include_input: Annotated[
        Optional[bool],
        FieldMetadata(query=QueryParamMetadata(style="form", explode=True)),
    ] = None
EOF
added=$(diff <(python3 "$DUMP" "$work/a") <(python3 "$DUMP" "$work/q") | grep '^>' || true)
if [ "$added" != '> query GET /{}/interactions/{}?include_input' ]; then
    fail "a new query parameter was not one added query line: $added"
fi

# A request the AST walk cannot read (not a `_build_request` call) fails
# rather than dropping the endpoint, and so do a missing revision and a
# request class that is not under models/.
make_tree "$work/raw" '    uri: Optional[str] = None' '/{api_version}/interactions/{id}'
cat >"$work/raw/agents.py" <<'EOF'
def get():
    return dict(method="GET", path="/{api_version}/agents/{id}")
EOF
if python3 "$DUMP" "$work/raw" >/dev/null 2>&1; then
    fail "an endpoint outside a _build_request call exited 0"
fi
make_tree "$work/norev" '    uri: Optional[str] = None' '/{api_version}/interactions/{id}'
rm "$work/norev/_hooks/auth.py"
if python3 "$DUMP" "$work/norev" >/dev/null 2>&1; then
    fail "bindings without an API_REVISION constant exited 0"
fi
make_tree "$work/nomodel" '    uri: Optional[str] = None' '/{api_version}/interactions/{id}'
rm "$work/nomodel/models/getinteraction.py"
if python3 "$DUMP" "$work/nomodel" >/dev/null 2>&1; then
    fail "a request class missing from models/ exited 0"
fi

# A missing types/ directory must fail, not print an empty (clean-looking) dump.
mkdir -p "$work/empty"
if python3 "$DUMP" "$work/empty" >/dev/null 2>&1; then
    fail "a tree without types/ exited 0"
fi

# --json resolves references through imports (a sibling module, a module
# alias from another package, a union alias), drops the SDK's Unknown*
# fallback arms, records each endpoint's query parameters and the
# Api-Revision, and reads the legacy modules beside `_gaos`: their
# endpoints, and the classes in ../types.py their signatures name.
genai="$work/genai"
mkdir -p "$genai/_gaos/types/interactions" "$genai/_gaos/types/agents" \
    "$genai/_gaos/models" "$genai/_gaos/_hooks"
cat >"$genai/version.py" <<'EOF'
__version__ = '9.9.9'
EOF
cat >"$genai/_gaos/_hooks/auth.py" <<'EOF'
GOOGLE_GENAI_API_REVISION = "2026-05-20"
EOF
cat >"$genai/_gaos/types/__init__.py" <<'EOF'
from .basemodel import BaseModel
EOF
cat >"$genai/_gaos/types/interactions/usage.py" <<'EOF'
from .. import BaseModel
from typing import Optional


class Usage(BaseModel):
    total_tokens: Optional[int] = None
EOF
cat >"$genai/_gaos/types/interactions/tool.py" <<'EOF'
from .. import BaseModel, UnrecognizedStr
from typing import Any, Literal, Optional, Union


class CodeExecution(BaseModel):
    type: Literal["code_execution"] = "code_execution"


class UnknownTool(BaseModel):
    type: Literal["UNKNOWN"] = "UNKNOWN"
    raw: Any
    is_unknown: Literal[True] = True


Mode = Union[Literal["a", "b"], UnrecognizedStr]

Tool = Union[CodeExecution, UnknownTool]
EOF
cat >"$genai/_gaos/types/agents/agent.py" <<'EOF'
from .. import BaseModel
from ..interactions import tool as interactions_tool, usage as interactions_usage
from typing import List, Optional


class Agent(BaseModel):
    tools: Optional[List[interactions_tool.Tool]] = None
    usage: Optional[interactions_usage.Usage] = None
    mode: interactions_tool.Mode
EOF
cat >"$genai/_gaos/agents.py" <<'PY'
from . import models


class Agents:
    def list(self):
        request = models.ListAgentsRequest(page_size=1)
        return self._build_request(method="GET", path="/{api_version}/agents", request=request)

    def get(self):
        request = models.GetAgentRequest(id="a")
        return self._build_request(
            method="GET", path="/{api_version}/agents/{agentsId}", request=request
        )


class AsyncAgents:
    async def list(self):
        request = models.ListAgentsRequest(page_size=1)
        return self._build_request_async(
            method="GET", path="/{api_version}/agents", request=request
        )
PY
cat >"$genai/_gaos/models/listagents.py" <<'EOF'
class ListAgentsRequest(BaseModel):
    api_version: Annotated[Optional[str], FieldMetadata(path=PathParamMetadata())] = None
    filter_: Annotated[
        Optional[str],
        pydantic.Field(alias="filter"),
        FieldMetadata(query=QueryParamMetadata(style="form", explode=True)),
    ] = None
    page_size: Annotated[
        Optional[int], FieldMetadata(query=QueryParamMetadata(style="form", explode=True))
    ] = None


class GetAgentRequest(BaseModel):
    id: Annotated[str, pydantic.Field(alias="agentsId"), FieldMetadata(path=PathParamMetadata())]
EOF

# The legacy modules. `Files._vertex_only` raises for a Developer API client,
# so neither its path nor the classes its signature names (absent from
# types.py) are read, and the Async* classes are skipped the same way.
cat >"$genai/files.py" <<'EOF'
from . import _api_module, types
from typing import Optional, Union


def _ListFilesConfig_to_mldev(from_object, parent_object=None):
  to_object = {}
  if getv(from_object, ['page_size']) is not None:
    setv(parent_object, ['_query', 'pageSize'], getv(from_object, ['page_size']))
  return to_object


def _ListFilesParameters_to_mldev(from_object, parent_object=None):
  to_object = {}
  if getv(from_object, ['config']) is not None:
    _ListFilesConfig_to_mldev(getv(from_object, ['config']), to_object)
  return to_object


def _GetFileParameters_to_mldev(from_object, parent_object=None):
  to_object = {}
  setv(to_object, ['_url', 'file'], t.t_file_name(getv(from_object, ['name'])))
  return to_object


class Files(_api_module.BaseModule):

  def _list(
      self, *, config: Optional[types.ListFilesConfigOrDict] = None
  ) -> types.ListFilesResponse:
    if self._api_client.vertexai:
      raise ValueError('Only in Gemini Developer API mode.')
    else:
      request_dict = _ListFilesParameters_to_mldev(parameter_model)
      request_url_dict = request_dict.get('_url')
      if request_url_dict:
        path = 'files'.format_map(request_url_dict)
      else:
        path = 'files'
    query_params = request_dict.get('_query')
    if query_params:
      path = f'{path}?{urlencode(query_params)}'
    response = self._api_client.request('get', path, request_dict, http_options)

  def get(
      self, *, name: str, config: Optional[types.GetFileConfigOrDict] = None
  ) -> types.File:
    if self._api_client.vertexai:
      raise ValueError('Only in Gemini Developer API mode.')
    else:
      request_dict = _GetFileParameters_to_mldev(parameter_model)
      path = 'files/{file}'.format_map(request_dict.get('_url'))
    response = self._api_client.request('get', path, request_dict, http_options)

  def download(
      self, *, file: Union[str, types.File], config: Optional[types.DownloadFileConfigOrDict] = None
  ) -> bytes:
    path = f'files/{name}:download'
    query_params = {'alt': 'media'}
    path = f'{path}?{urlencode(query_params)}'
    return self._api_client.download_file(path, http_options=None)

  def _vertex_only(
      self, *, config: Optional[types.VertexConfigOrDict] = None
  ) -> types.VertexThing:
    if not self._api_client.vertexai:
      raise ValueError('Only in Vertex mode.')
    else:
      path = 'vertex/things'
    return self._api_client.request('get', path, {}, None)


class AsyncFiles(_api_module.BaseModule):

  async def get(self, *, name: str) -> types.AsyncOnly:
    path = 'async/{name}'
    return await self._api_client.async_request('get', path, {}, None)
EOF
cat >"$genai/file_search_stores.py" <<'EOF'
from . import _api_module, types
from .documents import Documents
from typing import Optional


def _DeleteFileSearchStoreConfig_to_mldev(from_object, parent_object=None):
  to_object = {}
  if getv(from_object, ['force']) is not None:
    setv(parent_object, ['_query', 'force'], getv(from_object, ['force']))
  return to_object


def _DeleteFileSearchStoreParameters_to_mldev(from_object, parent_object=None):
  to_object = {}
  setv(to_object, ['_url', 'name'], getv(from_object, ['name']))
  if getv(from_object, ['config']) is not None:
    _DeleteFileSearchStoreConfig_to_mldev(getv(from_object, ['config']), to_object)
  return to_object


class FileSearchStores(_api_module.BaseModule):

  @property
  def documents(self) -> Documents:
    return Documents(self._api_client)

  def _list(self) -> types.ListFileSearchStoresResponse:
    path = 'fileSearchStores'
    return self._api_client.request('get', path, {}, None)

  def delete(
      self, *, name: str, config: Optional[types.DeleteFileSearchStoreConfigOrDict] = None
  ) -> None:
    request_dict = _DeleteFileSearchStoreParameters_to_mldev(parameter_model)
    path = '{name}'.format_map(request_dict.get('_url'))
    self._api_client.request('delete', path, request_dict, None)

  def import_file(
      self, *, file_search_store_name: str,
      config: Optional[types.ImportFileConfigOrDict] = None,
  ) -> types.ImportFileOperation:
    path = '{file_search_store_name}:importFile'.format_map(request_url_dict)
    return self._api_client.request('post', path, {}, None)

  def download_media(self, *, media_id: str) -> bytes:
    clean_id = media_id.lstrip('/')
    path = f'{clean_id}?alt=media'
    return self._api_client.download_file(path)
EOF
cat >"$genai/documents.py" <<'EOF'
from . import _api_module, types
from typing import Optional


class Documents(_api_module.BaseModule):

  def _list(self, *, parent: str) -> types.ListDocumentsResponse:
    path = '{parent}/documents'.format_map(request_url_dict)
    return self._api_client.request('get', path, {}, None)

  def get(self, *, name: str) -> types.Document:
    path = '{name}'.format_map(request_url_dict)
    return self._api_client.request('get', path, {}, None)
EOF
cat >"$genai/operations.py" <<'EOF'
from . import _api_module, types
from typing import Any, TypeVar


class Operations(_api_module.BaseModule):

  def _get_videos_operation(self, *, operation_name: str) -> Any:
    if self._api_client.vertexai:
      path = 'vertex/{operationName}'.format_map(request_url_dict)
    else:
      path = '{operationName}'.format_map(request_url_dict)
    return self._api_client.request('get', path, {}, None)

  T = TypeVar('T', bound=types.Operation)

  def get(self, operation: T, *, config=None) -> T:
    return self._get_videos_operation(operation_name=operation.name)
EOF
cat >"$genai/types.py" <<'EOF'
import datetime
from abc import ABC
from typing import Optional
from . import _common


class FileState(_common.CaseInSensitiveEnum):
    ACTIVE = 'ACTIVE'


class File(_common.BaseModel):
    display_name: Optional[str] = None
    create_time: Optional[datetime.datetime] = None
    state: Optional[FileState] = None


class ListFilesConfig(_common.BaseModel):
    http_options: Optional[str] = None
    page_size: Optional[int] = None


class ListFilesResponse(_common.BaseModel):
    sdk_http_response: Optional[str] = None
    files: Optional[list[File]] = None


class GetFileConfig(_common.BaseModel):
    http_options: Optional[str] = None


class DownloadFileConfig(_common.BaseModel):
    http_options: Optional[str] = None


class DeleteFileSearchStoreConfig(_common.BaseModel):
    http_options: Optional[str] = None
    force: Optional[bool] = None


class ImportFileConfig(_common.BaseModel):
    http_options: Optional[str] = None
    display_name: Optional[str] = None


class Operation(ABC):
    name: Optional[str] = None
    done: Optional[bool] = None


class ImportFileResponse(_common.BaseModel):
    document_name: Optional[str] = None


class ImportFileOperation(_common.BaseModel, Operation):
    response: Optional[ImportFileResponse] = None


class ListFileSearchStoresResponse(_common.BaseModel):
    next_page_token: Optional[str] = None


class Document(_common.BaseModel):
    name: Optional[str] = None


class ListDocumentsResponse(_common.BaseModel):
    documents: Optional[list[Document]] = None
EOF
if ! json_dump=$(python3 "$DUMP" --json "$genai/_gaos" 2>&1); then
    fail "--json failed: $json_dump"
else
    checks=$(python3 -c '
import json, sys
d = json.loads(sys.stdin.read())
c, a, e = d["classes"], d["aliases"], d["endpoints"]
agent = c["agents.Agent"]["fields"]
ok = [
    d["sdk_version"] == "9.9.9",
    d["api_revision"] == "2026-05-20",
    agent["tools"]["type"] == {"kind": "list", "item": {"kind": "ref", "name": "interactions.Tool"}},
    a["interactions.Tool"] == {"kind": "ref", "name": "interactions.CodeExecution"},
    agent["usage"]["type"] == {"kind": "ref", "name": "interactions.Usage"},
    agent["mode"]["required"] is True,
    a["interactions.Mode"] == {"kind": "literal", "values": ["a", "b"], "open": True},
    "interactions.UnknownTool" not in c,
    # Query parameters, by wire name (`filter`, not `filter_`); a path
    # parameter is not one.
    e["GET /{}/agents"] == {"query": ["filter", "page_size"]},
    e["GET /{}/agents/{}"] == {"query": []},
    # Legacy endpoints: the _query keys of a converter chain, a literal
    # query, resource names expanded to their collection (or to the parent
    # collection), and a leading parameter with nothing to expand left named.
    e["GET /{}/files"] == {"query": ["pageSize"]},
    e["GET /{}/files/{}"] == {"query": []},
    e["GET /{}/files/{}:download"] == {"query": ["alt"]},
    e["GET /{}/fileSearchStores"] == {"query": []},
    e["DELETE /{}/fileSearchStores/{}"] == {"query": ["force"]},
    e["POST /{}/fileSearchStores/{}:importFile"] == {"query": []},
    e["GET /{}/{clean_id}"] == {"query": ["alt"]},
    e["GET /{}/fileSearchStores/{}/documents"] == {"query": []},
    e["GET /{}/fileSearchStores/{}/documents/{}"] == {"query": []},
    e["GET /{}/{operationName}"] == {"query": []},
    len(e) == 12,
    # Legacy classes: the ones the signatures name, camelCase, client-only
    # and query-routed fields dropped, inherited fields kept, a TypeVar
    # read as its bound, and a root with no wire field left out.
    set(c["legacy.File"]["fields"]) == {"displayName", "createTime", "state"},
    c["legacy.File"]["fields"]["createTime"]["type"] == {"kind": "datetime"},
    c["legacy.File"]["fields"]["state"]["type"]["values"] == ["ACTIVE"],
    set(c["legacy.ListFilesResponse"]["fields"]) == {"files"},
    set(c["legacy.ImportFileConfig"]["fields"]) == {"displayName"},
    set(c["legacy.ImportFileOperation"]["fields"]) == {"name", "done", "response"},
    set(c["legacy.ImportFileResponse"]["fields"]) == {"documentName"},
    set(c["legacy.Operation"]["fields"]) == {"name", "done"},
    "legacy.ListFilesConfig" not in c,
    "legacy.GetFileConfig" not in c,
    "legacy.DeleteFileSearchStoreConfig" not in c,
    sorted(k for k in c if k.startswith("legacy.")) == sorted(
        "legacy." + n for n in [
            "Document", "File", "ImportFileConfig", "ImportFileOperation",
            "ImportFileResponse", "ListDocumentsResponse",
            "ListFileSearchStoresResponse", "ListFilesResponse", "Operation",
        ]
    ),
]
print(" ".join("ok" if x else f"fail{i}" for i, x in enumerate(ok)))
' <<<"$json_dump")
    case "$checks" in
        *fail*) fail "--json output wrong: $checks" ;;
    esac
fi

# A new method in a legacy module is a new endpoint, and the class its
# signature names is a new class: the sweep and the oracle both see it.
cat >>"$genai/files.py" <<'EOF'


class Files2(_api_module.BaseModule):

  def register(self, *, uris: list[str]) -> types.RegisterFilesResponse:
    path = 'files:register'
    return self._api_client.request('post', path, {}, None)
EOF
cat >>"$genai/types.py" <<'EOF'


class RegisterFilesResponse(_common.BaseModel):
    files: Optional[list[File]] = None
EOF
if ! grown=$(python3 "$DUMP" --json "$genai/_gaos" 2>&1); then
    fail "--json failed after adding a legacy method: $grown"
else
    checks=$(python3 -c '
import json, sys
d = json.loads(sys.stdin.read())
ok = ["POST /{}/files:register" in d["endpoints"], "legacy.RegisterFilesResponse" in d["classes"]]
print(" ".join("ok" if x else f"fail{i}" for i, x in enumerate(ok)))
' <<<"$grown")
    case "$checks" in
        *fail*) fail "a new legacy method was not seen: $checks" ;;
    esac
fi

# A signature naming a class types.py lacks fails rather than skipping it,
# and so does a legacy module that is gone.
cat >>"$genai/documents.py" <<'EOF'

  def delete(self, *, name: str) -> types.NoSuchClass:
    path = '{name}'.format_map(request_url_dict)
    return self._api_client.request('delete', path, {}, None)
EOF
if python3 "$DUMP" --json "$genai/_gaos" >/dev/null 2>&1; then
    fail "a signature naming a missing class exited 0"
fi
rm "$genai/documents.py"
if python3 "$DUMP" --json "$genai/_gaos" >/dev/null 2>&1; then
    fail "a missing legacy module exited 0"
fi

if [ "$failures" -gt 0 ]; then
    echo "$failures failure(s)" >&2
    exit 1
fi
echo "dump_bindings_schema.py: all checks passed"
