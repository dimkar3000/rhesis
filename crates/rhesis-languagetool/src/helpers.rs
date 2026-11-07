pub fn normalize_address(address: &str) -> String {
    let address = address.trim().trim_end_matches('/');
    if address.contains("://") {
        address.to_string()
    } else {
        format!("http://{address}")
    }
}

mod unit_tests {
    #[test]
    fn normalize_address_keeps_and_adds_schemes() {
        assert_eq!(
            super::normalize_address(" http://localhost:2689/ "),
            "http://localhost:2689"
        );
        assert_eq!(
            super::normalize_address("localhost:2689//"),
            "http://localhost:2689"
        );
        assert_eq!(
            super::normalize_address("https://lt.example.com"),
            "https://lt.example.com"
        );
        assert_eq!(super::normalize_address("unix:"), "http://unix:");
    }
}
