from google.protobuf.internal import containers as _containers
from google.protobuf import descriptor as _descriptor
from google.protobuf import message as _message
from collections.abc import Iterable as _Iterable, Mapping as _Mapping
from typing import ClassVar as _ClassVar, Optional as _Optional, Union as _Union

DESCRIPTOR: _descriptor.FileDescriptor

class SymbolRef(_message.Message):
    __slots__ = ("id", "name", "qualified_name", "kind", "visibility", "file_path", "start_byte", "end_byte", "signature", "doc_comment", "parent_id")
    ID_FIELD_NUMBER: _ClassVar[int]
    NAME_FIELD_NUMBER: _ClassVar[int]
    QUALIFIED_NAME_FIELD_NUMBER: _ClassVar[int]
    KIND_FIELD_NUMBER: _ClassVar[int]
    VISIBILITY_FIELD_NUMBER: _ClassVar[int]
    FILE_PATH_FIELD_NUMBER: _ClassVar[int]
    START_BYTE_FIELD_NUMBER: _ClassVar[int]
    END_BYTE_FIELD_NUMBER: _ClassVar[int]
    SIGNATURE_FIELD_NUMBER: _ClassVar[int]
    DOC_COMMENT_FIELD_NUMBER: _ClassVar[int]
    PARENT_ID_FIELD_NUMBER: _ClassVar[int]
    id: str
    name: str
    qualified_name: str
    kind: str
    visibility: str
    file_path: str
    start_byte: int
    end_byte: int
    signature: str
    doc_comment: str
    parent_id: str
    def __init__(self, id: _Optional[str] = ..., name: _Optional[str] = ..., qualified_name: _Optional[str] = ..., kind: _Optional[str] = ..., visibility: _Optional[str] = ..., file_path: _Optional[str] = ..., start_byte: _Optional[int] = ..., end_byte: _Optional[int] = ..., signature: _Optional[str] = ..., doc_comment: _Optional[str] = ..., parent_id: _Optional[str] = ...) -> None: ...

class CallEdgeRef(_message.Message):
    __slots__ = ("caller_id", "callee_id", "kind")
    CALLER_ID_FIELD_NUMBER: _ClassVar[int]
    CALLEE_ID_FIELD_NUMBER: _ClassVar[int]
    KIND_FIELD_NUMBER: _ClassVar[int]
    caller_id: str
    callee_id: str
    kind: str
    def __init__(self, caller_id: _Optional[str] = ..., callee_id: _Optional[str] = ..., kind: _Optional[str] = ...) -> None: ...

class DependencyRef(_message.Message):
    __slots__ = ("package", "version_req", "used_by_symbol_ids")
    PACKAGE_FIELD_NUMBER: _ClassVar[int]
    VERSION_REQ_FIELD_NUMBER: _ClassVar[int]
    USED_BY_SYMBOL_IDS_FIELD_NUMBER: _ClassVar[int]
    package: str
    version_req: str
    used_by_symbol_ids: _containers.RepeatedScalarFieldContainer[str]
    def __init__(self, package: _Optional[str] = ..., version_req: _Optional[str] = ..., used_by_symbol_ids: _Optional[_Iterable[str]] = ...) -> None: ...

class TypeInfoRef(_message.Message):
    __slots__ = ("symbol_id", "params", "return_type", "fields", "implements")
    SYMBOL_ID_FIELD_NUMBER: _ClassVar[int]
    PARAMS_FIELD_NUMBER: _ClassVar[int]
    RETURN_TYPE_FIELD_NUMBER: _ClassVar[int]
    FIELDS_FIELD_NUMBER: _ClassVar[int]
    IMPLEMENTS_FIELD_NUMBER: _ClassVar[int]
    symbol_id: str
    params: _containers.RepeatedCompositeFieldContainer[Param]
    return_type: str
    fields: _containers.RepeatedCompositeFieldContainer[Field]
    implements: _containers.RepeatedScalarFieldContainer[str]
    def __init__(self, symbol_id: _Optional[str] = ..., params: _Optional[_Iterable[_Union[Param, _Mapping]]] = ..., return_type: _Optional[str] = ..., fields: _Optional[_Iterable[_Union[Field, _Mapping]]] = ..., implements: _Optional[_Iterable[str]] = ...) -> None: ...

class Param(_message.Message):
    __slots__ = ("name", "type_str")
    NAME_FIELD_NUMBER: _ClassVar[int]
    TYPE_STR_FIELD_NUMBER: _ClassVar[int]
    name: str
    type_str: str
    def __init__(self, name: _Optional[str] = ..., type_str: _Optional[str] = ...) -> None: ...

class Field(_message.Message):
    __slots__ = ("name", "type_str")
    NAME_FIELD_NUMBER: _ClassVar[int]
    TYPE_STR_FIELD_NUMBER: _ClassVar[int]
    name: str
    type_str: str
    def __init__(self, name: _Optional[str] = ..., type_str: _Optional[str] = ...) -> None: ...
