import io
import struct

import pytest

from es_research.rawlog import MAGIC, Kind, RawLogError, read


def record(kind: int, ns: int, data: bytes) -> bytes:
    return struct.pack("<BqI", kind, ns, len(data)) + data


def test_reads_records_in_order() -> None:
    buf = MAGIC + record(1, 10, b"csv") + record(0, 20, b"\x00\x01") + record(2, 30, b"{}")
    recs = list(read(io.BytesIO(buf)))
    assert [r.kind for r in recs] == [Kind.INSTRUMENTS, Kind.BINARY, Kind.TEXT]
    assert [r.recv_ns for r in recs] == [10, 20, 30]
    assert recs[1].data == b"\x00\x01"


def test_truncated_tail_ends_quietly() -> None:
    buf = MAGIC + record(0, 1, b"abcd") + record(0, 2, b"efgh")[:-2]
    assert len(list(read(io.BytesIO(buf)))) == 1


def test_rejects_bad_magic_and_kind() -> None:
    with pytest.raises(RawLogError):
        list(read(io.BytesIO(b"NOTRAW" + record(0, 1, b""))))
    with pytest.raises(RawLogError):
        list(read(io.BytesIO(MAGIC + record(9, 1, b""))))
