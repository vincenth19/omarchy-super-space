//! Embedding client for llama-server's /v1/embeddings, over plain HTTP/1.1
//! (both servers are on 127.0.0.1, so no HTTP library is needed).

use serde_json::{Value, json};
use std::io::{Read, Write};
use std::net::TcpStream;
use std::time::Duration;

pub const DIM: usize = 768;

pub enum Input<'a> {
    Text(&'a str),
    /// A JPEG or PNG file, sent as a data URL.
    Image(&'a str),
}

#[derive(Debug)]
pub enum Fail {
    /// The server is not there (or still loading): try again later.
    Down(String),
    /// The server answered and refused this input (too long, bad image).
    Refused(String),
}

/// Normalized vectors, one per input, in order.
pub fn embed(url: &str, inputs: &[Input], timeout: Duration) -> Result<Vec<Vec<f32>>, Fail> {
    let mut items = Vec::with_capacity(inputs.len());
    for i in inputs {
        items.push(match i {
            Input::Text(t) => json!(t),
            Input::Image(p) => {
                let bytes = std::fs::read(p).map_err(|e| Fail::Refused(format!("{p}: {e}")))?;
                let mime = if p.to_lowercase().ends_with(".png") { "image/png" } else { "image/jpeg" };
                let url = format!("data:{mime};base64,{}", base64(&bytes));
                json!({"content": [{"type": "image_url", "image_url": {"url": url}}]})
            }
        });
    }
    let body = json!({ "input": items }).to_string();
    let (status, resp) = request("POST", url, body.as_bytes(), timeout).map_err(Fail::Down)?;
    if status == 503 {
        return Err(Fail::Down("server is loading".into()));
    }
    let text = || String::from_utf8_lossy(&resp[..resp.len().min(300)]).into_owned();
    if status != 200 {
        return Err(Fail::Refused(format!("HTTP {status}: {}", text())));
    }
    let v: Value = serde_json::from_slice(&resp).map_err(|e| Fail::Refused(format!("{e}: {}", text())))?;
    let mut out = vec![Vec::new(); inputs.len()];
    for (n, d) in v["data"].as_array().into_iter().flatten().enumerate() {
        let i = d["index"].as_u64().map_or(n, |i| i as usize);
        let e: Vec<f32> = d["embedding"].as_array().into_iter().flatten().filter_map(|x| x.as_f64()).map(|x| x as f32).collect();
        if let Some(slot) = out.get_mut(i) {
            *slot = normalize(e);
        }
    }
    if out.iter().any(|v| v.len() != DIM) {
        return Err(Fail::Refused(format!("expected {DIM}-dim vectors: {}", text())));
    }
    Ok(out)
}

pub fn healthy(url: &str) -> bool {
    let base = url.find("/v1/").map_or(url, |i| &url[..i]);
    matches!(request("GET", &format!("{base}/health"), b"", Duration::from_secs(1)), Ok((200, _)))
}

/// One HTTP/1.1 request with `Connection: close`; returns status and body.
pub fn request(method: &str, url: &str, body: &[u8], timeout: Duration) -> Result<(u16, Vec<u8>), String> {
    let rest = url.strip_prefix("http://").ok_or("only http:// URLs are supported")?;
    let (host, path) = rest.split_at(rest.find('/').unwrap_or(rest.len()));
    let path = if path.is_empty() { "/" } else { path };
    let mut s = TcpStream::connect(host).map_err(|e| format!("{host}: {e}"))?;
    s.set_read_timeout(Some(timeout)).map_err(|e| e.to_string())?;
    s.set_write_timeout(Some(timeout)).map_err(|e| e.to_string())?;
    let mut req = format!(
        "{method} {path} HTTP/1.1\r\nHost: {host}\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
        body.len()
    )
    .into_bytes();
    req.extend_from_slice(body);
    s.write_all(&req).map_err(|e| e.to_string())?;
    let mut buf = Vec::new();
    s.read_to_end(&mut buf).map_err(|e| e.to_string())?;
    let end = buf.windows(4).position(|w| w == b"\r\n\r\n").ok_or("bad HTTP response")?;
    let head = String::from_utf8_lossy(&buf[..end]);
    let status = head.split(' ').nth(1).and_then(|s| s.parse().ok()).ok_or("bad HTTP status")?;
    // llama-server sends Content-Length bodies (checked with b11718), never chunked.
    Ok((status, buf.split_off(end + 4)))
}

fn base64(data: &[u8]) -> String {
    const T: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    let mut s = String::with_capacity(data.len().div_ceil(3) * 4);
    for c in data.chunks(3) {
        let n = (c[0] as u32) << 16 | (*c.get(1).unwrap_or(&0) as u32) << 8 | *c.get(2).unwrap_or(&0) as u32;
        for i in 0..4 {
            s.push(if i <= c.len() { T[(n >> (18 - 6 * i) & 63) as usize] as char } else { '=' });
        }
    }
    s
}

pub fn normalize(mut v: Vec<f32>) -> Vec<f32> {
    let n = v.iter().map(|x| x * x).sum::<f32>().sqrt();
    if n > 0.0 {
        v.iter_mut().for_each(|x| *x /= n);
    }
    v
}

/// int8 with one scale: x ≈ q * scale.
pub fn quantize(v: &[f32]) -> (Vec<i8>, f32) {
    let max = v.iter().fold(0f32, |m, x| m.max(x.abs()));
    let scale = if max > 0.0 { max / 127.0 } else { 1.0 };
    (v.iter().map(|x| (x / scale).round() as i8).collect(), scale)
}

/// Integer dot product; times both scales it is the cosine.
pub fn dot(a: &[i8], b: &[i8]) -> i32 {
    a.iter().zip(b).map(|(&x, &y)| x as i32 * y as i32).sum()
}

#[cfg(test)]
mod t {
    #[test]
    fn base64_matches_rfc4648() {
        assert_eq!(super::base64(b"f"), "Zg==");
        assert_eq!(super::base64(b"fo"), "Zm8=");
        assert_eq!(super::base64(b"foobar"), "Zm9vYmFy");
    }
}
