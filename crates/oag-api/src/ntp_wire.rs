//! Minimal, hand-rolled NTPv4 (RFC 5905) packet codec -- just enough to (a)
//! query a real upstream NTP server as a client, and (b) answer a real NTP
//! client as a server, using a *verified* (quorum-checked) time instead of
//! this peer's own raw system clock. Deliberately not a full
//! implementation: no extension fields, no authentication, no leap-second
//! table, no clock-filter/Marzullo's-algorithm precision refinement --
//! "hand-roll the narrow useful subset, not the whole spec," the same
//! posture this project already takes for its Prometheus exporter and Kubo
//! HTTP client (see `OPEN_QUESTIONS.md`).

use std::net::SocketAddr;
use std::time::{SystemTime, UNIX_EPOCH};

use tokio::net::UdpSocket;
use tokio::time::Duration;

/// Seconds between the NTP epoch (1900-01-01) and the Unix epoch
/// (1970-01-01) -- the one magic constant every NTP implementation needs.
const NTP_UNIX_EPOCH_DELTA_SECONDS: u64 = 2_208_988_800;
pub const NTP_PACKET_LEN: usize = 48;

pub fn now_unix_ms() -> i64 {
    SystemTime::now().duration_since(UNIX_EPOCH).unwrap_or_default().as_millis() as i64
}

fn unix_ms_to_ntp_timestamp(unix_ms: i64) -> (u32, u32) {
    let unix_ms = unix_ms.max(0) as u64;
    let unix_seconds = unix_ms / 1000;
    let ms_fraction = unix_ms % 1000;
    let ntp_seconds = (unix_seconds + NTP_UNIX_EPOCH_DELTA_SECONDS) as u32;
    let ntp_fraction = ((ms_fraction * (1u64 << 32)) / 1000) as u32;
    (ntp_seconds, ntp_fraction)
}

fn ntp_timestamp_to_unix_ms(seconds: u32, fraction: u32) -> i64 {
    let unix_seconds = (seconds as i64) - (NTP_UNIX_EPOCH_DELTA_SECONDS as i64);
    let ms_fraction = ((fraction as u64) * 1000) / (1u64 << 32);
    unix_seconds * 1000 + ms_fraction as i64
}

fn write_timestamp(buf: &mut [u8], offset: usize, unix_ms: i64) {
    let (seconds, fraction) = unix_ms_to_ntp_timestamp(unix_ms);
    buf[offset..offset + 4].copy_from_slice(&seconds.to_be_bytes());
    buf[offset + 4..offset + 8].copy_from_slice(&fraction.to_be_bytes());
}

fn read_timestamp(buf: &[u8], offset: usize) -> i64 {
    let seconds = u32::from_be_bytes(buf[offset..offset + 4].try_into().unwrap());
    let fraction = u32::from_be_bytes(buf[offset + 4..offset + 8].try_into().unwrap());
    ntp_timestamp_to_unix_ms(seconds, fraction)
}

/// Build a mode-3 (client) request packet. `transmit_unix_ms` becomes the
/// packet's own Transmit Timestamp (T1) -- a well-behaved server always
/// echoes it back verbatim as the response's Origin Timestamp, which is how
/// a client matches a response to its own request and computes offset/delay.
pub fn build_client_request(transmit_unix_ms: i64) -> [u8; NTP_PACKET_LEN] {
    let mut buf = [0u8; NTP_PACKET_LEN];
    // LI=0 (no warning), VN=4, Mode=3 (client) -- 0b00_100_011 = 0x23.
    buf[0] = 0x23;
    write_timestamp(&mut buf, 40, transmit_unix_ms); // Transmit Timestamp
    buf
}

#[derive(Debug, Clone, Copy)]
pub struct ClientRequestInfo {
    pub transmit_unix_ms: i64,
}

/// Parses just enough of an incoming mode-3 client request to answer it --
/// its own Transmit Timestamp, which must be echoed back as our response's
/// Origin Timestamp.
pub fn parse_client_request(buf: &[u8]) -> Option<ClientRequestInfo> {
    if buf.len() < NTP_PACKET_LEN {
        return None;
    }
    let mode = buf[0] & 0b0000_0111;
    if mode != 3 {
        return None; // not a client request we know how to answer
    }
    Some(ClientRequestInfo { transmit_unix_ms: read_timestamp(buf, 40) })
}

/// Builds a mode-4 (server) response to a client request, reporting
/// `verified_unix_ms` (this gateway's quorum-verified time estimate, never
/// this host's own unadjusted system clock) as both the Receive and
/// Transmit Timestamps -- a hand-rolled simplification: real NTP servers
/// timestamp receive and transmit separately (processing takes real, if
/// tiny, time), but one verified estimate covers both closely enough for
/// this gateway's purpose.
pub fn build_server_response(origin_unix_ms: i64, verified_unix_ms: i64) -> [u8; NTP_PACKET_LEN] {
    let mut buf = [0u8; NTP_PACKET_LEN];
    // LI=0, VN=4, Mode=4 (server) -- 0b00_100_100 = 0x24.
    buf[0] = 0x24;
    buf[1] = 2; // Stratum 2 -- "synchronized via another source," never claims to BE a stratum-1 reference clock.
    buf[2] = 4; // Poll interval, log2 seconds -- plausible, not measured; clients mostly ignore it for a one-shot query.
    buf[3] = 0xEC; // Precision, log2 seconds (~2^-20s) -- plausible, not measured.
    buf[12..16].copy_from_slice(b"OAGG"); // Reference ID: marks this as OAG-gateway-derived time, not a real stratum-1 source.
    write_timestamp(&mut buf, 16, verified_unix_ms); // Reference Timestamp
    write_timestamp(&mut buf, 24, origin_unix_ms); // Origin Timestamp (echoes the client's own T1)
    write_timestamp(&mut buf, 32, verified_unix_ms); // Receive Timestamp (T2)
    write_timestamp(&mut buf, 40, verified_unix_ms); // Transmit Timestamp (T3)
    buf
}

#[derive(Debug, thiserror::Error)]
pub enum NtpUpstreamError {
    #[error("could not resolve upstream address '{0}': {1}")]
    InvalidAddress(String, std::io::Error),
    #[error("no addresses resolved for '{0}'")]
    NoAddresses(String),
    #[error("socket error: {0}")]
    Socket(std::io::Error),
    #[error("request timed out")]
    Timeout,
    #[error("malformed response, or response from an unexpected sender")]
    MalformedResponse,
}

const UPSTREAM_TIMEOUT: Duration = Duration::from_secs(5);

/// Query one real upstream NTP server (`addr`, e.g. "pool.ntp.org:123"),
/// using the real, full four-timestamp NTP client algorithm (not the
/// simpler RTT/2 heuristic the OAG peer-to-peer gateway hops use over plain
/// signed JSON) -- this is a real NTP exchange over the real wire protocol,
/// so there's no reason to approximate it.
pub async fn query_upstream(addr: &str) -> Result<i64, NtpUpstreamError> {
    let socket = UdpSocket::bind("0.0.0.0:0").await.map_err(NtpUpstreamError::Socket)?;
    let target: SocketAddr = tokio::net::lookup_host(addr)
        .await
        .map_err(|e| NtpUpstreamError::InvalidAddress(addr.to_string(), e))?
        .next()
        .ok_or_else(|| NtpUpstreamError::NoAddresses(addr.to_string()))?;

    let t1 = now_unix_ms();
    let request = build_client_request(t1);
    tokio::time::timeout(UPSTREAM_TIMEOUT, socket.send_to(&request, target))
        .await
        .map_err(|_| NtpUpstreamError::Timeout)?
        .map_err(NtpUpstreamError::Socket)?;

    let mut buf = [0u8; NTP_PACKET_LEN];
    let (len, from) = tokio::time::timeout(UPSTREAM_TIMEOUT, socket.recv_from(&mut buf))
        .await
        .map_err(|_| NtpUpstreamError::Timeout)?
        .map_err(NtpUpstreamError::Socket)?;
    let t4 = now_unix_ms();

    // Reject a packet from anyone other than the server we actually asked
    // -- a bare bound UDP socket will happily deliver traffic from any
    // sender, and this feeds directly into a "verified" time reading.
    if from != target || len < NTP_PACKET_LEN || (buf[0] & 0b0000_0111) != 4 {
        return Err(NtpUpstreamError::MalformedResponse);
    }
    let t2 = read_timestamp(&buf, 32);
    let t3 = read_timestamp(&buf, 40);

    // Standard NTP offset formula: ((T2 - T1) + (T3 - T4)) / 2.
    let offset_ms = ((t2 - t1) + (t3 - t4)) / 2;
    Ok(t4 + offset_ms)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn timestamp_round_trips_through_ntp_format() {
        let now = now_unix_ms();
        let (seconds, fraction) = unix_ms_to_ntp_timestamp(now);
        let back = ntp_timestamp_to_unix_ms(seconds, fraction);
        // Sub-millisecond rounding in the 32-bit fraction field is expected;
        // allow a tiny tolerance rather than requiring bit-exact equality.
        assert!((back - now).abs() <= 1, "expected {now}, got {back}");
    }

    #[test]
    fn client_request_round_trips() {
        let t1 = 1_700_000_000_123;
        let packet = build_client_request(t1);
        let info = parse_client_request(&packet).unwrap();
        assert!((info.transmit_unix_ms - t1).abs() <= 1);
    }

    #[test]
    fn parse_client_request_rejects_a_server_response() {
        let response = build_server_response(1_000, 2_000);
        assert!(parse_client_request(&response).is_none());
    }

    #[test]
    fn server_response_echoes_the_clients_origin_timestamp() {
        let origin = 1_700_000_000_000;
        let verified = 1_700_000_000_500;
        let response = build_server_response(origin, verified);
        assert_eq!(read_timestamp(&response, 24), origin);
        assert!((read_timestamp(&response, 32) - verified).abs() <= 1);
        assert!((read_timestamp(&response, 40) - verified).abs() <= 1);
        assert_eq!(&response[12..16], b"OAGG");
        assert_eq!(response[0] & 0b0000_0111, 4, "mode must be 4 (server)");
    }
}
