from dekode.v1 import types_pb2 as _types_pb2
from google.protobuf.internal import containers as _containers
from google.protobuf.internal import enum_type_wrapper as _enum_type_wrapper
from google.protobuf import descriptor as _descriptor
from google.protobuf import message as _message
from collections.abc import Iterable as _Iterable, Mapping as _Mapping
from typing import ClassVar as _ClassVar, Optional as _Optional, Union as _Union

DESCRIPTOR: _descriptor.FileDescriptor

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

class ConnectRequest(_message.Message):
    __slots__ = ("agent_id", "auth_token", "codebase", "intent")
    AGENT_ID_FIELD_NUMBER: _ClassVar[int]
    AUTH_TOKEN_FIELD_NUMBER: _ClassVar[int]
    CODEBASE_FIELD_NUMBER: _ClassVar[int]
    INTENT_FIELD_NUMBER: _ClassVar[int]
    agent_id: str
    auth_token: str
    codebase: str
    intent: str
    def __init__(self, agent_id: _Optional[str] = ..., auth_token: _Optional[str] = ..., codebase: _Optional[str] = ..., intent: _Optional[str] = ...) -> None: ...

class ConnectResponse(_message.Message):
    __slots__ = ("session_id", "codebase_version", "summary")
    SESSION_ID_FIELD_NUMBER: _ClassVar[int]
    CODEBASE_VERSION_FIELD_NUMBER: _ClassVar[int]
    SUMMARY_FIELD_NUMBER: _ClassVar[int]
    session_id: str
    codebase_version: str
    summary: CodebaseSummary
    def __init__(self, session_id: _Optional[str] = ..., codebase_version: _Optional[str] = ..., summary: _Optional[_Union[CodebaseSummary, _Mapping]] = ...) -> None: ...

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
    __slots__ = ("session_id", "intent", "changes")
    SESSION_ID_FIELD_NUMBER: _ClassVar[int]
    INTENT_FIELD_NUMBER: _ClassVar[int]
    CHANGES_FIELD_NUMBER: _ClassVar[int]
    session_id: str
    intent: str
    changes: _containers.RepeatedCompositeFieldContainer[Change]
    def __init__(self, session_id: _Optional[str] = ..., intent: _Optional[str] = ..., changes: _Optional[_Iterable[_Union[Change, _Mapping]]] = ...) -> None: ...

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
    __slots__ = ("status", "changeset_id", "new_version", "errors")
    STATUS_FIELD_NUMBER: _ClassVar[int]
    CHANGESET_ID_FIELD_NUMBER: _ClassVar[int]
    NEW_VERSION_FIELD_NUMBER: _ClassVar[int]
    ERRORS_FIELD_NUMBER: _ClassVar[int]
    status: SubmitStatus
    changeset_id: str
    new_version: str
    errors: _containers.RepeatedCompositeFieldContainer[SubmitError]
    def __init__(self, status: _Optional[_Union[SubmitStatus, str]] = ..., changeset_id: _Optional[str] = ..., new_version: _Optional[str] = ..., errors: _Optional[_Iterable[_Union[SubmitError, _Mapping]]] = ...) -> None: ...

class SubmitError(_message.Message):
    __slots__ = ("message", "symbol_id", "file_path")
    MESSAGE_FIELD_NUMBER: _ClassVar[int]
    SYMBOL_ID_FIELD_NUMBER: _ClassVar[int]
    FILE_PATH_FIELD_NUMBER: _ClassVar[int]
    message: str
    symbol_id: str
    file_path: str
    def __init__(self, message: _Optional[str] = ..., symbol_id: _Optional[str] = ..., file_path: _Optional[str] = ...) -> None: ...
