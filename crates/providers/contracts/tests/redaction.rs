//! Secrets, cookies and signed URLs never appear in debug output or error messages.

use parabolica_domain::provider::UnavailableReason;
use parabolica_provider_contracts::account::{AccountProblem, AccountStatus, Connection};
use parabolica_provider_contracts::auth::{BrowserAuthorization, SignedIn};
use parabolica_provider_contracts::credentials::{CredentialError, StoredCredential};
use parabolica_provider_contracts::error::{ProviderError, ProviderErrorKind};
use parabolica_provider_contracts::secret::{Secret, redact_url};
use parabolica_provider_contracts::service::{AccountIdentity, ServiceId};

const TOKEN: &str = "ya29.disposable-access-token-91c2";
const COOKIE: &str = "SAPISID=disposable-cookie-value-04ab";
const SIGNED_URL: &str = "https://rr3---sn.example.googlevideo.test/videoplayback/sig/AOq0QJ8w/expire/1700000000?sig=disposable-signature&lsig=disposable-lsig#t=10";

#[test]
fn secrets_print_redacted() {
    let credential = StoredCredential {
        account: AccountIdentity {
            id: "account-1".to_owned(),
            display_name: "Test account".to_owned(),
        },
        secret: Secret::new(format!("{TOKEN}; {COOKIE}")),
    };
    let signed_in = SignedIn {
        account: credential.account.clone(),
        secret: credential.secret.clone(),
    };
    for text in [format!("{credential:?}"), format!("{signed_in:#?}")] {
        assert!(!text.contains(TOKEN), "{text}");
        assert!(!text.contains("disposable-cookie-value"), "{text}");
        assert!(text.contains("Secret([redacted])"));
        assert!(text.contains("account-1"), "the account stays identifiable");
    }

    let authorization = BrowserAuthorization::new().unwrap();
    let text = format!("{authorization:?}");
    assert!(!text.contains(authorization.state()));
    assert!(!text.contains(authorization.code_verifier().expose_str().unwrap()));
}

#[test]
fn signed_urls_keep_only_scheme_and_host() {
    let redacted = redact_url(SIGNED_URL);
    assert_eq!(
        redacted,
        "https://rr3---sn.example.googlevideo.test/[redacted]"
    );
    assert_eq!(
        redact_url("https://user:password@example.test/path"),
        "https://example.test/[redacted]"
    );
    assert_eq!(redact_url("not a url with sig=secret"), "[redacted URL]");
}

#[test]
fn errors_name_the_service_and_problem_without_values() {
    let errors = [
        ProviderError::unavailable(ServiceId::YOUTUBE_MUSIC, UnavailableReason::SessionExpired),
        ProviderError::new(
            ServiceId::DISCOGS,
            CredentialError::Os {
                operation: "save",
                code: 0x8007_0005,
            },
        ),
        ProviderError::new(
            ServiceId::Scrobbling,
            ProviderErrorKind::Protocol {
                detail: format!("unexpected redirect to {}", redact_url(SIGNED_URL)),
            },
        ),
    ];
    let expected = [
        "YouTube Music: session expired",
        "Discogs: the system credential store could not save the sign-in (error 0x80070005)",
        "Scrobbling: unexpected response: unexpected redirect to https://rr3---sn.example.googlevideo.test/[redacted]",
    ];
    for (error, expected) in errors.iter().zip(expected) {
        assert_eq!(error.to_string(), expected);
        let debug = format!("{error:?}");
        assert!(!debug.contains("disposable"), "{debug}");
    }

    let status = AccountStatus {
        service: ServiceId::YOUTUBE_MUSIC,
        connection: Connection::Expired(AccountIdentity {
            id: "account-1".to_owned(),
            display_name: "Test account".to_owned(),
        }),
        signing_in: false,
        problem: Some(AccountProblem::Revoke(errors[0].clone())),
    };
    assert!(!format!("{status:?}").contains("disposable"));
}
