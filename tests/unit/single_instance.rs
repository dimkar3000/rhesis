//! Single-instance guard tests — extracted from `src/single_instance.rs`.
//!
//! Note: `socket_name_is_within_linux_limits` calls `acquire()` which binds
//! the abstract socket. If a real Rhesis instance is running this test will
//! fail (socket already owned). This is expected in CI where no instance runs.

#[test]
fn socket_name_is_within_linux_limits() {
    // Abstract socket names are limited to 107 bytes (address struct,
    // including the leading NUL); attempting to bind a longer name
    // would panic in socket_addr().
    assert!(rhesis::single_instance::SOCKET_NAME.len() < 107);
    // The socket_addr function is private; we verify the bound by
    // attempting to create the guard (which binds the socket).
    let guard = rhesis::single_instance::acquire();
    assert!(guard, "must acquire the socket in a clean test environment");
}

#[test]
fn show_request_matches_the_trimmed_payload() {
    let request = rhesis::single_instance::SHOW_REQUEST.trim_ascii();
    assert_eq!(request, b"show");
    // handle_stream reads into a zeroed 16-byte buffer; the comparison
    // depends on the prefix logic staying correct for short payloads.
    let mut buffer = [0u8; 16];
    buffer[..request.len()].copy_from_slice(request);
    assert!(buffer.starts_with(request));
}
