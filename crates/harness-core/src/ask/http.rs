//! The slice of HTTP/1.1 a decision page needs: one request read from a
//! connection, and one response that closes it.

/// The largest request head read. A browser's request to this server is a
/// fraction of it.
pub const HEAD_LIMIT: usize = 16 * 1024;

/// The largest answer set taken: well past what a page asking a hundred things
/// sends.
pub const BODY_LIMIT: usize = 64 * 1024;

/// The statuses this server answers with.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Status {
    Ok,
    BadRequest,
    Forbidden,
    NotFound,
    Conflict,
    LengthRequired,
    PayloadTooLarge,
    HeaderFieldsTooLarge,
}

impl Status {
    fn line(self) -> &'static str {
        match self {
            Self::Ok => "200 OK",
            Self::BadRequest => "400 Bad Request",
            Self::Forbidden => "403 Forbidden",
            Self::NotFound => "404 Not Found",
            Self::Conflict => "409 Conflict",
            Self::LengthRequired => "411 Length Required",
            Self::PayloadTooLarge => "413 Payload Too Large",
            Self::HeaderFieldsTooLarge => "431 Request Header Fields Too Large",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Request {
    pub method: String,
    /// The request target without its query.
    pub path: String,
    pub host: Option<String>,
    pub origin: Option<String>,
    pub body: Vec<u8>,
}

/// What the bytes read so far from a connection amount to.
#[derive(Debug, PartialEq, Eq)]
pub enum Parsed {
    /// The request is not whole yet.
    Partial,
    Whole(Request),
    /// The request cannot be taken, and is answered with this.
    Refused(Status),
}

/// Parse the bytes read so far from one connection. A body is read only by
/// its `Content-Length`: a chunked one is refused rather than decoded.
pub fn parse(read: &[u8]) -> Parsed {
    let mut headers = [httparse::EMPTY_HEADER; 64];
    let mut request = httparse::Request::new(&mut headers);
    let head = match request.parse(read) {
        Ok(httparse::Status::Complete(head)) => head,
        Ok(httparse::Status::Partial) if read.len() > HEAD_LIMIT => {
            return Parsed::Refused(Status::HeaderFieldsTooLarge);
        }
        Ok(httparse::Status::Partial) => return Parsed::Partial,
        Err(httparse::Error::TooManyHeaders) => {
            return Parsed::Refused(Status::HeaderFieldsTooLarge);
        }
        Err(_) => return Parsed::Refused(Status::BadRequest),
    };
    if head > HEAD_LIMIT {
        return Parsed::Refused(Status::HeaderFieldsTooLarge);
    }
    let header = |name: &str| {
        request
            .headers
            .iter()
            .find(|h| h.name.eq_ignore_ascii_case(name))
            .map(|h| h.value)
    };
    if header("transfer-encoding").is_some() {
        return Parsed::Refused(Status::LengthRequired);
    }
    let length = match header("content-length") {
        None => 0,
        Some(value) => match std::str::from_utf8(value).ok().and_then(|v| v.parse().ok()) {
            Some(length) => length,
            None => return Parsed::Refused(Status::BadRequest),
        },
    };
    if length > BODY_LIMIT {
        return Parsed::Refused(Status::PayloadTooLarge);
    }
    if read.len() - head < length {
        return Parsed::Partial;
    }
    let text =
        |value: Option<&[u8]>| value.and_then(|v| std::str::from_utf8(v).ok().map(String::from));
    let target = request.path.unwrap_or_default();
    Parsed::Whole(Request {
        method: request.method.unwrap_or_default().to_string(),
        path: target.split('?').next().unwrap_or_default().to_string(),
        host: text(header("host")),
        origin: text(header("origin")),
        body: read[head..head + length].to_vec(),
    })
}

/// One response, whole, and nothing after it: every connection carries one
/// exchange. Nothing is cached, nothing is sniffed, and no address leaves in
/// a `Referer`, since the path carries the token.
pub fn response(status: Status, content_type: &str, body: &[u8]) -> Vec<u8> {
    let head = format!(
        "HTTP/1.1 {}\r\nContent-Type: {content_type}\r\nContent-Length: {}\r\n\
         Cache-Control: no-store\r\nX-Content-Type-Options: nosniff\r\n\
         Referrer-Policy: no-referrer\r\nConnection: close\r\n\r\n",
        status.line(),
        body.len()
    );
    let mut bytes = Vec::with_capacity(head.len() + body.len());
    bytes.extend_from_slice(head.as_bytes());
    bytes.extend_from_slice(body);
    bytes
}

#[cfg(test)]
mod tests {
    use super::*;

    fn whole(bytes: &[u8]) -> Request {
        match parse(bytes) {
            Parsed::Whole(request) => request,
            other => panic!("{other:?}"),
        }
    }

    #[test]
    fn a_get_is_read_with_its_host_and_without_its_query() {
        let request = whole(b"GET /t/page/p.html?x=1 HTTP/1.1\r\nHost: 127.0.0.1:9\r\n\r\n");
        assert_eq!(request.method, "GET");
        assert_eq!(request.path, "/t/page/p.html");
        assert_eq!(request.host.as_deref(), Some("127.0.0.1:9"));
        assert_eq!(request.origin, None);
    }

    #[test]
    fn a_post_waits_for_the_body_its_length_names() {
        let head = b"POST /t/answers HTTP/1.1\r\nHost: h\r\nOrigin: o\r\nContent-Length: 5\r\n\r\n";
        assert_eq!(parse(head), Parsed::Partial);
        let mut bytes = head.to_vec();
        bytes.extend_from_slice(b"{\"a\":");
        let request = whole(&bytes);
        assert_eq!(request.body, b"{\"a\":");
        assert_eq!(request.origin.as_deref(), Some("o"));
    }

    #[test]
    fn what_this_server_does_not_read_is_refused() {
        let over = format!(
            "POST / HTTP/1.1\r\nContent-Length: {}\r\n\r\n",
            BODY_LIMIT + 1
        );
        for (bytes, status) in [
            (over.into_bytes(), Status::PayloadTooLarge),
            (
                b"POST / HTTP/1.1\r\nTransfer-Encoding: chunked\r\n\r\n".to_vec(),
                Status::LengthRequired,
            ),
            (
                b"POST / HTTP/1.1\r\nContent-Length: x\r\n\r\n".to_vec(),
                Status::BadRequest,
            ),
            (b"\x00\x01 garbage\r\n\r\n".to_vec(), Status::BadRequest),
            (
                format!("GET / HTTP/1.1\r\nX: {}", "a".repeat(HEAD_LIMIT)).into_bytes(),
                Status::HeaderFieldsTooLarge,
            ),
        ] {
            assert_eq!(parse(&bytes), Parsed::Refused(status));
        }
    }

    #[test]
    fn a_response_closes_and_leaks_no_address() {
        let text = String::from_utf8(response(Status::NotFound, "text/plain", b"")).unwrap();
        assert!(text.starts_with("HTTP/1.1 404 Not Found\r\n"));
        for header in [
            "Connection: close",
            "Referrer-Policy: no-referrer",
            "Cache-Control: no-store",
        ] {
            assert!(text.contains(header), "{header}");
        }
    }
}
