// Station credential and limit contract : credential checks, named
// timeouts and error text. Expected values come from the requirements
// and the contract text only. The staged association / DHCP behaviour
// (order, bounded waits, error mapping, cancellation) is tested through
// `session::run` in upload_session.rs.

use embassy_time::Duration;
use pulp_host::apps::upload_connect::{
    ASSOCIATE_TIMEOUT, ConnectError, DHCP_TIMEOUT, Limits, check_credentials,
};

const ALL_ERRORS: [ConnectError; 5] = [
    ConnectError::MissingCredentials,
    ConnectError::InvalidCredentials,
    ConnectError::AssociationFailed,
    ConnectError::AssociationTimeout,
    ConnectError::DhcpTimeout,
];

// ---------------------------------------------------------------- 1. credentials

#[test]
fn empty_ssid_is_missing_credentials_regardless_of_password() {
    // contract: SSID length 0 -> MissingCredentials (any password)
    let passwords: [&[u8]; 5] = [b"", b"short", b"12345678", &[b'p'; 63], &[b'p'; 64]];
    for password in passwords {
        assert_eq!(
            check_credentials(b"", password),
            Err(ConnectError::MissingCredentials),
            "password len {}",
            password.len()
        );
    }
}

#[test]
fn ssid_up_to_32_bytes_with_valid_password_is_ok() {
    // contract: SSID 1..=32 and password 8..=63 -> Ok
    assert_eq!(check_credentials(&[b's'; 32], &[b'p'; 8]), Ok(()));
    assert_eq!(check_credentials(b"a", b"12345678"), Ok(())); // 1-byte SSID boundary
    assert_eq!(check_credentials(b"home", b"correct horse"), Ok(()));
}

#[test]
fn ssid_over_32_bytes_is_invalid() {
    // contract: SSID length > 32 -> InvalidCredentials
    assert_eq!(
        check_credentials(&[b's'; 33], &[b'p'; 8]),
        Err(ConnectError::InvalidCredentials)
    );
    assert_eq!(
        check_credentials(&[b's'; 100], &[b'p'; 20]),
        Err(ConnectError::InvalidCredentials)
    );
}

#[test]
fn password_outside_8_to_63_bytes_is_invalid() {
    // contract: SSID 1..=32 and password length not in 8..=63 (incl. empty)
    for len in [0usize, 1, 7, 64, 65, 200] {
        assert_eq!(
            check_credentials(b"home", &vec![b'p'; len]),
            Err(ConnectError::InvalidCredentials),
            "password len {len}"
        );
    }
}

#[test]
fn password_boundaries_8_and_63_are_ok() {
    // contract: 8..=63 inclusive
    assert_eq!(check_credentials(b"home", &[b'p'; 8]), Ok(()));
    assert_eq!(check_credentials(b"home", &[b'p'; 63]), Ok(()));
}

#[test]
fn lengths_are_counted_in_bytes_not_chars() {
    // contract: lengths are bytes. "網" is 3 UTF-8 bytes.
    let ssid_32 = format!("{}ab", "網".repeat(10)); // 30 + 2 = 32 bytes, 12 chars
    assert_eq!(ssid_32.len(), 32);
    let pw_63 = "密".repeat(21); // 63 bytes, 21 chars
    assert_eq!(pw_63.len(), 63);
    assert_eq!(
        check_credentials(ssid_32.as_bytes(), pw_63.as_bytes()),
        Ok(())
    );

    // 11 chars but 33 bytes -> too long
    let ssid_33 = "網".repeat(11);
    assert_eq!(ssid_33.len(), 33);
    assert_eq!(
        check_credentials(ssid_33.as_bytes(), b"12345678"),
        Err(ConnectError::InvalidCredentials)
    );

    // 3 chars = 9 bytes: valid although fewer than 8 chars would suggest otherwise
    assert_eq!(
        check_credentials("網".as_bytes(), "密碼哈".as_bytes()),
        Ok(())
    );
    // 2 chars = 6 bytes: invalid
    assert_eq!(
        check_credentials(b"home", "密碼".as_bytes()),
        Err(ConnectError::InvalidCredentials)
    );
    // 22 chars = 66 bytes: invalid although < 63 chars
    let pw_66 = "密".repeat(22);
    assert_eq!(pw_66.len(), 66);
    assert_eq!(
        check_credentials(b"home", pw_66.as_bytes()),
        Err(ConnectError::InvalidCredentials)
    );
}

// ---------------------------------------------------------------- 2. limits

#[test]
fn default_limits_use_the_named_timeouts() {
    // contract: Limits::DEFAULT = { ASSOCIATE_TIMEOUT, DHCP_TIMEOUT }
    assert_eq!(Limits::DEFAULT.associate, ASSOCIATE_TIMEOUT);
    assert_eq!(Limits::DEFAULT.dhcp, DHCP_TIMEOUT);
}

#[test]
fn named_timeouts_are_finite_and_nonzero_with_contract_values() {
    // "設定期限": a real, finite deadline. contract: 20 s / 15 s.
    for t in [ASSOCIATE_TIMEOUT, DHCP_TIMEOUT] {
        assert!(t > Duration::from_ticks(0), "zero timeout");
        assert!(t < Duration::MAX, "unbounded timeout");
    }
    assert_eq!(ASSOCIATE_TIMEOUT, Duration::from_secs(20));
    assert_eq!(DHCP_TIMEOUT, Duration::from_secs(15));
}

// ---------------------------------------------------------------- 3. error text

#[test]
fn every_error_has_displayable_distinct_text() {
    // "顯示": contract: lines() non-empty, first line distinct per variant.
    let mut firsts = Vec::new();
    for e in ALL_ERRORS {
        let lines = e.lines();
        assert!(!lines.is_empty(), "{e:?} has no lines");
        for line in lines {
            assert!(!line.is_empty(), "{e:?} has an empty line");
        }
        firsts.push(lines[0]);
    }
    for (i, a) in firsts.iter().enumerate() {
        for b in &firsts[i + 1..] {
            assert_ne!(a, b, "two variants share the first line {a:?}");
        }
    }
}
