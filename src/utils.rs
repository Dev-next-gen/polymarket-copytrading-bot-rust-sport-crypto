//! Utility functions and helpers for the Polymarket trading bot.
//!
//! This module provides common utility functions used across the application,
//! including address validation, error formatting, and other helper functions.

use anyhow::Result;

/// Validates an Ethereum address format.
///
/// # Arguments
/// * `s` - The address string to validate (with or without 0x prefix)
///
/// # Returns
/// * `true` if the address is a valid 40-character hexadecimal string
/// * `false` otherwise
///
/// # Examples
/// ```
/// use polymarket_trading_bot::utils::is_valid_eth_address;
///
/// assert!(is_valid_eth_address("0x742d35Cc6634C0532925a3b8D3Ac3E3F12345678"));
/// assert!(is_valid_eth_address("742d35Cc6634C0532925a3b8D3Ac3E3F12345678"));
/// assert!(!is_valid_eth_address("invalid"));
/// assert!(!is_valid_eth_address(""));
/// assert!(!is_valid_eth_address("0x123")); // too short
/// ```
pub fn is_valid_eth_address(s: &str) -> bool {
    if s.is_empty() {
        return false;
    }

    let s = s.strip_prefix("0x").unwrap_or(s);

    // Check length: Ethereum addresses are exactly 40 hex characters
    if s.len() != 40 {
        return false;
    }

    // Check that all characters are valid hexadecimal
    s.chars().all(|c| c.is_ascii_hexdigit())
}

/// Normalizes an Ethereum address to lowercase format with 0x prefix.
///
/// # Arguments
/// * `address` - The address to normalize
///
/// # Returns
/// * Normalized address string or error if invalid format
///
/// # Examples
/// ```
/// use polymarket_trading_bot::utils::normalize_eth_address;
///
/// let addr = normalize_eth_address("742d35Cc6634C0532925a3b8D3Ac3E3F12345678").unwrap();
/// assert_eq!(addr, "0x742d35cc6634c0532925a3b8d3ac3e3f12345678");
///
/// let addr = normalize_eth_address("0xABC123").is_err();
/// assert!(addr); // Should fail for invalid address
/// ```
pub fn normalize_eth_address(address: &str) -> Result<String> {
    if !is_valid_eth_address(address) {
        return Err(anyhow::anyhow!("Invalid Ethereum address format: {}", address));
    }

    let normalized = if address.starts_with("0x") {
        address.to_lowercase()
    } else {
        format!("0x{}", address.to_lowercase())
    };

    Ok(normalized)
}

/// Formats a duration in seconds to a human-readable string.
///
/// # Arguments
/// * `seconds` - Duration in seconds
///
/// # Returns
/// * Human-readable duration string
///
/// # Examples
/// ```
/// use polymarket_trading_bot::utils::format_duration;
///
/// assert_eq!(format_duration(65), "1m 5s");
/// assert_eq!(format_duration(3661), "1h 1m 1s");
/// assert_eq!(format_duration(30), "30s");
/// ```
pub fn format_duration(seconds: u64) -> String {
    let hours = seconds / 3600;
    let minutes = (seconds % 3600) / 60;
    let secs = seconds % 60;

    match (hours, minutes, secs) {
        (0, 0, s) => format!("{}s", s),
        (0, m, s) => format!("{}m {}s", m, s),
        (h, m, s) => format!("{}h {}m {}s", h, m, s),
    }
}

/// Truncates a string to a maximum length, adding "..." if truncated.
///
/// # Arguments
/// * `s` - String to truncate
/// * `max_len` - Maximum length (including "...")
///
/// # Returns
/// * Truncated string
pub fn truncate_string(s: &str, max_len: usize) -> String {
    if max_len <= 3 {
        return s.chars().take(max_len).collect();
    }

    if s.len() <= max_len {
        s.to_string()
    } else {
        format!("{}...", &s[..max_len - 3])
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_is_valid_eth_address() {
        // Valid addresses
        assert!(is_valid_eth_address("0x742d35Cc6634C0532925a3b8D3Ac3E3F12345678"));
        assert!(is_valid_eth_address("742d35Cc6634C0532925a3b8D3Ac3E3F12345678"));
        assert!(is_valid_eth_address("0xffffffffffffffffffffffffffffffffffffffff"));
        assert!(is_valid_eth_address("0x0000000000000000000000000000000000000000"));

        // Invalid addresses
        assert!(!is_valid_eth_address(""));
        assert!(!is_valid_eth_address("invalid"));
        assert!(!is_valid_eth_address("0x123")); // too short
        assert!(!is_valid_eth_address("0x742d35Cc6634C0532925a3b8D3Ac3E3F123456789")); // too long
        assert!(!is_valid_eth_address("0x742d35Cc6634C0532925a3b8D3Ac3E3Gx2345678")); // invalid char 'G'
    }

    #[test]
    fn test_normalize_eth_address() {
        let addr1 = normalize_eth_address("742d35Cc6634C0532925a3b8D3Ac3E3F12345678").unwrap();
        assert_eq!(addr1, "0x742d35cc6634c0532925a3b8d3ac3e3f12345678");

        let addr2 = normalize_eth_address("0xABC123def456789012345678901234567890ABCD").unwrap();
        assert_eq!(addr2, "0xabc123def456789012345678901234567890abcd");

        // Invalid address should error
        assert!(normalize_eth_address("0xABC123").is_err());
        assert!(normalize_eth_address("").is_err());
    }

    #[test]
    fn test_format_duration() {
        assert_eq!(format_duration(30), "30s");
        assert_eq!(format_duration(65), "1m 5s");
        assert_eq!(format_duration(3661), "1h 1m 1s");
        assert_eq!(format_duration(7200), "2h 0m 0s");
    }

    #[test]
    fn test_truncate_string() {
        assert_eq!(truncate_string("hello", 10), "hello");
        assert_eq!(truncate_string("hello world", 8), "hello...");
        assert_eq!(truncate_string("hi", 2), "hi");
        assert_eq!(truncate_string("hello", 3), "hel"); // edge case where max_len <= 3
    }
}