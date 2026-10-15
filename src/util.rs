use std::io;
use std::sync::{Mutex, MutexGuard};
use std::time::{SystemTime, UNIX_EPOCH};

pub fn now_ms() -> u64 {
    SystemTime::now().duration_since(UNIX_EPOCH).map(|d| u64::try_from(d.as_millis()).unwrap_or(u64::MAX)).unwrap_or(0)
}

pub fn random_bytes<const N: usize>() -> io::Result<[u8; N]> {
    let mut buf = [0u8; N];
    getrandom::fill(&mut buf).map_err(|e| io::Error::other(e.to_string()))?;
    Ok(buf)
}

pub fn random_token() -> io::Result<String> {
    Ok(to_hex(&random_bytes::<32>()?))
}

pub fn new_uuid_v4() -> io::Result<String> {
    let mut bytes = random_bytes::<16>()?;
    bytes[6] = (bytes[6] & 0x0f) | 0x40;
    bytes[8] = (bytes[8] & 0x3f) | 0x80;
    Ok(format_uuid(&bytes))
}

pub fn format_uuid(bytes: &[u8; 16]) -> String {
    let hex = to_hex(bytes);
    format!("{}-{}-{}-{}-{}", &hex[0..8], &hex[8..12], &hex[12..16], &hex[16..20], &hex[20..32])
}

pub fn is_uuid(value: &str) -> bool {
    let bytes = value.as_bytes();
    bytes.len() == 36
        && bytes.iter().enumerate().all(|(i, b)| match i {
            8 | 13 | 18 | 23 => *b == b'-',
            _ => b.is_ascii_hexdigit(),
        })
}

pub fn to_hex(bytes: &[u8]) -> String {
    const DIGITS: &[u8; 16] = b"0123456789abcdef";
    let mut out = String::with_capacity(bytes.len() * 2);
    for b in bytes {
        out.push(char::from(DIGITS[usize::from(b >> 4)]));
        out.push(char::from(DIGITS[usize::from(b & 0x0f)]));
    }
    out
}

pub fn constant_time_eq(a: &[u8], b: &[u8]) -> bool {
    if a.len() != b.len() {
        return false;
    }
    a.iter().zip(b).fold(0u8, |acc, (x, y)| acc | (x ^ y)) == 0
}

pub fn lock<T>(mutex: &Mutex<T>) -> MutexGuard<'_, T> {
    mutex.lock().unwrap_or_else(|poisoned| poisoned.into_inner())
}

pub fn fnv1a64(data: &[u8]) -> u64 {
    let mut hash: u64 = 0xcbf2_9ce4_8422_2325;
    for byte in data {
        hash ^= u64::from(*byte);
        hash = hash.wrapping_mul(0x0100_0000_01b3);
    }
    hash
}

pub fn civil_date_from_ms(ms: u64) -> (i64, u32, u32) {
    let days = i64::try_from(ms / 86_400_000).unwrap_or(0);
    let z = days + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z.rem_euclid(146_097);
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let day = u32::try_from(doy - (153 * mp + 2) / 5 + 1).unwrap_or(1);
    let month = u32::try_from(if mp < 10 { mp + 3 } else { mp - 9 }).unwrap_or(1);
    let year = yoe + era * 400 + i64::from(month <= 2);
    (year, month, day)
}

fn days_from_civil(year: i64, month: i64, day: i64) -> i64 {
    let year = if month <= 2 { year - 1 } else { year };
    let era = year.div_euclid(400);
    let yoe = year.rem_euclid(400);
    let doy = (153 * ((month + 9) % 12) + 2) / 5 + day - 1;
    let doe = yoe * 365 + yoe / 4 - yoe / 100 + doy;
    era * 146_097 + doe - 719_468
}

pub fn rfc3339_to_ms(text: &str) -> Option<u64> {
    let bytes = text.as_bytes();
    let separators = [(4, b'-'), (7, b'-'), (13, b':'), (16, b':')];
    if bytes.len() < 20
        || !matches!(bytes[10], b'T' | b't' | b' ')
        || separators.iter().any(|(index, expected)| bytes[*index] != *expected)
    {
        return None;
    }
    let number = |from: usize, to: usize| {
        text.get(from..to).filter(|s| s.bytes().all(|b| b.is_ascii_digit()))?.parse::<i64>().ok()
    };
    let (year, month, day) = (number(0, 4)?, number(5, 7)?, number(8, 10)?);
    let (hour, minute, second) = (number(11, 13)?, number(14, 16)?, number(17, 19)?);
    if !(1..=12).contains(&month) || !(1..=31).contains(&day) || hour > 23 || minute > 59 || second > 60 {
        return None;
    }
    let mut rest = &text[19..];
    let mut millis = 0;
    if let Some(fraction) = rest.strip_prefix('.') {
        let digits = fraction.bytes().take_while(u8::is_ascii_digit).count();
        if digits == 0 {
            return None;
        }
        let kept = &fraction[..digits.min(3)];
        millis = kept.parse::<i64>().ok()? * 10_i64.pow(3 - u32::try_from(kept.len()).ok()?);
        rest = &fraction[digits..];
    }
    let offset = match rest {
        "Z" | "z" => 0,
        _ => {
            let sign = match rest.as_bytes().first() {
                Some(b'+') => 1,
                Some(b'-') => -1,
                _ => return None,
            };
            if rest.len() != 6 || rest.as_bytes()[3] != b':' {
                return None;
            }
            let (hours, minutes) = (rest.get(1..3)?.parse::<i64>().ok()?, rest.get(4..6)?.parse::<i64>().ok()?);
            sign * (hours * 3_600 + minutes * 60)
        }
    };
    let seconds = days_from_civil(year, month, day) * 86_400 + hour * 3_600 + minute * 60 + second - offset;
    u64::try_from(seconds * 1_000 + millis).ok()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn uuid_v4_has_canonical_shape_and_version_bits() {
        let id = new_uuid_v4().unwrap();
        assert!(is_uuid(&id));
        assert_eq!(&id[14..15], "4");
        assert!(matches!(&id[19..20], "8" | "9" | "a" | "b"));
    }

    #[test]
    fn uuids_are_unique() {
        let a = new_uuid_v4().unwrap();
        let b = new_uuid_v4().unwrap();
        assert_ne!(a, b);
    }

    #[test]
    fn is_uuid_rejects_malformed_values() {
        assert!(!is_uuid("not-a-uuid"));
        assert!(!is_uuid("123e4567-e89b-12d3-a456-42661417400g"));
        assert!(!is_uuid("123e4567-e89b-12d3-a456-4266141740000"));
        assert!(is_uuid("123e4567-e89b-12d3-a456-426614174000"));
    }

    #[test]
    fn token_is_64_hex_chars() {
        let token = random_token().unwrap();
        assert_eq!(token.len(), 64);
        assert!(token.bytes().all(|b| b.is_ascii_hexdigit()));
    }

    #[test]
    fn constant_time_eq_compares_content_and_length() {
        assert!(constant_time_eq(b"abc", b"abc"));
        assert!(!constant_time_eq(b"abc", b"abd"));
        assert!(!constant_time_eq(b"abc", b"abcd"));
    }

    #[test]
    fn hex_encoding_is_lowercase() {
        assert_eq!(to_hex(&[0x00, 0xab, 0xff]), "00abff");
    }

    #[test]
    fn civil_date_matches_known_dates() {
        assert_eq!(civil_date_from_ms(0), (1970, 1, 1));
        assert_eq!(civil_date_from_ms(951_782_400_000), (2000, 2, 29));
        assert_eq!(civil_date_from_ms(1_791_936_000_000), (2026, 10, 14));
    }

    #[test]
    fn rfc3339_timestamps_convert_to_unix_ms() {
        assert_eq!(rfc3339_to_ms("1970-01-01T00:00:00Z"), Some(0));
        assert_eq!(rfc3339_to_ms("2000-02-29T00:00:00Z"), Some(951_782_400_000));
        assert_eq!(rfc3339_to_ms("2026-10-16T00:00:00.278740+00:00"), Some(1_792_108_800_278));
        assert_eq!(rfc3339_to_ms("2026-10-16T03:00:00.5+03:00"), Some(1_792_108_800_500));
        assert_eq!(rfc3339_to_ms("2026-10-15T21:00:00-03:00"), Some(1_792_108_800_000));
        let (year, month, day) = civil_date_from_ms(rfc3339_to_ms("2024-03-01T12:00:00Z").unwrap());
        assert_eq!((year, month, day), (2024, 3, 1));
    }

    #[test]
    fn malformed_rfc3339_timestamps_are_rejected() {
        for text in [
            "",
            "2026-10-16",
            "2026-10-16T00:00:00",
            "2026-13-16T00:00:00Z",
            "2026-10-16T24:00:00Z",
            "2026-10-16T00:00:00.Z",
            "2026-10-16T00:00:00+0300",
            "2026/10/16T00:00:00Z",
            "+026-10-16T00:00:00Z",
            "1969-12-31T23:59:59Z",
        ] {
            assert_eq!(rfc3339_to_ms(text), None, "{text:?}");
        }
    }

    #[test]
    fn fnv_is_stable() {
        assert_eq!(fnv1a64(b""), 0xcbf2_9ce4_8422_2325);
        assert_ne!(fnv1a64(b"a"), fnv1a64(b"b"));
    }
}
