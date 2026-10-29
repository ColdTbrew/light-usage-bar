use serde_json::Value;
use usage_core::FetchError;

pub(crate) fn successful_code(value: &Value) -> Result<(), FetchError> {
    let success = match value.get("code") {
        None => true,
        Some(Value::Number(n)) => n.as_i64() == Some(0),
        Some(Value::String(s)) => {
            s == "0" || s.eq_ignore_ascii_case("ok") || s.eq_ignore_ascii_case("success")
        }
        _ => false,
    };
    success.then_some(()).ok_or(FetchError::InvalidResponse)
}

pub(super) fn email(bytes: &[u8]) -> Result<String, FetchError> {
    let value: Value = serde_json::from_slice(bytes).map_err(|_| FetchError::InvalidResponse)?;
    successful_code(&value)?;
    let email = value
        .pointer("/userStatus/email")
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|s| !s.is_empty() && s.len() <= 320 && !s.chars().any(char::is_control))
        .ok_or(FetchError::NeedsVerification)?;
    Ok(email.to_ascii_lowercase())
}

#[cfg(any(target_os = "macos", test))]
pub(super) fn csrf_from_procargs(bytes: &[u8]) -> Result<String, FetchError> {
    let argc = bytes
        .get(..4)
        .and_then(|v| <[u8; 4]>::try_from(v).ok())
        .map(i32::from_ne_bytes)
        .filter(|n| (1..=256).contains(n))
        .ok_or(FetchError::NeedsVerification)? as usize;
    let rest = &bytes[4..];
    let executable_end = rest
        .iter()
        .position(|b| *b == 0)
        .ok_or(FetchError::NeedsVerification)?;
    let start = rest[executable_end..]
        .iter()
        .position(|b| *b != 0)
        .map(|n| n + executable_end)
        .ok_or(FetchError::NeedsVerification)?;
    let mut remaining = &rest[start..];
    let mut args = Vec::with_capacity(argc);
    for _ in 0..argc {
        let end = remaining
            .iter()
            .position(|b| *b == 0)
            .ok_or(FetchError::NeedsVerification)?;
        args.push(&remaining[..end]);
        remaining = &remaining[end + 1..];
    }
    let mut token = None;
    let mut index = 0;
    while index < args.len() {
        let value = if args[index] == b"--csrf_token" {
            index += 1;
            Some(*args.get(index).ok_or(FetchError::NeedsVerification)?)
        } else {
            args[index].strip_prefix(b"--csrf_token=")
        };
        if let Some(value) = value {
            if token.is_some()
                || value.is_empty()
                || value.len() > 4096
                || !value.iter().all(|b| (33..=126).contains(b))
            {
                return Err(FetchError::NeedsVerification);
            }
            token =
                Some(String::from_utf8(value.to_vec()).map_err(|_| FetchError::NeedsVerification)?);
        }
        index += 1;
    }
    token.ok_or(FetchError::NeedsVerification)
}

#[cfg(test)]
mod tests {
    use super::*;
    fn argv(args: &[&[u8]], environment: &[u8]) -> Vec<u8> {
        let mut bytes = (args.len() as i32).to_ne_bytes().to_vec();
        bytes.extend_from_slice(b"/synthetic/server\0\0");
        for arg in args {
            bytes.extend_from_slice(arg);
            bytes.push(0);
        }
        bytes.extend_from_slice(environment);
        bytes
    }
    #[test]
    fn csrf_is_only_taken_from_exact_bounded_argv_flags() {
        assert_eq!(
            csrf_from_procargs(&argv(
                &[b"server", b"--csrf_token", b"synthetic"],
                b"--csrf_token=environment\0"
            ))
            .unwrap(),
            "synthetic"
        );
        assert_eq!(
            csrf_from_procargs(&argv(&[b"server", b"--csrf_token=synthetic"], b"")).unwrap(),
            "synthetic"
        );
        for args in [
            vec![b"server".as_slice()],
            vec![b"server", b"--csrf_token"],
            vec![b"server", b"--csrf_token="],
            vec![b"server", b"--csrf_token=a", b"--csrf_token=b"],
            vec![b"server", b"--csrf_token=a\nb"],
        ] {
            assert_eq!(
                csrf_from_procargs(&argv(&args, b"--csrf_token=environment\0")),
                Err(FetchError::NeedsVerification)
            );
        }
    }
    #[test]
    fn malformed_argv_cannot_read_environment_as_a_token() {
        let valid = argv(&[b"server", b"--csrf_token=synthetic"], b"");
        for cut in 0..valid.len() {
            assert!(csrf_from_procargs(&valid[..cut]).is_err());
        }
        let mut excessive = valid.clone();
        excessive[..4].copy_from_slice(&257i32.to_ne_bytes());
        assert_eq!(
            csrf_from_procargs(&excessive),
            Err(FetchError::NeedsVerification)
        );
        assert_eq!(
            csrf_from_procargs(&argv(&[b"server"], b"--csrf_token=environment\0")),
            Err(FetchError::NeedsVerification)
        );
    }
    #[test]
    fn status_requires_success_and_nonblank_identity_but_no_plan() {
        assert_eq!(
            email(br#"{"userStatus":{"email":" A@example.invalid "}}"#).unwrap(),
            "a@example.invalid"
        );
        assert_eq!(
            email(br#"{"code":7,"userStatus":{"email":"a@example.invalid"}}"#),
            Err(FetchError::InvalidResponse)
        );
        for response in [br#"{}"#.as_slice(), br#"{"userStatus":{"email":" "}}"#] {
            assert_eq!(email(response), Err(FetchError::NeedsVerification));
        }
    }
}
