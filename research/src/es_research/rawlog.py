"""Reader for the engine's raw tick log (engine/crates/es-archiver).

Format, little-endian:

    magic   b"ESRAW\\x01"                                  (6 bytes, once)
    record  kind u8 | recv_ns i64 | len u32 | bytes[len]   (repeated)

kind: 0 binary WebSocket frame, 1 instrument master CSV, 2 JSON text frame.
"""

from __future__ import annotations

import struct
import sys
from collections import Counter
from collections.abc import Iterator
from dataclasses import dataclass
from enum import IntEnum
from pathlib import Path
from typing import BinaryIO

MAGIC = b"ESRAW\x01"
MAX_RECORD = 64 * 1024 * 1024
_HEADER = struct.Struct("<BqI")


class Kind(IntEnum):
    BINARY = 0
    INSTRUMENTS = 1
    TEXT = 2


class RawLogError(Exception):
    pass


@dataclass(frozen=True)
class Record:
    kind: Kind
    recv_ns: int
    data: bytes


def read(stream: BinaryIO) -> Iterator[Record]:
    """Yields records. A truncated last record (a crash mid-write) ends the log quietly."""
    if stream.read(len(MAGIC)) != MAGIC:
        raise RawLogError("not a raw tick log (bad magic)")
    while True:
        head = stream.read(_HEADER.size)
        if len(head) < _HEADER.size:
            return
        kind, recv_ns, length = _HEADER.unpack(head)
        if kind not in Kind._value2member_map_:
            raise RawLogError(f"unknown record kind {kind}")
        if length > MAX_RECORD:
            raise RawLogError(f"record too large: {length} bytes")
        data = stream.read(length)
        if len(data) < length:
            return
        yield Record(Kind(kind), recv_ns, data)


def summary(path: Path) -> dict[str, int]:
    counts: Counter[str] = Counter()
    first = last = 0
    with path.open("rb") as f:
        for rec in read(f):
            counts[rec.kind.name.lower()] += 1
            counts["bytes"] += len(rec.data)
            first = first or rec.recv_ns
            last = rec.recv_ns
    return {**counts, "first_ns": first, "last_ns": last}


def main(argv: list[str]) -> int:
    if len(argv) != 2:
        print("usage: python -m es_research.rawlog <file>", file=sys.stderr)
        return 2
    for key, value in summary(Path(argv[1])).items():
        print(f"{key}: {value}")
    return 0


if __name__ == "__main__":
    raise SystemExit(main(sys.argv))
