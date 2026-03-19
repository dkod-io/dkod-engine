from dkod.v1 import types_pb2 as _types_pb2
from google.protobuf.internal import containers as _containers
from google.protobuf.internal import enum_type_wrapper as _enum_type_wrapper
from google.protobuf import descriptor as _descriptor
from google.protobuf import message as _message
from collections.abc import Iterable as _Iterable, Mapping as _Mapping
from typing import ClassVar as _ClassVar, Optional as _Optional, Union as _Union

DESCRIPTOR: _descriptor.FileDescriptor

class WorkspaceMode(int, metaclass=_enum_type_wrapper.EnumTypeWrapper):
    __slots__ = ()
    EPHEMERAL: _ClassVar[WorkspaceMode]
    PERSISTENT: _ClassVar[WorkspaceMode]

class ContextDepth(int, metaclass=_enum_type_wrapper.EnumTypeWrapper):
    __slots__ = ()
    SIGNATURES: _ClassVar[ContextDepth]
    FULL: _ClassVar[ContextDepth]
    CALL_GRAPH: _ClassVar[ContextDepth]

class ChangeType(int, metaclass=_enum_type_wrapper.EnumTypeWrapper):
    __slots__ = ()
    MODIFY_FUNCTION: _ClassVar[ChangeType]
    ADD_FUNCTION: _ClassVar[ChangeType]
    DELETE_FUNCTION: _ClassVar[ChangeType]
    MODIFY_TYPE: _ClassVar[ChangeType]
    ADD_TYPE: _ClassVar[ChangeType]
    ADD_DEPENDENCY: _ClassVar[ChangeType]

class SubmitStatus(int, metaclass=_enum_type_wrapper.EnumTypeWrapper):
    __slots__ = ()
    ACCEPTED: _ClassVar[SubmitStatus]
    REJECTED: _ClassVar[SubmitStatus]
    CONFLICT: _ClassVar[SubmitStatus]
EPHEMERAL: WorkspaceMode
PERSISTENT: WorkspaceMode
SIGNATURES: ContextDepth
FULL: ContextDepth
CALL_GRAPH: ContextDepth
MODIFY_FUNCTION: ChangeType
ADD_FUNCTION: ChangeType
DELETE_FUNCTION: ChangeType
MODIFY_TYPE: ChangeType
ADD_TYPE: ChangeType
ADD_DEPENDENCY: ChangeType
ACCEPTED: SubmitStatus
REJECTED: SubmitStatus
CONFLICT: SubmitStatus

class PushMode(int, metaclass=_enum_type_wrapper.EnumTypeWrapper):
    __slots__ = ()
    PUSH_MODE_UNSPECIFIED: _ClassVar[PushMode]
    PUSH_MODE_BRANCH: _ClassVar[PushMode]
    PUSH_MODE_PR: _ClassVar[PushMode]
PUSH_MODE_UNSPECIFIED: PushMode
PUSH_MODE_BRANCH: PushMode
PUSH_MODE_PR: PushMode

class ConnectRequest(_message.Message):
    __slots__ = ("agent_id", "auth_token", "codebase", "intent", "workspace_config", "agent_name")
    AGENT_ID_FIELD_NUMBER: _ClassVar[int]
    AUTH_TOKEN_FIELD_NUMBER: _ClassVar[int]
    CODEBASE_FIELD_NUMBER: _ClassVar[int]
    INTENT_FIELD_NUMBER: _ClassVar[int]
    WORKSPACE_CONFIG_FIELD_NUMBER: _ClassVar[int]
    AGENT_NAME_FIELD_NUMBER: _ClassVar[int]
    agent_id: str
    auth_token: str
    codebase: str
    intent: str
    workspace_config: WorkspaceConfig
    agent_name: str
    def __init__(self, agent_id: _Optional[str] = ..., auth_token: _Optional[str] = ..., codebase: _Optional[str] = ..., intent: _Optional[str] = ..., workspace_config: _Optional[_Union[WorkspaceConfig, _Mapping]] = ..., agent_name: _Optional[str] = ...) -> None: ...

class ConnectResponse(_message.Message):
    __slots__ = ("session_id", "codebase_version", "summary", "changeset_id", "workspace_id", "concurrency")
    SESSION_ID_FIELD_NUMBER: _ClassVar[int]
    CODEBASE_VERSION_FIELD_NUMBER: _ClassVar[int]
    SUMMARY_FIELD_NUMBER: _ClassVar[int]
    CHANGESET_ID_FIELD_NUMBER: _ClassVar[int]
    WORKSPACE_ID_FIELD_NUMBER: _ClassVar[int]
    CONCURRENCY_FIELD_NUMBER: _ClassVar[int]
    session_id: str
    codebase_version: str
    summary: CodebaseSummary
    changeset_id: str
    workspace_id: str
    concurrency: WorkspaceConcurrencyInfo
    def __init__(self, session_id: _Optional[str] = ..., codebase_version: _Optional[str] = ..., summary: _Optional[_Union[CodebaseSummary, _Mapping]] = ..., changeset_id: _Optional[str] = ..., workspace_id: _Optional[str] = ..., concurrency: _Optional[_Union[WorkspaceConcurrencyInfo, _Mapping]] = ...) -> None: ...

class WorkspaceConfig(_message.Message):
    __slots__ = ("mode", "base_commit", "resume_session_id", "watch_other_sessions")
    MODE_FIELD_NUMBER: _ClassVar[int]
    BASE_COMMIT_FIELD_NUMBER: _ClassVar[int]
    RESUME_SESSION_ID_FIELD_NUMBER: _ClassVar[int]
    WATCH_OTHER_SESSIONS_FIELD_NUMBER: _ClassVar[int]
    mode: WorkspaceMode
    base_commit: str
    resume_session_id: str
    watch_other_sessions: bool
    def __init__(self, mode: _Optional[_Union[WorkspaceMode, str]] = ..., base_commit: _Optional[str] = ..., resume_session_id: _Optional[str] = ..., watch_other_sessions: bool = ...) -> None: ...

class WorkspaceConcurrencyInfo(_message.Message):
    __slots__ = ("active_sessions", "other_sessions")
    ACTIVE_SESSIONS_FIELD_NUMBER: _ClassVar[int]
    OTHER_SESSIONS_FIELD_NUMBER: _ClassVar[int]
    active_sessions: int
    other_sessions: _containers.RepeatedCompositeFieldContainer[ActiveSessionSummary]
    def __init__(self, active_sessions: _Optional[int] = ..., other_sessions: _Optional[_Iterable[_Union[ActiveSessionSummary, _Mapping]]] = ...) -> None: ...

class ActiveSessionSummary(_message.Message):
    __slots__ = ("agent_id", "intent", "active_files")
    AGENT_ID_FIELD_NUMBER: _ClassVar[int]
    INTENT_FIELD_NUMBER: _ClassVar[int]
    ACTIVE_FILES_FIELD_NUMBER: _ClassVar[int]
    agent_id: str
    intent: str
    active_files: _containers.RepeatedScalarFieldContainer[str]
    def __init__(self, agent_id: _Optional[str] = ..., intent: _Optional[str] = ..., active_files: _Optional[_Iterable[str]] = ...) -> None: ...

class CodebaseSummary(_message.Message):
    __slots__ = ("languages", "total_symbols", "total_files")
    LANGUAGES_FIELD_NUMBER: _ClassVar[int]
    TOTAL_SYMBOLS_FIELD_NUMBER: _ClassVar[int]
    TOTAL_FILES_FIELD_NUMBER: _ClassVar[int]
    languages: _containers.RepeatedScalarFieldContainer[str]
    total_symbols: int
    total_files: int
    def __init__(self, languages: _Optional[_Iterable[str]] = ..., total_symbols: _Optional[int] = ..., total_files: _Optional[int] = ...) -> None: ...

class ContextRequest(_message.Message):
    __slots__ = ("session_id", "query", "depth", "include_tests", "include_dependencies", "max_tokens")
    SESSION_ID_FIELD_NUMBER: _ClassVar[int]
    QUERY_FIELD_NUMBER: _ClassVar[int]
    DEPTH_FIELD_NUMBER: _ClassVar[int]
    INCLUDE_TESTS_FIELD_NUMBER: _ClassVar[int]
    INCLUDE_DEPENDENCIES_FIELD_NUMBER: _ClassVar[int]
    MAX_TOKENS_FIELD_NUMBER: _ClassVar[int]
    session_id: str
    query: str
    depth: ContextDepth
    include_tests: bool
    include_dependencies: bool
    max_tokens: int
    def __init__(self, session_id: _Optional[str] = ..., query: _Optional[str] = ..., depth: _Optional[_Union[ContextDepth, str]] = ..., include_tests: bool = ..., include_dependencies: bool = ..., max_tokens: _Optional[int] = ...) -> None: ...

class ContextResponse(_message.Message):
    __slots__ = ("symbols", "call_graph", "dependencies", "estimated_tokens")
    SYMBOLS_FIELD_NUMBER: _ClassVar[int]
    CALL_GRAPH_FIELD_NUMBER: _ClassVar[int]
    DEPENDENCIES_FIELD_NUMBER: _ClassVar[int]
    ESTIMATED_TOKENS_FIELD_NUMBER: _ClassVar[int]
    symbols: _containers.RepeatedCompositeFieldContainer[SymbolResult]
    call_graph: _containers.RepeatedCompositeFieldContainer[_types_pb2.CallEdgeRef]
    dependencies: _containers.RepeatedCompositeFieldContainer[_types_pb2.DependencyRef]
    estimated_tokens: int
    def __init__(self, symbols: _Optional[_Iterable[_Union[SymbolResult, _Mapping]]] = ..., call_graph: _Optional[_Iterable[_Union[_types_pb2.CallEdgeRef, _Mapping]]] = ..., dependencies: _Optional[_Iterable[_Union[_types_pb2.DependencyRef, _Mapping]]] = ..., estimated_tokens: _Optional[int] = ...) -> None: ...

class SymbolResult(_message.Message):
    __slots__ = ("symbol", "source", "caller_ids", "callee_ids", "test_symbol_ids")
    SYMBOL_FIELD_NUMBER: _ClassVar[int]
    SOURCE_FIELD_NUMBER: _ClassVar[int]
    CALLER_IDS_FIELD_NUMBER: _ClassVar[int]
    CALLEE_IDS_FIELD_NUMBER: _ClassVar[int]
    TEST_SYMBOL_IDS_FIELD_NUMBER: _ClassVar[int]
    symbol: _types_pb2.SymbolRef
    source: str
    caller_ids: _containers.RepeatedScalarFieldContainer[str]
    callee_ids: _containers.RepeatedScalarFieldContainer[str]
    test_symbol_ids: _containers.RepeatedScalarFieldContainer[str]
    def __init__(self, symbol: _Optional[_Union[_types_pb2.SymbolRef, _Mapping]] = ..., source: _Optional[str] = ..., caller_ids: _Optional[_Iterable[str]] = ..., callee_ids: _Optional[_Iterable[str]] = ..., test_symbol_ids: _Optional[_Iterable[str]] = ...) -> None: ...

class SubmitRequest(_message.Message):
    __slots__ = ("session_id", "intent", "changes", "changeset_id")
    SESSION_ID_FIELD_NUMBER: _ClassVar[int]
    INTENT_FIELD_NUMBER: _ClassVar[int]
    CHANGES_FIELD_NUMBER: _ClassVar[int]
    CHANGESET_ID_FIELD_NUMBER: _ClassVar[int]
    session_id: str
    intent: str
    changes: _containers.RepeatedCompositeFieldContainer[Change]
    changeset_id: str
    def __init__(self, session_id: _Optional[str] = ..., intent: _Optional[str] = ..., changes: _Optional[_Iterable[_Union[Change, _Mapping]]] = ..., changeset_id: _Optional[str] = ...) -> None: ...

class Change(_message.Message):
    __slots__ = ("type", "symbol_name", "file_path", "old_symbol_id", "new_source", "rationale")
    TYPE_FIELD_NUMBER: _ClassVar[int]
    SYMBOL_NAME_FIELD_NUMBER: _ClassVar[int]
    FILE_PATH_FIELD_NUMBER: _ClassVar[int]
    OLD_SYMBOL_ID_FIELD_NUMBER: _ClassVar[int]
    NEW_SOURCE_FIELD_NUMBER: _ClassVar[int]
    RATIONALE_FIELD_NUMBER: _ClassVar[int]
    type: ChangeType
    symbol_name: str
    file_path: str
    old_symbol_id: str
    new_source: str
    rationale: str
    def __init__(self, type: _Optional[_Union[ChangeType, str]] = ..., symbol_name: _Optional[str] = ..., file_path: _Optional[str] = ..., old_symbol_id: _Optional[str] = ..., new_source: _Optional[str] = ..., rationale: _Optional[str] = ...) -> None: ...

class SubmitResponse(_message.Message):
    __slots__ = ("status", "changeset_id", "new_version", "errors", "conflict_block")
    STATUS_FIELD_NUMBER: _ClassVar[int]
    CHANGESET_ID_FIELD_NUMBER: _ClassVar[int]
    NEW_VERSION_FIELD_NUMBER: _ClassVar[int]
    ERRORS_FIELD_NUMBER: _ClassVar[int]
    CONFLICT_BLOCK_FIELD_NUMBER: _ClassVar[int]
    status: SubmitStatus
    changeset_id: str
    new_version: str
    errors: _containers.RepeatedCompositeFieldContainer[SubmitError]
    conflict_block: SubmitConflictBlock
    def __init__(self, status: _Optional[_Union[SubmitStatus, str]] = ..., changeset_id: _Optional[str] = ..., new_version: _Optional[str] = ..., errors: _Optional[_Iterable[_Union[SubmitError, _Mapping]]] = ..., conflict_block: _Optional[_Union[SubmitConflictBlock, _Mapping]] = ...) -> None: ...

class SubmitSymbolVersion(_message.Message):
    __slots__ = ("description", "signature", "body", "change_type")
    DESCRIPTION_FIELD_NUMBER: _ClassVar[int]
    SIGNATURE_FIELD_NUMBER: _ClassVar[int]
    BODY_FIELD_NUMBER: _ClassVar[int]
    CHANGE_TYPE_FIELD_NUMBER: _ClassVar[int]
    description: str
    signature: str
    body: str
    change_type: str
    def __init__(self, description: _Optional[str] = ..., signature: _Optional[str] = ..., body: _Optional[str] = ..., change_type: _Optional[str] = ...) -> None: ...

class SubmitSymbolConflictDetail(_message.Message):
    __slots__ = ("file_path", "qualified_name", "kind", "conflicting_agent", "their_change", "your_change", "base_version")
    FILE_PATH_FIELD_NUMBER: _ClassVar[int]
    QUALIFIED_NAME_FIELD_NUMBER: _ClassVar[int]
    KIND_FIELD_NUMBER: _ClassVar[int]
    CONFLICTING_AGENT_FIELD_NUMBER: _ClassVar[int]
    THEIR_CHANGE_FIELD_NUMBER: _ClassVar[int]
    YOUR_CHANGE_FIELD_NUMBER: _ClassVar[int]
    BASE_VERSION_FIELD_NUMBER: _ClassVar[int]
    file_path: str
    qualified_name: str
    kind: str
    conflicting_agent: str
    their_change: SubmitSymbolVersion
    your_change: SubmitSymbolVersion
    base_version: SubmitSymbolVersion
    def __init__(self, file_path: _Optional[str] = ..., qualified_name: _Optional[str] = ..., kind: _Optional[str] = ..., conflicting_agent: _Optional[str] = ..., their_change: _Optional[_Union[SubmitSymbolVersion, _Mapping]] = ..., your_change: _Optional[_Union[SubmitSymbolVersion, _Mapping]] = ..., base_version: _Optional[_Union[SubmitSymbolVersion, _Mapping]] = ...) -> None: ...

class SubmitConflictBlock(_message.Message):
    __slots__ = ("conflicting_symbols", "message")
    CONFLICTING_SYMBOLS_FIELD_NUMBER: _ClassVar[int]
    MESSAGE_FIELD_NUMBER: _ClassVar[int]
    conflicting_symbols: _containers.RepeatedCompositeFieldContainer[SubmitSymbolConflictDetail]
    message: str
    def __init__(self, conflicting_symbols: _Optional[_Iterable[_Union[SubmitSymbolConflictDetail, _Mapping]]] = ..., message: _Optional[str] = ...) -> None: ...

class SubmitError(_message.Message):
    __slots__ = ("message", "symbol_id", "file_path")
    MESSAGE_FIELD_NUMBER: _ClassVar[int]
    SYMBOL_ID_FIELD_NUMBER: _ClassVar[int]
    FILE_PATH_FIELD_NUMBER: _ClassVar[int]
    message: str
    symbol_id: str
    file_path: str
    def __init__(self, message: _Optional[str] = ..., symbol_id: _Optional[str] = ..., file_path: _Optional[str] = ...) -> None: ...

class VerifyRequest(_message.Message):
    __slots__ = ("session_id", "changeset_id")
    SESSION_ID_FIELD_NUMBER: _ClassVar[int]
    CHANGESET_ID_FIELD_NUMBER: _ClassVar[int]
    session_id: str
    changeset_id: str
    def __init__(self, session_id: _Optional[str] = ..., changeset_id: _Optional[str] = ...) -> None: ...

class VerifyStepResult(_message.Message):
    __slots__ = ("step_order", "step_name", "status", "output", "required", "findings", "suggestions")
    STEP_ORDER_FIELD_NUMBER: _ClassVar[int]
    STEP_NAME_FIELD_NUMBER: _ClassVar[int]
    STATUS_FIELD_NUMBER: _ClassVar[int]
    OUTPUT_FIELD_NUMBER: _ClassVar[int]
    REQUIRED_FIELD_NUMBER: _ClassVar[int]
    FINDINGS_FIELD_NUMBER: _ClassVar[int]
    SUGGESTIONS_FIELD_NUMBER: _ClassVar[int]
    step_order: int
    step_name: str
    status: str
    output: str
    required: bool
    findings: _containers.RepeatedCompositeFieldContainer[Finding]
    suggestions: _containers.RepeatedCompositeFieldContainer[Suggestion]
    def __init__(self, step_order: _Optional[int] = ..., step_name: _Optional[str] = ..., status: _Optional[str] = ..., output: _Optional[str] = ..., required: bool = ..., findings: _Optional[_Iterable[_Union[Finding, _Mapping]]] = ..., suggestions: _Optional[_Iterable[_Union[Suggestion, _Mapping]]] = ...) -> None: ...

class Finding(_message.Message):
    __slots__ = ("severity", "check_name", "message", "file_path", "line", "symbol")
    SEVERITY_FIELD_NUMBER: _ClassVar[int]
    CHECK_NAME_FIELD_NUMBER: _ClassVar[int]
    MESSAGE_FIELD_NUMBER: _ClassVar[int]
    FILE_PATH_FIELD_NUMBER: _ClassVar[int]
    LINE_FIELD_NUMBER: _ClassVar[int]
    SYMBOL_FIELD_NUMBER: _ClassVar[int]
    severity: str
    check_name: str
    message: str
    file_path: str
    line: int
    symbol: str
    def __init__(self, severity: _Optional[str] = ..., check_name: _Optional[str] = ..., message: _Optional[str] = ..., file_path: _Optional[str] = ..., line: _Optional[int] = ..., symbol: _Optional[str] = ...) -> None: ...

class Suggestion(_message.Message):
    __slots__ = ("finding_index", "description", "file_path", "replacement")
    FINDING_INDEX_FIELD_NUMBER: _ClassVar[int]
    DESCRIPTION_FIELD_NUMBER: _ClassVar[int]
    FILE_PATH_FIELD_NUMBER: _ClassVar[int]
    REPLACEMENT_FIELD_NUMBER: _ClassVar[int]
    finding_index: int
    description: str
    file_path: str
    replacement: str
    def __init__(self, finding_index: _Optional[int] = ..., description: _Optional[str] = ..., file_path: _Optional[str] = ..., replacement: _Optional[str] = ...) -> None: ...

class MergeRequest(_message.Message):
    __slots__ = ("session_id", "changeset_id", "commit_message")
    SESSION_ID_FIELD_NUMBER: _ClassVar[int]
    CHANGESET_ID_FIELD_NUMBER: _ClassVar[int]
    COMMIT_MESSAGE_FIELD_NUMBER: _ClassVar[int]
    session_id: str
    changeset_id: str
    commit_message: str
    def __init__(self, session_id: _Optional[str] = ..., changeset_id: _Optional[str] = ..., commit_message: _Optional[str] = ...) -> None: ...

class MergeResponse(_message.Message):
    __slots__ = ("success", "conflict")
    SUCCESS_FIELD_NUMBER: _ClassVar[int]
    CONFLICT_FIELD_NUMBER: _ClassVar[int]
    success: MergeSuccess
    conflict: MergeConflict
    def __init__(self, success: _Optional[_Union[MergeSuccess, _Mapping]] = ..., conflict: _Optional[_Union[MergeConflict, _Mapping]] = ...) -> None: ...

class MergeSuccess(_message.Message):
    __slots__ = ("commit_hash", "merged_version", "auto_rebased", "auto_rebased_files")
    COMMIT_HASH_FIELD_NUMBER: _ClassVar[int]
    MERGED_VERSION_FIELD_NUMBER: _ClassVar[int]
    AUTO_REBASED_FIELD_NUMBER: _ClassVar[int]
    AUTO_REBASED_FILES_FIELD_NUMBER: _ClassVar[int]
    commit_hash: str
    merged_version: str
    auto_rebased: bool
    auto_rebased_files: _containers.RepeatedScalarFieldContainer[str]
    def __init__(self, commit_hash: _Optional[str] = ..., merged_version: _Optional[str] = ..., auto_rebased: bool = ..., auto_rebased_files: _Optional[_Iterable[str]] = ...) -> None: ...

class MergeConflict(_message.Message):
    __slots__ = ("changeset_id", "conflicts", "suggested_action", "available_actions")
    CHANGESET_ID_FIELD_NUMBER: _ClassVar[int]
    CONFLICTS_FIELD_NUMBER: _ClassVar[int]
    SUGGESTED_ACTION_FIELD_NUMBER: _ClassVar[int]
    AVAILABLE_ACTIONS_FIELD_NUMBER: _ClassVar[int]
    changeset_id: str
    conflicts: _containers.RepeatedCompositeFieldContainer[ConflictDetail]
    suggested_action: str
    available_actions: _containers.RepeatedScalarFieldContainer[str]
    def __init__(self, changeset_id: _Optional[str] = ..., conflicts: _Optional[_Iterable[_Union[ConflictDetail, _Mapping]]] = ..., suggested_action: _Optional[str] = ..., available_actions: _Optional[_Iterable[str]] = ...) -> None: ...

class ConflictDetail(_message.Message):
    __slots__ = ("file_path", "symbols", "your_agent", "their_agent", "conflict_type", "description")
    FILE_PATH_FIELD_NUMBER: _ClassVar[int]
    SYMBOLS_FIELD_NUMBER: _ClassVar[int]
    YOUR_AGENT_FIELD_NUMBER: _ClassVar[int]
    THEIR_AGENT_FIELD_NUMBER: _ClassVar[int]
    CONFLICT_TYPE_FIELD_NUMBER: _ClassVar[int]
    DESCRIPTION_FIELD_NUMBER: _ClassVar[int]
    file_path: str
    symbols: _containers.RepeatedScalarFieldContainer[str]
    your_agent: str
    their_agent: str
    conflict_type: str
    description: str
    def __init__(self, file_path: _Optional[str] = ..., symbols: _Optional[_Iterable[str]] = ..., your_agent: _Optional[str] = ..., their_agent: _Optional[str] = ..., conflict_type: _Optional[str] = ..., description: _Optional[str] = ...) -> None: ...

class WatchRequest(_message.Message):
    __slots__ = ("session_id", "repo_id", "filter")
    SESSION_ID_FIELD_NUMBER: _ClassVar[int]
    REPO_ID_FIELD_NUMBER: _ClassVar[int]
    FILTER_FIELD_NUMBER: _ClassVar[int]
    session_id: str
    repo_id: str
    filter: str
    def __init__(self, session_id: _Optional[str] = ..., repo_id: _Optional[str] = ..., filter: _Optional[str] = ...) -> None: ...

class FileChange(_message.Message):
    __slots__ = ("path", "operation")
    PATH_FIELD_NUMBER: _ClassVar[int]
    OPERATION_FIELD_NUMBER: _ClassVar[int]
    path: str
    operation: str
    def __init__(self, path: _Optional[str] = ..., operation: _Optional[str] = ...) -> None: ...

class SymbolChangeDetail(_message.Message):
    __slots__ = ("symbol_name", "file_path", "change_type", "kind")
    SYMBOL_NAME_FIELD_NUMBER: _ClassVar[int]
    FILE_PATH_FIELD_NUMBER: _ClassVar[int]
    CHANGE_TYPE_FIELD_NUMBER: _ClassVar[int]
    KIND_FIELD_NUMBER: _ClassVar[int]
    symbol_name: str
    file_path: str
    change_type: str
    kind: str
    def __init__(self, symbol_name: _Optional[str] = ..., file_path: _Optional[str] = ..., change_type: _Optional[str] = ..., kind: _Optional[str] = ...) -> None: ...

class WatchEvent(_message.Message):
    __slots__ = ("event_type", "changeset_id", "agent_id", "affected_symbols", "details", "session_id", "affected_files", "symbol_changes", "repo_id", "event_id")
    EVENT_TYPE_FIELD_NUMBER: _ClassVar[int]
    CHANGESET_ID_FIELD_NUMBER: _ClassVar[int]
    AGENT_ID_FIELD_NUMBER: _ClassVar[int]
    AFFECTED_SYMBOLS_FIELD_NUMBER: _ClassVar[int]
    DETAILS_FIELD_NUMBER: _ClassVar[int]
    SESSION_ID_FIELD_NUMBER: _ClassVar[int]
    AFFECTED_FILES_FIELD_NUMBER: _ClassVar[int]
    SYMBOL_CHANGES_FIELD_NUMBER: _ClassVar[int]
    REPO_ID_FIELD_NUMBER: _ClassVar[int]
    EVENT_ID_FIELD_NUMBER: _ClassVar[int]
    event_type: str
    changeset_id: str
    agent_id: str
    affected_symbols: _containers.RepeatedScalarFieldContainer[str]
    details: str
    session_id: str
    affected_files: _containers.RepeatedCompositeFieldContainer[FileChange]
    symbol_changes: _containers.RepeatedCompositeFieldContainer[SymbolChangeDetail]
    repo_id: str
    event_id: str
    def __init__(self, event_type: _Optional[str] = ..., changeset_id: _Optional[str] = ..., agent_id: _Optional[str] = ..., affected_symbols: _Optional[_Iterable[str]] = ..., details: _Optional[str] = ..., session_id: _Optional[str] = ..., affected_files: _Optional[_Iterable[_Union[FileChange, _Mapping]]] = ..., symbol_changes: _Optional[_Iterable[_Union[SymbolChangeDetail, _Mapping]]] = ..., repo_id: _Optional[str] = ..., event_id: _Optional[str] = ...) -> None: ...

class FileReadRequest(_message.Message):
    __slots__ = ("session_id", "path")
    SESSION_ID_FIELD_NUMBER: _ClassVar[int]
    PATH_FIELD_NUMBER: _ClassVar[int]
    session_id: str
    path: str
    def __init__(self, session_id: _Optional[str] = ..., path: _Optional[str] = ...) -> None: ...

class FileReadResponse(_message.Message):
    __slots__ = ("content", "hash", "modified_in_session")
    CONTENT_FIELD_NUMBER: _ClassVar[int]
    HASH_FIELD_NUMBER: _ClassVar[int]
    MODIFIED_IN_SESSION_FIELD_NUMBER: _ClassVar[int]
    content: bytes
    hash: str
    modified_in_session: bool
    def __init__(self, content: _Optional[bytes] = ..., hash: _Optional[str] = ..., modified_in_session: bool = ...) -> None: ...

class FileWriteRequest(_message.Message):
    __slots__ = ("session_id", "path", "content")
    SESSION_ID_FIELD_NUMBER: _ClassVar[int]
    PATH_FIELD_NUMBER: _ClassVar[int]
    CONTENT_FIELD_NUMBER: _ClassVar[int]
    session_id: str
    path: str
    content: bytes
    def __init__(self, session_id: _Optional[str] = ..., path: _Optional[str] = ..., content: _Optional[bytes] = ...) -> None: ...

class FileWriteResponse(_message.Message):
    __slots__ = ("new_hash", "detected_changes", "conflict_warnings")
    NEW_HASH_FIELD_NUMBER: _ClassVar[int]
    DETECTED_CHANGES_FIELD_NUMBER: _ClassVar[int]
    CONFLICT_WARNINGS_FIELD_NUMBER: _ClassVar[int]
    new_hash: str
    detected_changes: _containers.RepeatedCompositeFieldContainer[SymbolChange]
    conflict_warnings: _containers.RepeatedCompositeFieldContainer[ConflictWarning]
    def __init__(self, new_hash: _Optional[str] = ..., detected_changes: _Optional[_Iterable[_Union[SymbolChange, _Mapping]]] = ..., conflict_warnings: _Optional[_Iterable[_Union[ConflictWarning, _Mapping]]] = ...) -> None: ...

class ConflictWarning(_message.Message):
    __slots__ = ("file_path", "symbol_name", "conflicting_agent", "conflicting_session_id", "message")
    FILE_PATH_FIELD_NUMBER: _ClassVar[int]
    SYMBOL_NAME_FIELD_NUMBER: _ClassVar[int]
    CONFLICTING_AGENT_FIELD_NUMBER: _ClassVar[int]
    CONFLICTING_SESSION_ID_FIELD_NUMBER: _ClassVar[int]
    MESSAGE_FIELD_NUMBER: _ClassVar[int]
    file_path: str
    symbol_name: str
    conflicting_agent: str
    conflicting_session_id: str
    message: str
    def __init__(self, file_path: _Optional[str] = ..., symbol_name: _Optional[str] = ..., conflicting_agent: _Optional[str] = ..., conflicting_session_id: _Optional[str] = ..., message: _Optional[str] = ...) -> None: ...

class FileListRequest(_message.Message):
    __slots__ = ("session_id", "prefix", "only_modified")
    SESSION_ID_FIELD_NUMBER: _ClassVar[int]
    PREFIX_FIELD_NUMBER: _ClassVar[int]
    ONLY_MODIFIED_FIELD_NUMBER: _ClassVar[int]
    session_id: str
    prefix: str
    only_modified: bool
    def __init__(self, session_id: _Optional[str] = ..., prefix: _Optional[str] = ..., only_modified: bool = ...) -> None: ...

class FileListResponse(_message.Message):
    __slots__ = ("files",)
    FILES_FIELD_NUMBER: _ClassVar[int]
    files: _containers.RepeatedCompositeFieldContainer[FileEntry]
    def __init__(self, files: _Optional[_Iterable[_Union[FileEntry, _Mapping]]] = ...) -> None: ...

class FileEntry(_message.Message):
    __slots__ = ("path", "modified_in_session", "modified_by_other")
    PATH_FIELD_NUMBER: _ClassVar[int]
    MODIFIED_IN_SESSION_FIELD_NUMBER: _ClassVar[int]
    MODIFIED_BY_OTHER_FIELD_NUMBER: _ClassVar[int]
    path: str
    modified_in_session: bool
    modified_by_other: str
    def __init__(self, path: _Optional[str] = ..., modified_in_session: bool = ..., modified_by_other: _Optional[str] = ...) -> None: ...

class PreSubmitCheckRequest(_message.Message):
    __slots__ = ("session_id",)
    SESSION_ID_FIELD_NUMBER: _ClassVar[int]
    session_id: str
    def __init__(self, session_id: _Optional[str] = ...) -> None: ...

class PreSubmitCheckResponse(_message.Message):
    __slots__ = ("has_conflicts", "potential_conflicts", "files_modified", "symbols_changed")
    HAS_CONFLICTS_FIELD_NUMBER: _ClassVar[int]
    POTENTIAL_CONFLICTS_FIELD_NUMBER: _ClassVar[int]
    FILES_MODIFIED_FIELD_NUMBER: _ClassVar[int]
    SYMBOLS_CHANGED_FIELD_NUMBER: _ClassVar[int]
    has_conflicts: bool
    potential_conflicts: _containers.RepeatedCompositeFieldContainer[SemanticConflict]
    files_modified: int
    symbols_changed: int
    def __init__(self, has_conflicts: bool = ..., potential_conflicts: _Optional[_Iterable[_Union[SemanticConflict, _Mapping]]] = ..., files_modified: _Optional[int] = ..., symbols_changed: _Optional[int] = ...) -> None: ...

class SemanticConflict(_message.Message):
    __slots__ = ("file_path", "symbol_name", "our_change", "their_change")
    FILE_PATH_FIELD_NUMBER: _ClassVar[int]
    SYMBOL_NAME_FIELD_NUMBER: _ClassVar[int]
    OUR_CHANGE_FIELD_NUMBER: _ClassVar[int]
    THEIR_CHANGE_FIELD_NUMBER: _ClassVar[int]
    file_path: str
    symbol_name: str
    our_change: str
    their_change: str
    def __init__(self, file_path: _Optional[str] = ..., symbol_name: _Optional[str] = ..., our_change: _Optional[str] = ..., their_change: _Optional[str] = ...) -> None: ...

class SymbolChange(_message.Message):
    __slots__ = ("symbol_name", "change_type")
    SYMBOL_NAME_FIELD_NUMBER: _ClassVar[int]
    CHANGE_TYPE_FIELD_NUMBER: _ClassVar[int]
    symbol_name: str
    change_type: str
    def __init__(self, symbol_name: _Optional[str] = ..., change_type: _Optional[str] = ...) -> None: ...

class SessionStatusRequest(_message.Message):
    __slots__ = ("session_id",)
    SESSION_ID_FIELD_NUMBER: _ClassVar[int]
    session_id: str
    def __init__(self, session_id: _Optional[str] = ...) -> None: ...

class SessionStatusResponse(_message.Message):
    __slots__ = ("session_id", "base_commit", "files_modified", "symbols_modified", "overlay_size_bytes", "active_other_sessions")
    SESSION_ID_FIELD_NUMBER: _ClassVar[int]
    BASE_COMMIT_FIELD_NUMBER: _ClassVar[int]
    FILES_MODIFIED_FIELD_NUMBER: _ClassVar[int]
    SYMBOLS_MODIFIED_FIELD_NUMBER: _ClassVar[int]
    OVERLAY_SIZE_BYTES_FIELD_NUMBER: _ClassVar[int]
    ACTIVE_OTHER_SESSIONS_FIELD_NUMBER: _ClassVar[int]
    session_id: str
    base_commit: str
    files_modified: _containers.RepeatedScalarFieldContainer[str]
    symbols_modified: _containers.RepeatedScalarFieldContainer[str]
    overlay_size_bytes: int
    active_other_sessions: int
    def __init__(self, session_id: _Optional[str] = ..., base_commit: _Optional[str] = ..., files_modified: _Optional[_Iterable[str]] = ..., symbols_modified: _Optional[_Iterable[str]] = ..., overlay_size_bytes: _Optional[int] = ..., active_other_sessions: _Optional[int] = ...) -> None: ...

class PushRequest(_message.Message):
    __slots__ = ("session_id", "mode", "branch_name", "pr_title", "pr_body")
    SESSION_ID_FIELD_NUMBER: _ClassVar[int]
    MODE_FIELD_NUMBER: _ClassVar[int]
    BRANCH_NAME_FIELD_NUMBER: _ClassVar[int]
    PR_TITLE_FIELD_NUMBER: _ClassVar[int]
    PR_BODY_FIELD_NUMBER: _ClassVar[int]
    session_id: str
    mode: int
    branch_name: str
    pr_title: str
    pr_body: str
    def __init__(self, session_id: _Optional[str] = ..., mode: _Optional[_Union[PushMode, str]] = ..., branch_name: _Optional[str] = ..., pr_title: _Optional[str] = ..., pr_body: _Optional[str] = ...) -> None: ...

class PushResponse(_message.Message):
    __slots__ = ("branch_name", "pr_url", "commit_hash", "changeset_ids")
    BRANCH_NAME_FIELD_NUMBER: _ClassVar[int]
    PR_URL_FIELD_NUMBER: _ClassVar[int]
    COMMIT_HASH_FIELD_NUMBER: _ClassVar[int]
    CHANGESET_IDS_FIELD_NUMBER: _ClassVar[int]
    branch_name: str
    pr_url: str
    commit_hash: str
    changeset_ids: _containers.RepeatedScalarFieldContainer[str]
    def __init__(self, branch_name: _Optional[str] = ..., pr_url: _Optional[str] = ..., commit_hash: _Optional[str] = ..., changeset_ids: _Optional[_Iterable[str]] = ...) -> None: ...
