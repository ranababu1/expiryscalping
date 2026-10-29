use super::*;
use es_core::{Depth5, DepthLevel, Paise, Tick, Ts};

const NFO_TOKEN: u32 = 0x00AB_CD02; // low byte 2 = NFO
const NIFTY_INDEX: u32 = 256_265; // low byte 9 = indices

fn depth() -> Depth5 {
    let mut d = Depth5::default();
    for i in 0..5 {
        d.bids[i] = DepthLevel {
            qty: 650 * (i as u32 + 1),
            price: Paise(10_000 - 5 * i as i64),
            orders: 3 + i as u16,
        };
        d.asks[i] = DepthLevel {
            qty: 325 * (i as u32 + 1),
            price: Paise(10_005 + 5 * i as i64),
            orders: 2 + i as u16,
        };
    }
    d
}

fn full() -> FullPacket {
    FullPacket {
        token: NFO_TOKEN,
        ltp: Paise(10_005),
        ltq: 65,
        avg_price: Paise(9_900),
        volume: 1_234_567,
        buy_qty: 10_000,
        sell_qty: 9_000,
        open: Paise(9_000),
        high: Paise(11_000),
        low: Paise(8_500),
        close: Paise(9_500),
        last_trade_ts: 1_793_000_000,
        oi: 4_567_890,
        oi_high: 5_000_000,
        oi_low: 4_000_000,
        exchange_ts: 1_793_000_001,
        depth: depth(),
    }
}

fn parse(frame: &[u8]) -> (Result<FrameKind, ParseError>, Vec<Tick>) {
    let mut out = Vec::new();
    let r = Parser.parse_frame(frame, Ts(42), &mut out);
    (r, out)
}

#[test]
fn heartbeat() {
    assert_eq!(parse(&[0]).0, Ok(FrameKind::Heartbeat));
}

#[test]
fn empty_frame_is_error() {
    assert_eq!(parse(&[]).0, Err(ParseError::Empty));
}

#[test]
fn full_mode_packet() {
    let mut fb = FrameBuilder::new();
    fb.push(&full().encode());
    let (r, ticks) = parse(&fb.finish());
    assert_eq!(r, Ok(FrameKind::Data { ticks: 1, skipped: 0 }));
    let t = ticks[0];
    assert_eq!(t.token, NFO_TOKEN);
    assert_eq!(t.ltp, Paise(10_005));
    assert_eq!(t.ltq, 65);
    assert_eq!(t.volume, 1_234_567);
    assert_eq!(t.close, Paise(9_500));
    assert_eq!(t.oi, 4_567_890);
    assert_eq!(t.exchange_ts, 1_793_000_001);
    assert_eq!(t.recv, Ts(42));
    assert_eq!(t.depth, Some(depth()));
}

#[test]
fn golden_bytes_full_packet_offsets() {
    // Hand-check the documented offsets on the raw bytes.
    let b = full().encode();
    assert_eq!(b.len(), 184);
    assert_eq!(&b[0..4], &NFO_TOKEN.to_be_bytes());
    assert_eq!(&b[4..8], &10_005i32.to_be_bytes());
    assert_eq!(&b[48..52], &4_567_890u32.to_be_bytes());
    assert_eq!(&b[60..64], &1_793_000_001i32.to_be_bytes());
    // first bid: qty, price, orders
    assert_eq!(&b[64..68], &650u32.to_be_bytes());
    assert_eq!(&b[68..72], &10_000i32.to_be_bytes());
    assert_eq!(&b[72..74], &3u16.to_be_bytes());
    // first ask starts after 5 bids of 12 bytes
    assert_eq!(&b[124..128], &325u32.to_be_bytes());
    assert_eq!(&b[128..132], &10_005i32.to_be_bytes());
}

#[test]
fn quote_mode_packet() {
    let mut fb = FrameBuilder::new();
    fb.push(&full().encode()[..LEN_QUOTE]);
    let (r, ticks) = parse(&fb.finish());
    assert_eq!(r, Ok(FrameKind::Data { ticks: 1, skipped: 0 }));
    assert_eq!(ticks[0].volume, 1_234_567);
    assert_eq!(ticks[0].close, Paise(9_500));
    assert_eq!(ticks[0].oi, 0);
    assert_eq!(ticks[0].depth, None);
}

#[test]
fn ltp_mode_packet() {
    let mut fb = FrameBuilder::new();
    fb.push(&full().encode()[..LEN_LTP]);
    let (_, ticks) = parse(&fb.finish());
    assert_eq!(ticks[0].ltp, Paise(10_005));
    assert_eq!(ticks[0].depth, None);
}

#[test]
fn index_packets() {
    let ip = IndexPacket {
        token: NIFTY_INDEX,
        ltp: Paise(2_450_010),
        close: Paise(2_440_000),
        exchange_ts: 1_793_000_002,
        ..IndexPacket::default()
    };
    let enc = ip.encode();
    let mut fb = FrameBuilder::new();
    fb.push(&enc).push(&enc[..LEN_INDEX_QUOTE]);
    let (r, ticks) = parse(&fb.finish());
    assert_eq!(r, Ok(FrameKind::Data { ticks: 2, skipped: 0 }));
    assert_eq!(ticks[0].ltp, Paise(2_450_010));
    assert_eq!(ticks[0].close, Paise(2_440_000));
    assert_eq!(ticks[0].exchange_ts, 1_793_000_002);
    assert_eq!(ticks[1].exchange_ts, 0);
}

#[test]
fn multi_packet_frame_mixed_modes() {
    let ip = IndexPacket {
        token: NIFTY_INDEX,
        ltp: Paise(2_450_010),
        ..IndexPacket::default()
    };
    let mut fb = FrameBuilder::new();
    fb.push(&ip.encode())
        .push(&full().encode())
        .push(&full().encode()[..LEN_QUOTE])
        .push(&full().encode()[..LEN_LTP]);
    let (r, ticks) = parse(&fb.finish());
    assert_eq!(r, Ok(FrameKind::Data { ticks: 4, skipped: 0 }));
    assert_eq!(ticks[0].token, NIFTY_INDEX);
    assert!(ticks[1].depth.is_some());
}

#[test]
fn unsupported_segment_and_unknown_length_are_skipped() {
    let mut cds = full().encode();
    cds[0..4].copy_from_slice(&0x0000_1003u32.to_be_bytes()); // low byte 3 = CDS
    let mut fb = FrameBuilder::new();
    fb.push(&cds).push(&[0u8; 13]).push(&full().encode());
    let (r, ticks) = parse(&fb.finish());
    assert_eq!(r, Ok(FrameKind::Data { ticks: 1, skipped: 2 }));
    assert_eq!(ticks.len(), 1);
}

#[test]
fn truncated_frames_are_errors() {
    let mut fb = FrameBuilder::new();
    fb.push(&full().encode());
    let frame = fb.finish();
    for cut in 2..frame.len() {
        let (r, _) = parse(&frame[..cut]);
        assert!(
            matches!(r, Err(ParseError::Truncated { .. })),
            "cut at {cut}: {r:?}"
        );
    }
}

#[test]
fn trailing_bytes_are_an_error_and_add_no_ticks() {
    let mut fb = FrameBuilder::new();
    fb.push(&full().encode());
    let mut frame = fb.finish();
    frame.extend_from_slice(&[1, 2, 3]);
    let (r, ticks) = parse(&frame);
    assert_eq!(r, Err(ParseError::Trailing { extra: 3 }));
    assert!(ticks.is_empty());
}

#[test]
fn random_bytes_never_panic() {
    // Deterministic xorshift so the test is reproducible without extra crates.
    let mut s: u64 = 0x9E37_79B9_7F4A_7C15;
    let mut next = || {
        s ^= s << 13;
        s ^= s >> 7;
        s ^= s << 17;
        s
    };
    let mut out = Vec::new();
    for _ in 0..20_000 {
        let len = (next() % 400) as usize;
        let mut buf: Vec<u8> = (0..len).map(|_| next() as u8).collect();
        // Make some frames start with a plausible packet count.
        if len > 2 && next() % 2 == 0 {
            buf[0] = 0;
            buf[1] = (next() % 4) as u8;
        }
        out.clear();
        let _ = Parser.parse_frame(&buf, Ts(0), &mut out);
    }
}
