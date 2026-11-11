//! The `sha1` library upstream ships in `runtime/lua/sha1` (which luagen
//! does not translate): hex and binary digests and HMACs, the table callable
//! as `sha1(text)`. Upstream hashes account and character names with it
//! (`common.sha1`).

use lua::rt::{err, func};
use lua::{LResult, TableRef, Value};

fn text(a: &[Value], i: usize, f: &str) -> LResult<Vec<u8>> {
    match a.get(i) {
        Some(Value::Str(s)) => Ok(s.as_bytes().to_vec()),
        Some(Value::Number(n)) => Ok(lua::to_string(&Value::Number(*n)).as_bytes().to_vec()),
        _ => err(format!(
            "bad argument #{} to '{f}' (string expected)",
            i + 1
        )),
    }
}

fn digest(data: &[u8]) -> [u8; 20] {
    sha1_smol::Sha1::from(data).digest().bytes()
}

fn hex(bytes: &[u8]) -> String {
    bytes.iter().map(|b| format!("{b:02x}")).collect()
}

fn hmac(key: &[u8], data: &[u8]) -> [u8; 20] {
    const BLOCK: usize = 64;
    let mut k = if key.len() > BLOCK {
        digest(key).to_vec()
    } else {
        key.to_vec()
    };
    k.resize(BLOCK, 0);
    let mut inner: Vec<u8> = k.iter().map(|b| b ^ 0x36).collect();
    inner.extend_from_slice(data);
    let mut outer: Vec<u8> = k.iter().map(|b| b ^ 0x5c).collect();
    outer.extend_from_slice(&digest(&inner));
    digest(&outer)
}

fn bytes(b: &[u8]) -> Value {
    Value::Str(b.into())
}

pub fn library() -> Value {
    let t = TableRef::new();
    t.set_str("version", Value::str("0.6.0"));
    t.set_str(
        "sha1",
        func("sha1.sha1", |a| {
            Ok(vec![Value::str(&hex(&digest(&text(a, 0, "sha1")?)))])
        }),
    );
    t.set_str(
        "binary",
        func("sha1.binary", |a| {
            Ok(vec![bytes(&digest(&text(a, 0, "binary")?))])
        }),
    );
    t.set_str(
        "hmac",
        func("sha1.hmac", |a| {
            Ok(vec![Value::str(&hex(&hmac(
                &text(a, 0, "hmac")?,
                &text(a, 1, "hmac")?,
            )))])
        }),
    );
    t.set_str(
        "hmac_binary",
        func("sha1.hmac_binary", |a| {
            Ok(vec![bytes(&hmac(
                &text(a, 0, "hmac_binary")?,
                &text(a, 1, "hmac_binary")?,
            ))])
        }),
    );
    let meta = TableRef::new();
    meta.set_str(
        "__call",
        func("sha1.__call", |a| {
            Ok(vec![Value::str(&hex(&digest(&text(a, 1, "sha1")?)))])
        }),
    );
    t.borrow_mut().meta = Some(meta);
    Value::Table(t)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn known_digests() {
        assert_eq!(
            hex(&digest(b"abc")),
            "a9993e364706816aba3e25717850c26c9cd0d89d"
        );
        // RFC 2202 test case 2
        assert_eq!(
            hex(&hmac(b"Jefe", b"what do ya want for nothing?")),
            "effcdf6ae5eb2fa2d27416d5f184df9c259a7c79"
        );
    }
}
