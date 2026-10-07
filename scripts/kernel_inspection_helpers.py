_INSPECT_TYPE = type
_INSPECT_DICT = dict
_INSPECT_STR = str
_INSPECT_LIST = list
_INSPECT_TUPLE = tuple
_INSPECT_SET = set
_INSPECT_FROZENSET = frozenset
_INSPECT_BYTES = bytes
_INSPECT_BYTEARRAY = bytearray
_INSPECT_INT = int
_INSPECT_FLOAT = float
_INSPECT_BOOL = bool
_INSPECT_COMPLEX = complex
_INSPECT_NONE = type(None)
_INSPECT_ORD = ord
_INSPECT_DUMPS = json.dumps
_INSPECT_MAX_BINDINGS = 4096
_INSPECT_MAX_OBJECTS = 64
_INSPECT_MAX_NAME_BYTES = 256
_INSPECT_PAYLOAD_BYTES = 12288
def _inspect_name(name):
    if _INSPECT_TYPE(name) is not _INSPECT_STR:
        return None
    if not name or _INSPECT_STR.__len__(name) > _INSPECT_MAX_NAME_BYTES:
        return None
    for char in name:
        number = _INSPECT_ORD(char)
        if number < 32 or 127 <= number <= 159 or 0xD800 <= number <= 0xDFFF:
            return None
    if _INSPECT_BYTES.__len__(_INSPECT_STR.encode(name, "utf-8")) > _INSPECT_MAX_NAME_BYTES:
        return None
    lower = _INSPECT_STR.lower(name)
    if _INSPECT_STR.startswith(name, "_") or any(part in lower for part in (
            "password", "passwd", "secret", "token", "credential", "private_key",
            "api_key", "apikey", "access_key")):
        return None
    return name
def _inspect_metadata(name, value):
    kind = _INSPECT_TYPE(value)
    label, count = "opaque", None
    if kind is _INSPECT_LIST:
        label, count = "list", _INSPECT_LIST.__len__(value)
    elif kind is _INSPECT_TUPLE:
        label, count = "tuple", _INSPECT_TUPLE.__len__(value)
    elif kind is _INSPECT_DICT:
        label, count = "dict", _INSPECT_DICT.__len__(value)
    elif kind is _INSPECT_SET:
        label, count = "set", _INSPECT_SET.__len__(value)
    elif kind is _INSPECT_FROZENSET:
        label, count = "frozenset", _INSPECT_FROZENSET.__len__(value)
    elif kind is _INSPECT_STR:
        label, count = "str", _INSPECT_STR.__len__(value)
    elif kind is _INSPECT_BYTES:
        label, count = "bytes", _INSPECT_BYTES.__len__(value)
    elif kind is _INSPECT_BYTEARRAY:
        label, count = "bytearray", _INSPECT_BYTEARRAY.__len__(value)
    elif kind is _INSPECT_INT:
        label = "int"
    elif kind is _INSPECT_FLOAT:
        label = "float"
    elif kind is _INSPECT_BOOL:
        label = "bool"
    elif kind is _INSPECT_COMPLEX:
        label = "complex"
    elif kind is _INSPECT_NONE:
        label = "NoneType"
    return {"name": name, "type_label": label, "dimensions": None,
            "element_count": count, "summary": label + ", metadata only"}
def inspect_objects(scope: dict) -> dict:
    objects, truncated, checked, budget = [], False, 0, 64
    if _INSPECT_TYPE(scope) is not _INSPECT_DICT:
        return {"objects": objects, "truncated": True}
    for name, value in _INSPECT_DICT.items(scope):
        if checked >= _INSPECT_MAX_BINDINGS or _INSPECT_LIST.__len__(objects) >= _INSPECT_MAX_OBJECTS:
            truncated = True
            break
        checked += 1
        safe_name = _inspect_name(name)
        if safe_name is None:
            truncated = True
            continue
        item = _inspect_metadata(safe_name, value)
        size = _INSPECT_BYTES.__len__(_INSPECT_STR.encode(_INSPECT_DUMPS(item, ensure_ascii=False), "utf-8")) + 2
        if budget + size > _INSPECT_PAYLOAD_BYTES:
            truncated = True
            break
        objects.append(item)
        budget += size
    return {"objects": objects, "truncated": truncated}
