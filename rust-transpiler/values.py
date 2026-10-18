from dataclasses import dataclass


@dataclass
class Value:
    code: str
    typ: str
    # None means scalar; -1 means an array section with runtime extent.
    extent: int | None = None
    iterator: str | None = None
    owned: bool = False


def iterator(value):
    return value.iterator or f"({value.code}).iter().copied()"
